use std::{
    future::Future,
    process::Command,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Router,
    extract::{Query, State},
    response::Html,
    routing::get,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::StreamExt;
use keyring::Entry;
use reqwest::{Client, redirect::Policy};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use tokio::{
    net::TcpListener,
    sync::{Mutex, oneshot},
    time::timeout,
};
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

const KEYRING_SERVICE: &str = "sql-mcp-gateway";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const REFRESH_EARLY_SECONDS: u64 = 60;
const MAX_OAUTH_RESPONSE_BYTES: usize = 1_048_576;

#[derive(Debug, thiserror::Error)]
pub enum StdioAuthError {
    #[error("the MCP server URL is invalid or insecure")]
    InvalidServer,
    #[error("OAuth discovery or registration failed")]
    Discovery,
    #[error("the system credential store is unavailable")]
    CredentialStore,
    #[error("no stored OAuth login; run sql-mcp-gateway --login <gateway-url>")]
    NotLoggedIn,
    #[error("the OAuth login did not complete")]
    Login,
    #[error("OAuth credentials expired or could not be renewed; sign in again")]
    ReauthenticationRequired,
}

#[derive(Serialize, Deserialize)]
struct StoredCredentials {
    client_id: String,
    access_token: Zeroizing<String>,
    refresh_token: Zeroizing<String>,
    token_endpoint: String,
    resource: String,
    expires_at: u64,
}

#[derive(Clone)]
struct CredentialStore {
    account: String,
}

impl CredentialStore {
    fn new(server: &Url) -> Self {
        let account = URL_SAFE_NO_PAD.encode(Sha256::digest(server.as_str().as_bytes()));
        Self { account }
    }

    async fn read(&self) -> Result<StoredCredentials, StdioAuthError> {
        let account = self.account.clone();
        let secret = tokio::task::spawn_blocking(move || {
            let entry = Entry::new(KEYRING_SERVICE, &account)
                .map_err(|_| StdioAuthError::CredentialStore)?;
            match entry.get_password() {
                Ok(secret) => Ok(secret),
                Err(keyring::Error::NoEntry) => Err(StdioAuthError::NotLoggedIn),
                Err(_) => Err(StdioAuthError::CredentialStore),
            }
        })
        .await
        .map_err(|_| StdioAuthError::CredentialStore)??;
        let secret = Zeroizing::new(secret);
        serde_json::from_str(&secret).map_err(|_| StdioAuthError::CredentialStore)
    }

    async fn write(&self, credentials: &StoredCredentials) -> Result<(), StdioAuthError> {
        let account = self.account.clone();
        let serialized = Zeroizing::new(
            serde_json::to_string(credentials).map_err(|_| StdioAuthError::CredentialStore)?,
        );
        tokio::task::spawn_blocking(move || {
            let entry = Entry::new(KEYRING_SERVICE, &account)
                .map_err(|_| StdioAuthError::CredentialStore)?;
            entry
                .set_password(serialized.as_str())
                .map_err(|_| StdioAuthError::CredentialStore)
        })
        .await
        .map_err(|_| StdioAuthError::CredentialStore)?
    }

    async fn delete(&self) -> Result<(), StdioAuthError> {
        let account = self.account.clone();
        tokio::task::spawn_blocking(move || {
            let entry = Entry::new(KEYRING_SERVICE, &account)
                .map_err(|_| StdioAuthError::CredentialStore)?;
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(_) => Err(StdioAuthError::CredentialStore),
            }
        })
        .await
        .map_err(|_| StdioAuthError::CredentialStore)?
    }
}

enum ProviderKind {
    Environment(Zeroizing<String>),
    Keyring {
        store: CredentialStore,
        state: Mutex<KeyringTokenState>,
        client: Client,
    },
}

struct KeyringTokenState {
    credentials: StoredCredentials,
    reauthentication_required: bool,
}

pub struct TokenProvider(ProviderKind);

impl TokenProvider {
    pub async fn load(server_url: &str) -> Result<Self, StdioAuthError> {
        if let Ok(token) = std::env::var("SQL_MCP_ACCESS_TOKEN")
            && !token.trim().is_empty()
        {
            tracing::warn!(
                "using deprecated SQL_MCP_ACCESS_TOKEN; use --login for keyring-backed OAuth"
            );
            return Ok(Self(ProviderKind::Environment(Zeroizing::new(token))));
        }

        let server = normalize_server_url(server_url)?;
        let store = CredentialStore::new(&server);
        let credentials = store.read().await?;
        let client = oauth_http_client()?;
        Ok(Self(ProviderKind::Keyring {
            store,
            state: Mutex::new(KeyringTokenState {
                credentials,
                reauthentication_required: false,
            }),
            client,
        }))
    }

    pub async fn access_token(&self) -> Result<Zeroizing<String>, StdioAuthError> {
        match &self.0 {
            ProviderKind::Environment(token) => Ok(Zeroizing::new(token.to_string())),
            ProviderKind::Keyring {
                store,
                state,
                client,
            } => {
                let mut state = state.lock().await;
                if state.reauthentication_required {
                    return Err(StdioAuthError::ReauthenticationRequired);
                }
                let now = current_unix_time()?;
                if state.credentials.expires_at > now.saturating_add(REFRESH_EARLY_SECONDS) {
                    return Ok(Zeroizing::new(state.credentials.access_token.to_string()));
                }

                refresh_keyring_state(client, &mut state, now).await?;

                // Refresh tokens rotate. Adopt the returned token in memory even if the
                // keyring write fails; never retry the consumed old token automatically.
                let persistence = store.write(&state.credentials).await;
                if persistence.is_err() {
                    tracing::error!(
                        "OAuth token rotated but keyring persistence failed; this process can continue, but sign-in will be required after it exits"
                    );
                }
                Ok(Zeroizing::new(state.credentials.access_token.to_string()))
            }
        }
    }
}

#[derive(Deserialize)]
struct ResourceMetadata {
    resource: String,
    authorization_servers: Vec<String>,
}

#[derive(Deserialize)]
struct AuthorizationMetadata {
    issuer: String,
    authorization_endpoint: Url,
    token_endpoint: Url,
    registration_endpoint: Url,
    code_challenge_methods_supported: Vec<String>,
}

#[derive(Deserialize)]
struct RegistrationResponse {
    client_id: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Zeroizing<String>,
    refresh_token: Zeroizing<String>,
    expires_in: u64,
}

#[derive(Deserialize)]
struct CallbackQuery {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}

struct CallbackWaiter {
    sender: Mutex<Option<oneshot::Sender<CallbackQuery>>>,
    cancellation: CancellationToken,
}

pub async fn login(server_url: &str) -> Result<(), StdioAuthError> {
    let server = normalize_server_url(server_url)?;
    let store = CredentialStore::new(&server);
    let previous = match store.read().await {
        Ok(credentials) => Some(credentials),
        Err(StdioAuthError::NotLoggedIn) => None,
        Err(error) => return Err(error),
    };
    let client = oauth_http_client()?;
    let credentials = authorize_and_exchange(&server, &client, |url| async move {
        open_browser_or_print(&url);
    })
    .await?;
    store.write(&credentials).await?;
    if let Some(previous) = previous
        && let Ok(revocation_endpoint) = discover_revocation_endpoint(&client, &server).await
    {
        let revoked = client
            .post(revocation_endpoint)
            .form(&[
                ("token", previous.refresh_token.as_str()),
                ("client_id", previous.client_id.as_str()),
            ])
            .send()
            .await
            .is_ok_and(|response| response.status().is_success());
        if !revoked {
            tracing::warn!("could not confirm revocation of the previous stdio OAuth login");
        }
    }
    println!("OAuth login saved to the system credential store for this gateway.");
    Ok(())
}

async fn authorize_and_exchange<F, Fut>(
    server: &Url,
    client: &Client,
    open_browser: F,
) -> Result<StoredCredentials, StdioAuthError>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = ()>,
{
    let resource = endpoint(server, "mcp").to_string();
    let metadata = discover_authorization_server(client, server, &resource).await?;
    if !metadata
        .code_challenge_methods_supported
        .iter()
        .any(|method| method == "S256")
    {
        return Err(StdioAuthError::Discovery);
    }

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|_| StdioAuthError::Login)?;
    let callback_uri = format!(
        "http://127.0.0.1:{}/callback",
        listener
            .local_addr()
            .map_err(|_| StdioAuthError::Login)?
            .port()
    );
    let registration = client
        .post(metadata.registration_endpoint.clone())
        .json(&serde_json::json!({
            "client_name": "SQL MCP Gateway stdio",
            "redirect_uris": [callback_uri],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
            "scope": "mcp"
        }))
        .send()
        .await
        .map_err(|_| StdioAuthError::Discovery)?;
    if !registration.status().is_success() {
        return Err(StdioAuthError::Discovery);
    }
    let registration = bounded_json::<RegistrationResponse>(registration)
        .await
        .map_err(|_| StdioAuthError::Discovery)?;

    let state = random_urlsafe_value();
    let verifier = random_urlsafe_value();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut authorize_url = metadata.authorization_endpoint.clone();
    authorize_url
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &registration.client_id)
        .append_pair("redirect_uri", &callback_uri)
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("resource", &resource)
        .append_pair("scope", "mcp");

    let (sender, receiver) = oneshot::channel();
    let waiter = Arc::new(CallbackWaiter {
        sender: Mutex::new(Some(sender)),
        cancellation: CancellationToken::new(),
    });
    let app = Router::new()
        .route("/callback", get(receive_callback))
        .with_state(waiter.clone());
    let cancellation = waiter.cancellation.clone();
    let server_task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(cancellation.cancelled_owned())
            .await
    });

    open_browser(authorize_url.to_string()).await;
    let callback_result = timeout(LOGIN_TIMEOUT, receiver).await;
    waiter.cancellation.cancel();
    let _ = server_task.await;
    let callback = callback_result
        .map_err(|_| StdioAuthError::Login)?
        .map_err(|_| StdioAuthError::Login)?;
    if callback.state.as_deref() != Some(state.as_str()) || callback.error.is_some() {
        return Err(StdioAuthError::Login);
    }
    let code = callback.code.ok_or(StdioAuthError::Login)?;

    let response = client
        .post(metadata.token_endpoint.clone())
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", registration.client_id.as_str()),
            ("code", code.as_str()),
            ("redirect_uri", callback_uri.as_str()),
            ("code_verifier", verifier.as_str()),
            ("resource", resource.as_str()),
        ])
        .send()
        .await
        .map_err(|_| StdioAuthError::Login)?;
    if !response.status().is_success() {
        return Err(StdioAuthError::Login);
    }
    let tokens = bounded_json::<TokenResponse>(response)
        .await
        .map_err(|_| StdioAuthError::Login)?;
    if tokens.access_token.is_empty() || tokens.refresh_token.is_empty() || tokens.expires_in == 0 {
        return Err(StdioAuthError::Login);
    }
    Ok(StoredCredentials {
        client_id: registration.client_id,
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        token_endpoint: metadata.token_endpoint.to_string(),
        resource,
        expires_at: current_unix_time()?.saturating_add(tokens.expires_in),
    })
}

pub async fn logout(server_url: &str) -> Result<(), StdioAuthError> {
    let server = normalize_server_url(server_url)?;
    let store = CredentialStore::new(&server);
    match store.read().await {
        Ok(credentials) => {
            let client = oauth_http_client()?;
            let revocation_endpoint = discover_revocation_endpoint(&client, &server).await.ok();
            if let Some(endpoint) = revocation_endpoint {
                let response = client
                    .post(endpoint)
                    .form(&[
                        ("token", credentials.refresh_token.as_str()),
                        ("client_id", credentials.client_id.as_str()),
                    ])
                    .send()
                    .await;
                if !response.is_ok_and(|response| response.status().is_success()) {
                    tracing::warn!(
                        "could not confirm remote OAuth revocation; removing local credentials anyway"
                    );
                }
            }
        }
        Err(StdioAuthError::NotLoggedIn) => {}
        Err(error) => return Err(error),
    }
    store.delete().await?;
    println!("Local OAuth credentials removed.");
    Ok(())
}

async fn discover_authorization_server(
    client: &Client,
    server: &Url,
    resource: &str,
) -> Result<AuthorizationMetadata, StdioAuthError> {
    let resource_metadata_url = endpoint(server, ".well-known/oauth-protected-resource/mcp");
    let resource_response = client
        .get(resource_metadata_url)
        .send()
        .await
        .map_err(|_| StdioAuthError::Discovery)?;
    if !resource_response.status().is_success() {
        return Err(StdioAuthError::Discovery);
    }
    let resource_metadata = bounded_json::<ResourceMetadata>(resource_response)
        .await
        .map_err(|_| StdioAuthError::Discovery)?;
    if resource_metadata.resource != resource || resource_metadata.authorization_servers.len() != 1
    {
        return Err(StdioAuthError::Discovery);
    }
    let issuer = Url::parse(&resource_metadata.authorization_servers[0])
        .map_err(|_| StdioAuthError::Discovery)?;
    if !is_secure_url(&issuer) {
        return Err(StdioAuthError::Discovery);
    }
    let metadata_url = issuer
        .join(".well-known/oauth-authorization-server")
        .map_err(|_| StdioAuthError::Discovery)?;
    let response = client
        .get(metadata_url)
        .send()
        .await
        .map_err(|_| StdioAuthError::Discovery)?;
    if !response.status().is_success() {
        return Err(StdioAuthError::Discovery);
    }
    let metadata = bounded_json::<AuthorizationMetadata>(response)
        .await
        .map_err(|_| StdioAuthError::Discovery)?;
    if metadata.issuer.trim_end_matches('/') != issuer.as_str().trim_end_matches('/')
        || !is_secure_url(&metadata.authorization_endpoint)
        || !is_secure_url(&metadata.token_endpoint)
        || !is_secure_url(&metadata.registration_endpoint)
    {
        return Err(StdioAuthError::Discovery);
    }
    Ok(metadata)
}

async fn discover_revocation_endpoint(
    client: &Client,
    server: &Url,
) -> Result<Url, StdioAuthError> {
    #[derive(Deserialize)]
    struct Metadata {
        revocation_endpoint: Url,
    }
    let response = client
        .get(endpoint(server, ".well-known/oauth-authorization-server"))
        .send()
        .await
        .map_err(|_| StdioAuthError::Discovery)?;
    if !response.status().is_success() {
        return Err(StdioAuthError::Discovery);
    }
    let metadata = bounded_json::<Metadata>(response)
        .await
        .map_err(|_| StdioAuthError::Discovery)?;
    if !is_secure_url(&metadata.revocation_endpoint) {
        return Err(StdioAuthError::Discovery);
    }
    Ok(metadata.revocation_endpoint)
}

async fn receive_callback(
    State(waiter): State<Arc<CallbackWaiter>>,
    Query(query): Query<CallbackQuery>,
) -> Html<&'static str> {
    if let Some(sender) = waiter.sender.lock().await.take() {
        let _ = sender.send(query);
        waiter.cancellation.cancel();
    }
    Html(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>SQL MCP Gateway</title><p>Login response received. You can close this tab and return to the terminal.</p></html>",
    )
}

fn normalize_server_url(value: &str) -> Result<Url, StdioAuthError> {
    let mut url = Url::parse(value).map_err(|_| StdioAuthError::InvalidServer)?;
    if !is_secure_url(&url)
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/" | "/mcp")
    {
        return Err(StdioAuthError::InvalidServer);
    }
    url.set_path("/");
    Ok(url)
}

fn is_secure_url(url: &Url) -> bool {
    (url.scheme() == "https"
        || (url.scheme() == "http" && url.host_str().is_some_and(is_loopback_host)))
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

fn endpoint(base: &Url, path: &str) -> Url {
    base.join(path)
        .expect("normalized OAuth server URL accepts relative endpoints")
}

fn oauth_http_client() -> Result<Client, StdioAuthError> {
    Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| StdioAuthError::Discovery)
}

async fn bounded_json<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, ()> {
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > MAX_OAUTH_RESPONSE_BYTES as u64)
    {
        return Err(());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ())?;
        if bytes.len().saturating_add(chunk.len()) > MAX_OAUTH_RESPONSE_BYTES {
            return Err(());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| ())
}

async fn refresh_credentials(
    client: &Client,
    credentials: &mut StoredCredentials,
    now: u64,
) -> Result<(), StdioAuthError> {
    let response = client
        .post(&credentials.token_endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", credentials.client_id.as_str()),
            ("refresh_token", credentials.refresh_token.as_str()),
            ("resource", credentials.resource.as_str()),
        ])
        .send()
        .await
        .map_err(|_| StdioAuthError::ReauthenticationRequired)?;
    let refreshed = bounded_json::<TokenResponse>(response)
        .await
        .map_err(|_| StdioAuthError::ReauthenticationRequired)?;
    if refreshed.access_token.is_empty()
        || refreshed.refresh_token.is_empty()
        || refreshed.expires_in == 0
    {
        return Err(StdioAuthError::ReauthenticationRequired);
    }

    *credentials = StoredCredentials {
        client_id: credentials.client_id.clone(),
        access_token: refreshed.access_token,
        refresh_token: refreshed.refresh_token,
        token_endpoint: credentials.token_endpoint.clone(),
        resource: credentials.resource.clone(),
        expires_at: now.saturating_add(refreshed.expires_in),
    };
    Ok(())
}

async fn refresh_keyring_state(
    client: &Client,
    state: &mut KeyringTokenState,
    now: u64,
) -> Result<(), StdioAuthError> {
    if state.reauthentication_required {
        return Err(StdioAuthError::ReauthenticationRequired);
    }
    if let Err(error) = refresh_credentials(client, &mut state.credentials, now).await {
        // A lost response may mean the server already rotated the token.
        // Never retry a refresh token when its consumption is uncertain.
        state.reauthentication_required = true;
        return Err(error);
    }
    Ok(())
}

fn current_unix_time() -> Result<u64, StdioAuthError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| StdioAuthError::Login)
}

fn random_urlsafe_value() -> String {
    let mut material = [0_u8; 32];
    material[..16].copy_from_slice(Uuid::now_v7().as_bytes());
    material[16..].copy_from_slice(Uuid::now_v7().as_bytes());
    URL_SAFE_NO_PAD.encode(Sha256::digest(material))
}

fn open_browser_or_print(url: &str) {
    #[cfg(target_os = "linux")]
    let result = Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let result: std::io::Result<std::process::Child> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "no browser launcher",
    ));
    if result.is_err() {
        println!("Open this authorization URL in a browser:\n{url}");
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use axum::{
        Json, Router,
        extract::{Form, Query, State},
        http::{StatusCode, header},
        response::Redirect,
        routing::{get, post},
    };
    use reqwest::{Client, redirect::Policy};
    use serde_json::{Value, json};
    use tokio::net::TcpListener;
    use url::Url;
    use zeroize::Zeroizing;

    use super::{
        KeyringTokenState, StoredCredentials, authorize_and_exchange, is_secure_url,
        normalize_server_url, oauth_http_client, random_urlsafe_value, refresh_credentials,
        refresh_keyring_state,
    };

    #[derive(Clone)]
    struct FakeRefreshState {
        received: Arc<Mutex<Option<HashMap<String, String>>>>,
        calls: Arc<AtomicUsize>,
        reject: bool,
    }

    #[derive(Clone)]
    struct FakeOAuthState {
        issuer: String,
        resource: String,
        registration: Arc<Mutex<Option<Value>>>,
        authorization: Arc<Mutex<Option<HashMap<String, String>>>>,
        token: Arc<Mutex<Option<HashMap<String, String>>>>,
    }

    async fn fake_resource_metadata(State(state): State<FakeOAuthState>) -> Json<Value> {
        Json(json!({
            "resource": state.resource,
            "authorization_servers": [state.issuer]
        }))
    }

    async fn fake_authorization_metadata(State(state): State<FakeOAuthState>) -> Json<Value> {
        Json(json!({
            "issuer": state.issuer,
            "authorization_endpoint": format!("{}authorize", state.issuer),
            "token_endpoint": format!("{}token", state.issuer),
            "registration_endpoint": format!("{}register", state.issuer),
            "code_challenge_methods_supported": ["S256"]
        }))
    }

    async fn fake_registration(
        State(state): State<FakeOAuthState>,
        Json(registration): Json<Value>,
    ) -> Json<Value> {
        *state
            .registration
            .lock()
            .expect("DCR capture lock is healthy") = Some(registration);
        Json(json!({ "client_id": "synthetic-client-id" }))
    }

    async fn fake_authorize(
        State(state): State<FakeOAuthState>,
        Query(query): Query<HashMap<String, String>>,
    ) -> Result<Redirect, StatusCode> {
        *state
            .authorization
            .lock()
            .expect("authorize capture lock is healthy") = Some(query.clone());
        let redirect_uri = query.get("redirect_uri").ok_or(StatusCode::BAD_REQUEST)?;
        let state_parameter = query.get("state").ok_or(StatusCode::BAD_REQUEST)?;
        let mut redirect = Url::parse(redirect_uri).map_err(|_| StatusCode::BAD_REQUEST)?;
        redirect
            .query_pairs_mut()
            .append_pair("code", "synthetic-authorization-code")
            .append_pair("state", state_parameter);
        Ok(Redirect::to(redirect.as_str()))
    }

    async fn fake_authorization_code_exchange(
        State(state): State<FakeOAuthState>,
        Form(form): Form<HashMap<String, String>>,
    ) -> Json<Value> {
        *state.token.lock().expect("token capture lock is healthy") = Some(form);
        Json(json!({
            "access_token": "synthetic-access-token",
            "refresh_token": "synthetic-refresh-token",
            "expires_in": 3600
        }))
    }

    async fn start_fake_oauth_server() -> (String, FakeOAuthState, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fake OAuth listener binds");
        let address = listener
            .local_addr()
            .expect("fake OAuth listener has an address");
        let issuer = format!("http://{address}/");
        let state = FakeOAuthState {
            issuer: issuer.clone(),
            resource: format!("{issuer}mcp"),
            registration: Arc::new(Mutex::new(None)),
            authorization: Arc::new(Mutex::new(None)),
            token: Arc::new(Mutex::new(None)),
        };
        let app = Router::new()
            .route(
                "/.well-known/oauth-protected-resource/mcp",
                get(fake_resource_metadata),
            )
            .route(
                "/.well-known/oauth-authorization-server",
                get(fake_authorization_metadata),
            )
            .route("/register", post(fake_registration))
            .route("/authorize", get(fake_authorize))
            .route("/token", post(fake_authorization_code_exchange))
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("fake OAuth server serves requests");
        });
        (issuer, state, server)
    }

    async fn fake_refresh(
        State(state): State<FakeRefreshState>,
        Form(form): Form<HashMap<String, String>>,
    ) -> (StatusCode, Json<Value>) {
        state.calls.fetch_add(1, Ordering::SeqCst);
        *state
            .received
            .lock()
            .expect("fake token form lock is healthy") = Some(form);
        if state.reject {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_grant" })),
            );
        }
        (
            StatusCode::OK,
            Json(json!({
                "access_token": "replacement-access-token",
                "refresh_token": "replacement-refresh-token",
                "expires_in": 3600
            })),
        )
    }

    async fn start_fake_refresh_server(
        reject: bool,
    ) -> (
        String,
        Arc<Mutex<Option<HashMap<String, String>>>>,
        Arc<AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fake token listener binds");
        let address = listener
            .local_addr()
            .expect("fake token listener has an address");
        let received = Arc::new(Mutex::new(None));
        let calls = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route("/token", post(fake_refresh))
            .with_state(FakeRefreshState {
                received: received.clone(),
                calls: calls.clone(),
                reject,
            });
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("fake token server serves requests");
        });
        (format!("http://{address}/token"), received, calls, server)
    }

    fn synthetic_credentials(token_endpoint: String) -> StoredCredentials {
        StoredCredentials {
            client_id: "synthetic-client".to_owned(),
            access_token: Zeroizing::new("old-access-token".to_owned()),
            refresh_token: Zeroizing::new("old-refresh-token".to_owned()),
            token_endpoint,
            resource: "https://mcp.example.test/mcp".to_owned(),
            expires_at: 1,
        }
    }

    #[test]
    fn only_accepts_https_or_loopback_and_normalizes_the_mcp_endpoint() {
        let origin = normalize_server_url("https://mcp.example.test/mcp").unwrap();
        assert_eq!(origin.as_str(), "https://mcp.example.test/");
        assert!(normalize_server_url("http://mcp.example.test").is_err());
        assert!(normalize_server_url("http://127.0.0.1:8080/mcp").is_ok());
        assert!(normalize_server_url("https://mcp.example.test/other").is_err());
        assert!(is_secure_url(&Url::parse("http://[::1]:8080/").unwrap()));
    }

    #[test]
    fn generated_pkce_material_is_url_safe_and_sufficiently_long() {
        let first = random_urlsafe_value();
        let second = random_urlsafe_value();
        assert_eq!(first.len(), 43);
        assert!(
            first
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
        );
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn refresh_rotation_adopts_new_tokens_and_sends_old_refresh_once() {
        let (endpoint, received, _, server) = start_fake_refresh_server(false).await;
        let client = oauth_http_client().expect("OAuth HTTP client builds");
        let mut credentials = synthetic_credentials(endpoint);

        refresh_credentials(&client, &mut credentials, 100)
            .await
            .expect("fake refresh succeeds");

        assert_eq!(
            credentials.access_token.as_str(),
            "replacement-access-token"
        );
        assert_eq!(
            credentials.refresh_token.as_str(),
            "replacement-refresh-token"
        );
        assert_eq!(credentials.expires_at, 3700);
        let form = received
            .lock()
            .expect("fake form lock is healthy")
            .clone()
            .expect("refresh request arrived");
        assert_eq!(
            form.get("grant_type").map(String::as_str),
            Some("refresh_token")
        );
        assert_eq!(
            form.get("refresh_token").map(String::as_str),
            Some("old-refresh-token")
        );
        server.abort();
    }

    #[tokio::test]
    async fn rejected_refresh_keeps_credentials_and_requires_new_login() {
        let (endpoint, _, _, server) = start_fake_refresh_server(true).await;
        let client = oauth_http_client().expect("OAuth HTTP client builds");
        let mut credentials = synthetic_credentials(endpoint);

        assert!(
            refresh_credentials(&client, &mut credentials, 100)
                .await
                .is_err()
        );
        assert_eq!(credentials.access_token.as_str(), "old-access-token");
        assert_eq!(credentials.refresh_token.as_str(), "old-refresh-token");
        server.abort();
    }

    #[tokio::test]
    async fn uncertain_refresh_failure_is_not_retried_with_the_consumed_token() {
        let (endpoint, _, calls, server) = start_fake_refresh_server(true).await;
        let client = oauth_http_client().expect("OAuth HTTP client builds");
        let mut state = KeyringTokenState {
            credentials: synthetic_credentials(endpoint),
            reauthentication_required: false,
        };

        assert!(
            refresh_keyring_state(&client, &mut state, 100)
                .await
                .is_err()
        );
        assert!(
            refresh_keyring_state(&client, &mut state, 101)
                .await
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(state.reauthentication_required);
        server.abort();
    }

    #[tokio::test]
    async fn completes_discovery_registration_pkce_callback_and_code_exchange() {
        let (issuer, state, server) = start_fake_oauth_server().await;
        let server_url = Url::parse(&issuer).expect("fake gateway issuer parses");
        let client = oauth_http_client().expect("OAuth HTTP client builds");
        let credentials =
            authorize_and_exchange(&server_url, &client, |authorization_url| async move {
                let browser = Client::builder()
                    .redirect(Policy::none())
                    .build()
                    .expect("test browser client builds");
                let response = browser
                    .get(authorization_url)
                    .send()
                    .await
                    .expect("fake authorization page responds");
                assert_eq!(response.status(), StatusCode::SEE_OTHER);
                let callback_url = response
                    .headers()
                    .get(header::LOCATION)
                    .expect("authorization redirects to the loopback callback")
                    .to_str()
                    .expect("callback URL is valid text")
                    .to_owned();
                let callback = browser
                    .get(callback_url)
                    .send()
                    .await
                    .expect("loopback callback accepts the authorization response");
                assert!(callback.status().is_success());
            })
            .await
            .expect("the local fake OAuth server completes the client flow");

        assert_eq!(credentials.access_token.as_str(), "synthetic-access-token");
        assert_eq!(
            credentials.refresh_token.as_str(),
            "synthetic-refresh-token"
        );
        let registration = state
            .registration
            .lock()
            .expect("DCR lock is healthy")
            .clone()
            .expect("client registration was sent");
        assert_eq!(registration["token_endpoint_auth_method"], "none");
        assert!(
            registration["redirect_uris"][0]
                .as_str()
                .is_some_and(|uri| uri.starts_with("http://127.0.0.1:"))
        );
        let authorization = state
            .authorization
            .lock()
            .expect("authorization lock is healthy")
            .clone()
            .expect("authorization request was sent");
        assert_eq!(
            authorization
                .get("code_challenge_method")
                .map(String::as_str),
            Some("S256")
        );
        assert_eq!(
            authorization.get("resource").map(String::as_str),
            Some(state.resource.as_str())
        );
        let token = state
            .token
            .lock()
            .expect("token lock is healthy")
            .clone()
            .expect("authorization-code exchange was sent");
        assert_eq!(
            token.get("grant_type").map(String::as_str),
            Some("authorization_code")
        );
        assert_eq!(
            token.get("code").map(String::as_str),
            Some("synthetic-authorization-code")
        );
        assert_eq!(
            token.get("resource").map(String::as_str),
            Some(state.resource.as_str())
        );
        assert!(token.contains_key("code_verifier"));
        server.abort();
    }
}
