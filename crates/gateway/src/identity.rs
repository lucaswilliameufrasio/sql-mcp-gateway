use std::{sync::Arc, time::Instant};

use jsonwebtoken::{
    Algorithm, DecodingKey, Validation, decode, decode_header,
    jwk::{Jwk, JwkSet, PublicKeyUse},
};
use reqwest::{Client, redirect::Policy};
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};
use url::Url;

use crate::{api_error::ApiError, domain::OidcIdentity};

#[derive(Clone)]
pub struct OidcClient {
    issuer: String,
    audience: String,
    authorization_endpoint: Url,
    token_endpoint: Url,
    userinfo_endpoint: Url,
    jwks_endpoint: Url,
    jwks: Arc<RwLock<JwkSet>>,
    last_jwks_refresh: Arc<Mutex<Option<Instant>>>,
    client: Client,
}

#[derive(Debug, thiserror::Error)]
pub enum OidcError {
    #[error("OIDC discovery failed")]
    Discovery,
    #[error("OIDC token exchange failed")]
    TokenExchange,
}

#[derive(Deserialize)]
struct ProviderMetadata {
    issuer: String,
    authorization_endpoint: Url,
    token_endpoint: Url,
    userinfo_endpoint: Url,
    jwks_uri: Url,
}

#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
}

#[derive(Deserialize)]
struct UserInfo {
    sub: String,
    email: Option<String>,
    name: Option<String>,
    preferred_username: Option<String>,
}

#[derive(Deserialize)]
struct VerifiedAccessToken {
    sub: String,
}

impl OidcClient {
    pub async fn discover(issuer: Url, audience: String) -> Result<Self, OidcError> {
        let client = Client::builder()
            .redirect(Policy::none())
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|_| OidcError::Discovery)?;
        let mut metadata_url = issuer.clone();
        {
            let mut path = metadata_url
                .path_segments_mut()
                .map_err(|_| OidcError::Discovery)?;
            path.pop_if_empty();
            path.push(".well-known");
            path.push("openid-configuration");
        }

        let response = client
            .get(metadata_url)
            .send()
            .await
            .map_err(|_| OidcError::Discovery)?;
        if !response.status().is_success() {
            return Err(OidcError::Discovery);
        }

        let metadata = response
            .json::<ProviderMetadata>()
            .await
            .map_err(|_| OidcError::Discovery)?;
        if metadata.issuer != issuer.as_str()
            || !is_secure_provider_endpoint(&metadata.authorization_endpoint)
            || !is_secure_provider_endpoint(&metadata.token_endpoint)
            || !is_secure_provider_endpoint(&metadata.userinfo_endpoint)
            || !is_secure_provider_endpoint(&metadata.jwks_uri)
        {
            return Err(OidcError::Discovery);
        }

        let jwks = fetch_jwks(&client, &metadata.jwks_uri)
            .await
            .map_err(|_| OidcError::Discovery)?;

        Ok(Self {
            issuer: metadata.issuer,
            audience,
            authorization_endpoint: metadata.authorization_endpoint,
            token_endpoint: metadata.token_endpoint,
            userinfo_endpoint: metadata.userinfo_endpoint,
            jwks_endpoint: metadata.jwks_uri,
            jwks: Arc::new(RwLock::new(jwks)),
            last_jwks_refresh: Arc::new(Mutex::new(None)),
            client,
        })
    }

    pub async fn authenticate(&self, access_token: &str) -> Result<OidcIdentity, ApiError> {
        if access_token.is_empty() || access_token.len() > 16_384 {
            return Err(ApiError::Unauthorized);
        }

        let verified_subject = self.verify_access_token(access_token).await?;
        let response = self
            .client
            .get(self.userinfo_endpoint.clone())
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|_| ApiError::DependencyUnavailable)?;
        if !response.status().is_success() {
            return Err(ApiError::Unauthorized);
        }

        let userinfo = response
            .json::<UserInfo>()
            .await
            .map_err(|_| ApiError::Unauthorized)?;
        if userinfo.sub.trim().is_empty() || userinfo.sub != verified_subject {
            return Err(ApiError::Unauthorized);
        }

        let display_name = userinfo.name.or(userinfo.preferred_username);
        Ok(OidcIdentity {
            issuer: self.issuer.clone(),
            subject: userinfo.sub,
            email: userinfo.email,
            display_name,
        })
    }

    async fn verify_access_token(&self, token: &str) -> Result<String, ApiError> {
        let header = decode_header(token).map_err(|_| ApiError::Unauthorized)?;
        if !supported_signing_algorithm(header.alg) {
            return Err(ApiError::Unauthorized);
        }
        let key_id = header.kid.ok_or(ApiError::Unauthorized)?;
        let jwk = self.find_signing_key(&key_id).await?;
        if jwk
            .common
            .public_key_use
            .as_ref()
            .is_some_and(|key_use| key_use != &PublicKeyUse::Signature)
        {
            return Err(ApiError::Unauthorized);
        }
        if jwk
            .common
            .key_algorithm
            .is_some_and(|key_algorithm| key_algorithm != header.alg.into())
        {
            return Err(ApiError::Unauthorized);
        }

        let key = DecodingKey::from_jwk(&jwk).map_err(|_| ApiError::Unauthorized)?;
        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.set_audience(&[self.audience.as_str()]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.validate_nbf = true;
        validation.leeway = 30;
        validation.reject_tokens_expiring_in_less_than = 5;

        let claims = decode::<VerifiedAccessToken>(token, &key, &validation)
            .map_err(|_| ApiError::Unauthorized)?
            .claims;
        if claims.sub.trim().is_empty() {
            return Err(ApiError::Unauthorized);
        }

        Ok(claims.sub)
    }

    async fn find_signing_key(&self, key_id: &str) -> Result<Jwk, ApiError> {
        if let Some(key) = self.jwks.read().await.find(key_id).cloned() {
            return Ok(key);
        }

        let mut last_refresh = self.last_jwks_refresh.lock().await;
        if last_refresh.is_some_and(|last| last.elapsed() < std::time::Duration::from_secs(30)) {
            return Err(ApiError::Unauthorized);
        }
        *last_refresh = Some(Instant::now());
        drop(last_refresh);

        let latest = fetch_jwks(&self.client, &self.jwks_endpoint)
            .await
            .map_err(|_| ApiError::DependencyUnavailable)?;
        let key = latest.find(key_id).cloned().ok_or(ApiError::Unauthorized)?;
        *self.jwks.write().await = latest;
        Ok(key)
    }

    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub fn authorization_url(
        &self,
        client_id: &str,
        redirect_uri: &str,
        state: &str,
        code_challenge: &str,
        resource: &str,
    ) -> Url {
        let mut url = self.authorization_endpoint.clone();
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", "openid profile email")
            .append_pair("state", state)
            .append_pair("code_challenge", code_challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("resource", resource);
        url
    }

    pub async fn exchange_authorization_code(
        &self,
        client_id: &str,
        redirect_uri: &str,
        code: &str,
        code_verifier: &str,
        resource: &str,
    ) -> Result<TokenResponse, OidcError> {
        let form = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("grant_type", "authorization_code"),
                ("client_id", client_id),
                ("redirect_uri", redirect_uri),
                ("code", code),
                ("code_verifier", code_verifier),
                ("resource", resource),
            ])
            .finish();
        let response = self
            .client
            .post(self.token_endpoint.clone())
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form)
            .send()
            .await
            .map_err(|_| OidcError::TokenExchange)?;
        if !response.status().is_success() {
            return Err(OidcError::TokenExchange);
        }

        let token = response
            .json::<TokenResponse>()
            .await
            .map_err(|_| OidcError::TokenExchange)?;
        if token.access_token.is_empty() || !token.token_type.eq_ignore_ascii_case("bearer") {
            return Err(OidcError::TokenExchange);
        }

        Ok(token)
    }
}

async fn fetch_jwks(client: &Client, endpoint: &Url) -> Result<JwkSet, OidcError> {
    let response = client
        .get(endpoint.clone())
        .send()
        .await
        .map_err(|_| OidcError::Discovery)?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|content_length| content_length > 1_048_576)
    {
        return Err(OidcError::Discovery);
    }

    let body = response.bytes().await.map_err(|_| OidcError::Discovery)?;
    if body.len() > 1_048_576 {
        return Err(OidcError::Discovery);
    }

    serde_json::from_slice::<JwkSet>(&body).map_err(|_| OidcError::Discovery)
}

fn supported_signing_algorithm(algorithm: Algorithm) -> bool {
    matches!(
        algorithm,
        Algorithm::RS256
            | Algorithm::RS384
            | Algorithm::RS512
            | Algorithm::PS256
            | Algorithm::PS384
            | Algorithm::PS512
            | Algorithm::ES256
            | Algorithm::ES384
            | Algorithm::EdDSA
    )
}

fn is_secure_provider_endpoint(url: &Url) -> bool {
    if url.scheme() == "https" && url.host_str().is_some() {
        return true;
    }

    url.scheme() == "http" && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use axum::{
        Json, Router,
        extract::{Form, State},
        routing::{get, post},
    };
    use serde_json::{Value, json};
    use tokio::net::TcpListener;
    use url::Url;

    use super::OidcClient;

    #[derive(Clone)]
    struct ProviderState {
        issuer: String,
        captured: Arc<Mutex<Option<HashMap<String, String>>>>,
    }

    async fn provider_metadata(State(state): State<ProviderState>) -> Json<Value> {
        Json(json!({
            "issuer": state.issuer,
            "authorization_endpoint": format!("{}authorize", state.issuer),
            "token_endpoint": format!("{}token", state.issuer),
            "userinfo_endpoint": format!("{}userinfo", state.issuer),
            "jwks_uri": format!("{}jwks", state.issuer)
        }))
    }

    async fn jwks() -> Json<Value> {
        Json(json!({"keys": []}))
    }

    async fn token_exchange(
        State(state): State<ProviderState>,
        Form(form): Form<HashMap<String, String>>,
    ) -> Json<Value> {
        *state.captured.lock().expect("capture lock is not poisoned") = Some(form);
        Json(json!({"access_token": "fake-access-token", "token_type": "Bearer"}))
    }

    #[tokio::test]
    async fn exchanges_code_with_pkce_against_a_local_fake_oidc_provider() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("local listener binds");
        let address = listener.local_addr().expect("listener has an address");
        let issuer = format!("http://{address}/");
        let captured = Arc::new(Mutex::new(None));
        let app = Router::new()
            .route("/.well-known/openid-configuration", get(provider_metadata))
            .route("/jwks", get(jwks))
            .route("/token", post(token_exchange))
            .with_state(ProviderState {
                issuer: issuer.clone(),
                captured: captured.clone(),
            });
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("fake OIDC provider serves requests");
        });

        let client = OidcClient::discover(
            Url::parse(&issuer).expect("issuer URL parses"),
            "fake-gateway-audience".to_owned(),
        )
        .await
        .expect("discovery accepts a loopback-only HTTP provider");
        let token = client
            .exchange_authorization_code(
                "fake-client",
                "http://127.0.0.1:45678/oauth/callback",
                "single-use-code",
                "pkce-verifier-material",
                "https://mcp.example.test/mcp",
            )
            .await
            .expect("fake provider returns its synthetic access token");

        assert_eq!(token.access_token, "fake-access-token");
        assert_eq!(token.token_type, "Bearer");
        let form = captured
            .lock()
            .expect("capture lock is not poisoned")
            .clone()
            .expect("token endpoint captured the form");
        assert_eq!(
            form.get("grant_type").map(String::as_str),
            Some("authorization_code")
        );
        assert_eq!(
            form.get("code_verifier").map(String::as_str),
            Some("pkce-verifier-material")
        );
        assert_eq!(
            form.get("code").map(String::as_str),
            Some("single-use-code")
        );
        assert_eq!(
            form.get("resource").map(String::as_str),
            Some("https://mcp.example.test/mcp")
        );

        server.abort();
    }
}
