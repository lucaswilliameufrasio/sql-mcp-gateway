use axum::{
    Form, Json, Router,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, Postgres, Transaction, query, query_as};
use url::Url;
use uuid::Uuid;

use crate::{
    auth::{AuthenticatedPrincipal, authenticate_access_token as authenticate_oidc_access_token},
    state::AppState,
};

const ACCESS_TOKEN_TTL_SECONDS: i64 = 3600;
const REFRESH_TOKEN_TTL_SECONDS: i64 = 2_592_000;
const AUTHORIZATION_REQUEST_TTL_SECONDS: i64 = 600;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/.well-known/oauth-authorization-server",
            get(authorization_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource",
            get(resource_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(resource_metadata),
        )
        .route("/oauth/register", post(register_client))
        .route("/oauth/authorize", get(authorize))
        .route("/oauth/callback", get(callback))
        .route("/oauth/consent", get(consent_page).post(consent))
        .route("/oauth/token", post(token))
        .route("/oauth/revoke", post(revoke))
}

async fn authorization_metadata(State(state): State<AppState>) -> Json<serde_json::Value> {
    let issuer = state.mcp_public_url.as_str().trim_end_matches('/');
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": endpoint(&state.mcp_public_url, "oauth/authorize"),
        "token_endpoint": endpoint(&state.mcp_public_url, "oauth/token"),
        "revocation_endpoint": endpoint(&state.mcp_public_url, "oauth/revoke"),
        "revocation_endpoint_auth_methods_supported": ["none"],
        "registration_endpoint": endpoint(&state.mcp_public_url, "oauth/register"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "token_endpoint_auth_methods_supported": ["none"],
        "code_challenge_methods_supported": ["S256"],
        "client_id_metadata_document_supported": false
    }))
}

async fn resource_metadata(State(state): State<AppState>) -> Json<serde_json::Value> {
    let issuer = state.mcp_public_url.as_str().trim_end_matches('/');
    Json(json!({
        "resource": mcp_resource(&state.mcp_public_url, "sql"),
        "authorization_servers": [issuer],
        "scopes_supported": ["mcp"],
        "bearer_methods_supported": ["header"]
    }))
}

pub(crate) fn mcp_resource(public_url: &Url, _server_kind: &str) -> String {
    endpoint(public_url, "mcp")
}

pub(crate) fn protected_resource_metadata_path(_server_kind: &str) -> &'static str {
    "/.well-known/oauth-protected-resource/mcp"
}

fn is_supported_mcp_resource(public_url: &Url, resource: &str) -> bool {
    mcp_resource(public_url, "sql") == resource
}

fn endpoint(base: &Url, path: &str) -> String {
    base.join(path)
        .map(|url| url.to_string())
        .unwrap_or_else(|_| base.to_string())
}

#[derive(Deserialize)]
struct RegisterRequest {
    client_name: Option<String>,
    redirect_uris: Vec<String>,
    #[serde(default)]
    grant_types: Vec<String>,
    #[serde(default)]
    response_types: Vec<String>,
    #[serde(default)]
    token_endpoint_auth_method: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Serialize)]
struct RegisterResponse {
    client_id: String,
    client_name: String,
    redirect_uris: Vec<String>,
    grant_types: [&'static str; 2],
    response_types: [&'static str; 1],
    token_endpoint_auth_method: &'static str,
}

async fn register_client(
    State(state): State<AppState>,
    Json(request): Json<RegisterRequest>,
) -> Result<Response, OAuthError> {
    let client_name = request
        .client_name
        .as_deref()
        .unwrap_or("MCP Client")
        .trim();
    if client_name.is_empty() || client_name.len() > 120 {
        return Err(OAuthError::InvalidClientMetadata);
    }
    if request.redirect_uris.is_empty() || request.redirect_uris.len() > 10 {
        return Err(OAuthError::InvalidClientMetadata);
    }
    if (!request.grant_types.is_empty()
        && request
            .grant_types
            .iter()
            .any(|grant| grant != "authorization_code" && grant != "refresh_token"))
        || (!request.response_types.is_empty()
            && request
                .response_types
                .iter()
                .any(|response| response != "code"))
        || request
            .token_endpoint_auth_method
            .as_deref()
            .is_some_and(|method| method != "none")
        || request
            .scope
            .as_deref()
            .is_some_and(|scope| scope.split_whitespace().any(|scope| scope != "mcp"))
    {
        return Err(OAuthError::InvalidClientMetadata);
    }

    let mut redirects = Vec::with_capacity(request.redirect_uris.len());
    for uri in &request.redirect_uris {
        if uri.len() > 2048
            || !is_allowed_redirect_uri(uri)
            || redirects.iter().any(|item| item == uri)
        {
            return Err(OAuthError::InvalidClientMetadata);
        }
        redirects.push(uri.clone());
    }

    let client_id = random_secret("sgclient_");
    query(
        "INSERT INTO mcp_oauth_clients (client_id, client_name, redirect_uris) VALUES ($1, $2, $3)",
    )
    .bind(&client_id)
    .bind(client_name)
    .bind(serde_json::to_value(&redirects).map_err(|_| OAuthError::ServerError)?)
    .execute(&state.control_pool)
    .await
    .map_err(|_| OAuthError::ServerError)?;

    let mut response = (
        StatusCode::CREATED,
        Json(RegisterResponse {
            client_id,
            client_name: client_name.to_owned(),
            redirect_uris: redirects,
            grant_types: ["authorization_code", "refresh_token"],
            response_types: ["code"],
            token_endpoint_auth_method: "none",
        }),
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

fn is_allowed_redirect_uri(value: &str) -> bool {
    let Ok(uri) = Url::parse(value) else {
        return false;
    };
    if uri.fragment().is_some() || uri.username() != "" || uri.password().is_some() {
        return false;
    }
    if uri.scheme() == "https" {
        return uri.host_str().is_some();
    }
    uri.scheme() == "http"
        && uri.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        })
}

#[derive(Deserialize)]
struct AuthorizeQuery {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    state: String,
    code_challenge: String,
    code_challenge_method: String,
    resource: String,
    scope: Option<String>,
}

#[derive(FromRow)]
struct RegisteredClient {
    client_id: String,
    client_name: String,
    redirect_uris: serde_json::Value,
}

async fn authorize(
    State(state): State<AppState>,
    Query(request): Query<AuthorizeQuery>,
) -> Result<Response, OAuthError> {
    if request.response_type != "code"
        || request.state.len() > 512
        || request.state.is_empty()
        || request.code_challenge_method != "S256"
        || request.code_challenge.len() != 43
        || !request
            .code_challenge
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        || !is_supported_mcp_resource(&state.mcp_public_url, &request.resource)
        || request.scope.as_deref().is_some_and(|scope| scope != "mcp")
    {
        return Err(OAuthError::InvalidRequest);
    }
    let client = query_as::<_, RegisteredClient>(
        "SELECT client_id, client_name, redirect_uris FROM mcp_oauth_clients WHERE client_id = $1",
    )
    .bind(&request.client_id)
    .fetch_optional(&state.control_pool)
    .await
    .map_err(|_| OAuthError::ServerError)?
    .ok_or(OAuthError::InvalidClient)?;
    let redirect_uris = serde_json::from_value::<Vec<String>>(client.redirect_uris)
        .map_err(|_| OAuthError::ServerError)?;
    if client.client_id != request.client_id
        || !redirect_uris.iter().any(|uri| uri == &request.redirect_uri)
    {
        return Err(OAuthError::InvalidRedirectUri);
    }

    let upstream_state = random_secret("state_");
    let upstream_code_verifier =
        derive_upstream_verifier(state.mcp_oauth_state_key.as_bytes(), &upstream_state)?;
    let upstream_challenge = code_challenge(&upstream_code_verifier);
    let upstream_state_hash = token_hash(&upstream_state);
    let resource = request.resource;
    query(
        "INSERT INTO mcp_oauth_requests \
         (id, upstream_state_hash, client_id, redirect_uri, client_state, client_code_challenge, resource, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, NOW() + INTERVAL '10 minutes')",
    )
    .bind(Uuid::now_v7())
    .bind(upstream_state_hash)
    .bind(&request.client_id)
    .bind(&request.redirect_uri)
    .bind(&request.state)
    .bind(&request.code_challenge)
    .bind(&resource)
    .execute(&state.control_pool)
    .await
    .map_err(|_| OAuthError::ServerError)?;

    let callback_uri = endpoint(&state.mcp_public_url, "oauth/callback");
    let oidc_url = state.oidc.authorization_url(
        &state.oidc_client_id,
        &callback_uri,
        &upstream_state,
        &upstream_challenge,
    );
    let cookie = format!(
        "mcp_oauth_state={upstream_state}; Path=/oauth; HttpOnly; SameSite=Lax; Max-Age={AUTHORIZATION_REQUEST_TTL_SECONDS}{}",
        cookie_secure_suffix(&state)
    );
    let mut response = Redirect::to(oidc_url.as_str()).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).map_err(|_| OAuthError::ServerError)?,
    );
    let _ = client.client_name;
    Ok(response)
}

#[derive(Deserialize)]
struct CallbackQuery {
    state: String,
    code: String,
}

#[derive(FromRow)]
struct OAuthRequestRow {
    id: Uuid,
    client_id: String,
    redirect_uri: String,
    client_state: String,
    client_code_challenge: String,
    resource: String,
    client_name: String,
}

async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query_values): Query<CallbackQuery>,
) -> Result<Response, OAuthError> {
    if cookie_value(&headers, "mcp_oauth_state").as_deref() != Some(query_values.state.as_str()) {
        return Err(OAuthError::InvalidRequest);
    }
    let upstream_state_hash = token_hash(&query_values.state);
    let request = query_as::<_, OAuthRequestRow>(
        "SELECT r.id, r.client_id, r.redirect_uri, r.client_state, r.client_code_challenge, r.resource, c.client_name \
         FROM mcp_oauth_requests r JOIN mcp_oauth_clients c USING (client_id) \
         WHERE r.upstream_state_hash = $1 AND r.expires_at > NOW() AND r.principal_id IS NULL",
    )
    .bind(upstream_state_hash)
    .fetch_optional(&state.control_pool)
    .await
    .map_err(|_| OAuthError::ServerError)?
    .ok_or(OAuthError::InvalidRequest)?;

    let verifier =
        derive_upstream_verifier(state.mcp_oauth_state_key.as_bytes(), &query_values.state)?;
    let token = state
        .oidc
        .exchange_authorization_code(
            &state.oidc_client_id,
            &endpoint(&state.mcp_public_url, "oauth/callback"),
            &query_values.code,
            &verifier,
        )
        .await
        .map_err(|_| OAuthError::UpstreamAuthenticationFailed)?;
    let principal = authenticate_oidc_access_token(&state, &token.access_token)
        .await
        .map_err(|_| OAuthError::UpstreamAuthenticationFailed)?;
    query("UPDATE mcp_oauth_requests SET principal_id = $2 WHERE id = $1 AND principal_id IS NULL")
        .bind(request.id)
        .bind(principal.0.id)
        .execute(&state.control_pool)
        .await
        .map_err(|_| OAuthError::ServerError)?;

    let mut consent_url = Url::parse(&endpoint(&state.mcp_public_url, "oauth/consent"))
        .map_err(|_| OAuthError::ServerError)?;
    consent_url
        .query_pairs_mut()
        .append_pair("state", &query_values.state);
    let mut response = Redirect::to(consent_url.as_str()).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    Ok(response)
}

async fn consent_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query_values): Query<StateQuery>,
) -> Result<Response, OAuthError> {
    if cookie_value(&headers, "mcp_oauth_state").as_deref() != Some(query_values.state.as_str()) {
        return Err(OAuthError::InvalidRequest);
    }
    let request = query_as::<_, OAuthRequestRow>(
        "SELECT r.id, r.client_id, r.redirect_uri, r.client_state, r.client_code_challenge, r.resource, c.client_name \
         FROM mcp_oauth_requests r JOIN mcp_oauth_clients c USING (client_id) \
         WHERE r.upstream_state_hash = $1 AND r.expires_at > NOW() AND r.principal_id IS NOT NULL",
    )
    .bind(token_hash(&query_values.state))
    .fetch_optional(&state.control_pool)
    .await
    .map_err(|_| OAuthError::ServerError)?
    .ok_or(OAuthError::InvalidRequest)?;
    let state_value = html_escape(&query_values.state);
    let client_name = html_escape(&request.client_name);
    let body = format!(
        "<!doctype html><html lang=\"pt-BR\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"referrer\" content=\"no-referrer\"><title>Autorizar cliente MCP</title><body><main><h1>Conectar {client_name}</h1><p>O cliente poderá usar o SQL MCP Gateway em nome da sua identidade. Bancos e operações continuam limitados às permissões concedidas a você.</p><form method=\"post\" action=\"/oauth/consent\"><input type=\"hidden\" name=\"state\" value=\"{state_value}\"><button name=\"decision\" value=\"approve\">Autorizar</button><button name=\"decision\" value=\"deny\">Cancelar</button></form></main></body></html>"
    );
    let mut response = ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    Ok(response)
}

#[derive(Deserialize)]
struct StateQuery {
    state: String,
}

#[derive(Deserialize)]
struct ConsentForm {
    state: String,
    decision: String,
}

async fn consent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<ConsentForm>,
) -> Result<Response, OAuthError> {
    if cookie_value(&headers, "mcp_oauth_state").as_deref() != Some(form.state.as_str()) {
        return Err(OAuthError::InvalidRequest);
    }
    let state_hash = token_hash(&form.state);
    let request = query_as::<_, OAuthRequestRow>(
        "SELECT r.id, r.client_id, r.redirect_uri, r.client_state, r.client_code_challenge, r.resource, c.client_name \
         FROM mcp_oauth_requests r JOIN mcp_oauth_clients c USING (client_id) \
         WHERE r.upstream_state_hash = $1 AND r.expires_at > NOW() AND r.principal_id IS NOT NULL",
    )
    .bind(state_hash.clone())
    .fetch_optional(&state.control_pool)
    .await
    .map_err(|_| OAuthError::ServerError)?
    .ok_or(OAuthError::InvalidRequest)?;
    if form.decision == "deny" {
        query("DELETE FROM mcp_oauth_requests WHERE id = $1")
            .bind(request.id)
            .execute(&state.control_pool)
            .await
            .map_err(|_| OAuthError::ServerError)?;
        let mut redirect =
            Url::parse(&request.redirect_uri).map_err(|_| OAuthError::ServerError)?;
        redirect
            .query_pairs_mut()
            .append_pair("error", "access_denied")
            .append_pair("state", &request.client_state);
        return consent_redirect(redirect, &state);
    }
    if form.decision != "approve" {
        return Err(OAuthError::InvalidRequest);
    }

    let mut transaction = state
        .control_pool
        .begin()
        .await
        .map_err(|_| OAuthError::ServerError)?;
    let claimed = query(
        "DELETE FROM mcp_oauth_requests WHERE id = $1 AND principal_id IS NOT NULL RETURNING principal_id",
    )
    .bind(request.id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| OAuthError::ServerError)?
    .ok_or(OAuthError::InvalidRequest)?;
    let principal_id: Uuid =
        sqlx::Row::try_get(&claimed, "principal_id").map_err(|_| OAuthError::ServerError)?;
    let code = random_secret("sgcode_");
    query(
        "INSERT INTO mcp_oauth_codes (code_hash, client_id, principal_id, redirect_uri, code_challenge, resource, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, NOW() + INTERVAL '120 seconds')",
    )
    .bind(token_hash(&code))
    .bind(&request.client_id)
    .bind(principal_id)
    .bind(&request.redirect_uri)
    .bind(&request.client_code_challenge)
    .bind(&request.resource)
    .execute(&mut *transaction)
    .await
    .map_err(|_| OAuthError::ServerError)?;
    transaction
        .commit()
        .await
        .map_err(|_| OAuthError::ServerError)?;

    let mut redirect = Url::parse(&request.redirect_uri).map_err(|_| OAuthError::ServerError)?;
    redirect
        .query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", &request.client_state);
    let response = consent_redirect(redirect, &state)?;
    let mut response = response;
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&expired_state_cookie(&state))
            .map_err(|_| OAuthError::ServerError)?,
    );
    Ok(response)
}

fn consent_redirect(redirect: Url, state: &AppState) -> Result<Response, OAuthError> {
    let mut response = Redirect::to(redirect.as_str()).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&expired_state_cookie(state)).map_err(|_| OAuthError::ServerError)?,
    );
    Ok(response)
}

#[derive(Deserialize)]
struct TokenRequest {
    grant_type: String,
    client_id: String,
    code: Option<String>,
    redirect_uri: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
    resource: Option<String>,
}

#[derive(Serialize)]
struct TokenResponse {
    access_token: String,
    token_type: &'static str,
    expires_in: i64,
    refresh_token: String,
    scope: &'static str,
}

async fn token(
    State(state): State<AppState>,
    Form(request): Form<TokenRequest>,
) -> Result<Response, OAuthError> {
    let mut transaction = state
        .control_pool
        .begin()
        .await
        .map_err(|_| OAuthError::ServerError)?;
    let (principal_id, client_id, resource, family_id) = match request.grant_type.as_str() {
        "authorization_code" => {
            let code = request.code.as_deref().ok_or(OAuthError::InvalidRequest)?;
            let redirect_uri = request
                .redirect_uri
                .as_deref()
                .ok_or(OAuthError::InvalidRequest)?;
            let verifier = request
                .code_verifier
                .as_deref()
                .ok_or(OAuthError::InvalidRequest)?;
            if verifier.len() < 43
                || verifier.len() > 128
                || !verifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-._~".contains(&byte))
            {
                return Err(OAuthError::InvalidGrant);
            }
            let code_hash = token_hash(code);
            let row = query(
                "UPDATE mcp_oauth_codes SET consumed_at = NOW() \
                 WHERE code_hash = $1 AND client_id = $2 AND redirect_uri = $3 \
                   AND expires_at > NOW() AND consumed_at IS NULL \
                 RETURNING principal_id, client_id, resource, code_challenge",
            )
            .bind(code_hash)
            .bind(&request.client_id)
            .bind(redirect_uri)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|_| OAuthError::ServerError)?
            .ok_or(OAuthError::InvalidGrant)?;
            let challenge: String =
                sqlx::Row::try_get(&row, "code_challenge").map_err(|_| OAuthError::ServerError)?;
            if code_challenge(verifier) != challenge {
                transaction
                    .commit()
                    .await
                    .map_err(|_| OAuthError::ServerError)?;
                return Err(OAuthError::InvalidGrant);
            }
            let resource: String =
                sqlx::Row::try_get(&row, "resource").map_err(|_| OAuthError::ServerError)?;
            if request
                .resource
                .as_deref()
                .is_some_and(|provided| provided != resource)
            {
                return Err(OAuthError::InvalidGrant);
            }
            let principal_id: Uuid =
                sqlx::Row::try_get(&row, "principal_id").map_err(|_| OAuthError::ServerError)?;
            let client_id: String =
                sqlx::Row::try_get(&row, "client_id").map_err(|_| OAuthError::ServerError)?;
            (principal_id, client_id, resource, Uuid::now_v7())
        }
        "refresh_token" => {
            let refresh = request
                .refresh_token
                .as_deref()
                .ok_or(OAuthError::InvalidRequest)?;
            let refresh_hash = token_hash(refresh);
            let row = query(
                "UPDATE mcp_oauth_tokens SET consumed_at = NOW() \
                 WHERE token_hash = $1 AND token_type = 'refresh' AND client_id = $2 \
                   AND expires_at > NOW() AND consumed_at IS NULL AND revoked_at IS NULL \
                 RETURNING principal_id, client_id, resource, token_family_id",
            )
            .bind(&refresh_hash)
            .bind(&request.client_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|_| OAuthError::ServerError)?;
            let Some(row) = row else {
                let replayed_family = query(
                    "SELECT token_family_id FROM mcp_oauth_tokens \
                     WHERE token_hash = $1 AND token_type = 'refresh' AND client_id = $2 AND consumed_at IS NOT NULL",
                )
                .bind(refresh_hash)
                .bind(&request.client_id)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(|_| OAuthError::ServerError)?;
                if let Some(replayed_family) = replayed_family {
                    let family_id: Uuid = sqlx::Row::try_get(&replayed_family, "token_family_id")
                        .map_err(|_| OAuthError::ServerError)?;
                    query(
                        "UPDATE mcp_oauth_tokens SET revoked_at = NOW() WHERE token_family_id = $1",
                    )
                    .bind(family_id)
                    .execute(&mut *transaction)
                    .await
                    .map_err(|_| OAuthError::ServerError)?;
                    transaction
                        .commit()
                        .await
                        .map_err(|_| OAuthError::ServerError)?;
                }
                return Err(OAuthError::InvalidGrant);
            };
            let resource: String =
                sqlx::Row::try_get(&row, "resource").map_err(|_| OAuthError::ServerError)?;
            if request
                .resource
                .as_deref()
                .is_some_and(|provided| provided != resource)
            {
                return Err(OAuthError::InvalidGrant);
            }
            (
                sqlx::Row::try_get(&row, "principal_id").map_err(|_| OAuthError::ServerError)?,
                sqlx::Row::try_get(&row, "client_id").map_err(|_| OAuthError::ServerError)?,
                resource,
                sqlx::Row::try_get(&row, "token_family_id").map_err(|_| OAuthError::ServerError)?,
            )
        }
        _ => return Err(OAuthError::UnsupportedGrantType),
    };
    if client_id != request.client_id {
        return Err(OAuthError::InvalidGrant);
    }
    let tokens = issue_tokens(
        &mut transaction,
        &client_id,
        principal_id,
        &resource,
        family_id,
    )
    .await?;
    transaction
        .commit()
        .await
        .map_err(|_| OAuthError::ServerError)?;
    let mut response = Json(tokens).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    Ok(response)
}

#[derive(Deserialize)]
struct RevokeRequest {
    token: String,
    client_id: String,
}

async fn revoke(
    State(state): State<AppState>,
    Form(request): Form<RevokeRequest>,
) -> Result<StatusCode, OAuthError> {
    let family = query(
        "SELECT token_family_id FROM mcp_oauth_tokens WHERE token_hash = $1 AND client_id = $2",
    )
    .bind(token_hash(&request.token))
    .bind(&request.client_id)
    .fetch_optional(&state.control_pool)
    .await
    .map_err(|_| OAuthError::ServerError)?;
    if let Some(family) = family {
        let family_id: Uuid =
            sqlx::Row::try_get(&family, "token_family_id").map_err(|_| OAuthError::ServerError)?;
        query("UPDATE mcp_oauth_tokens SET revoked_at = NOW() WHERE token_family_id = $1")
            .bind(family_id)
            .execute(&state.control_pool)
            .await
            .map_err(|_| OAuthError::ServerError)?;
    }
    Ok(StatusCode::OK)
}

async fn issue_tokens(
    transaction: &mut Transaction<'_, Postgres>,
    client_id: &str,
    principal_id: Uuid,
    resource: &str,
    family_id: Uuid,
) -> Result<TokenResponse, OAuthError> {
    let access_token = random_secret("sgmcp_");
    let refresh_token = random_secret("sgrf_");
    query(
        "INSERT INTO mcp_oauth_tokens (token_hash, token_type, token_family_id, client_id, principal_id, resource, expires_at) \
         VALUES ($1, 'access', $2, $3, $4, $5, NOW() + ($7 * INTERVAL '1 second')), \
                ($6, 'refresh', $2, $3, $4, $5, NOW() + ($8 * INTERVAL '1 second'))",
    )
    .bind(token_hash(&access_token))
    .bind(family_id)
    .bind(client_id)
    .bind(principal_id)
    .bind(resource)
    .bind(token_hash(&refresh_token))
    .bind(ACCESS_TOKEN_TTL_SECONDS)
    .bind(REFRESH_TOKEN_TTL_SECONDS)
    .execute(&mut **transaction)
    .await
    .map_err(|_| OAuthError::ServerError)?;
    Ok(TokenResponse {
        access_token,
        token_type: "Bearer",
        expires_in: ACCESS_TOKEN_TTL_SECONDS,
        refresh_token,
        scope: "mcp",
    })
}

pub async fn authenticate_access_token(
    pool: &PgPool,
    token: &str,
    resource: &str,
) -> Result<AuthenticatedPrincipal, ()> {
    let row = query_as::<_, crate::domain::Principal>(
        "SELECT p.id, p.issuer, p.subject, p.email, p.display_name, p.platform_role, p.enabled, p.created_at, p.last_seen_at \
         FROM mcp_oauth_tokens t JOIN principals p ON p.id = t.principal_id \
         WHERE t.token_hash = $1 AND t.token_type = 'access' AND t.expires_at > NOW() \
           AND t.revoked_at IS NULL AND p.enabled = TRUE AND t.resource = $2",
    )
    .bind(token_hash(token))
    .bind(resource)
    .fetch_optional(pool)
    .await
    .map_err(|_| ())?
    .ok_or(())?;
    query("UPDATE principals SET last_seen_at = NOW(), updated_at = NOW() WHERE id = $1")
        .bind(row.id)
        .execute(pool)
        .await
        .map_err(|_| ())?;
    Ok(AuthenticatedPrincipal(row))
}

fn derive_upstream_verifier(key: &[u8], state: &str) -> Result<String, OAuthError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).map_err(|_| OAuthError::ServerError)?;
    mac.update(state.as_bytes());
    Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

fn code_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn random_secret(prefix: &str) -> String {
    format!(
        "{prefix}{}{}",
        Uuid::now_v7().simple(),
        Uuid::now_v7().simple()
    )
}

fn cookie_secure_suffix(state: &AppState) -> &'static str {
    if state.mcp_public_url.scheme() == "https" {
        "; Secure"
    } else {
        ""
    }
}

fn expired_state_cookie(state: &AppState) -> String {
    format!(
        "mcp_oauth_state=; Path=/oauth; HttpOnly; SameSite=Lax; Max-Age=0{}",
        cookie_secure_suffix(state)
    )
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|item| item.trim().split_once('='))
        .find_map(|(key, value)| (key == name).then(|| value.to_owned()))
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[derive(Debug)]
enum OAuthError {
    InvalidRequest,
    InvalidClient,
    InvalidClientMetadata,
    InvalidRedirectUri,
    InvalidGrant,
    UnsupportedGrantType,
    UpstreamAuthenticationFailed,
    ServerError,
}

impl IntoResponse for OAuthError {
    fn into_response(self) -> Response {
        let (status, error, description) = match self {
            Self::InvalidRequest => (
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Invalid request",
            ),
            Self::InvalidClient => (StatusCode::UNAUTHORIZED, "invalid_client", "Invalid client"),
            Self::InvalidClientMetadata => (
                StatusCode::BAD_REQUEST,
                "invalid_client_metadata",
                "Invalid client metadata",
            ),
            Self::InvalidRedirectUri => (
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Invalid redirect URI",
            ),
            Self::InvalidGrant => (
                StatusCode::BAD_REQUEST,
                "invalid_grant",
                "Invalid or expired grant",
            ),
            Self::UnsupportedGrantType => (
                StatusCode::BAD_REQUEST,
                "unsupported_grant_type",
                "Unsupported grant type",
            ),
            Self::UpstreamAuthenticationFailed => (
                StatusCode::UNAUTHORIZED,
                "access_denied",
                "Authentication failed",
            ),
            Self::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "Request could not be completed",
            ),
        };
        let mut response = (
            status,
            Json(json!({"error": error, "error_description": description})),
        )
            .into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
            .headers_mut()
            .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
        response
    }
}

#[cfg(test)]
mod tests {
    use super::{
        code_challenge, derive_upstream_verifier, html_escape, is_allowed_redirect_uri,
        is_supported_mcp_resource, mcp_resource, protected_resource_metadata_path,
    };
    use url::Url;

    #[test]
    fn sql_mcp_metadata_exposes_only_the_sql_resource() {
        let public_url = Url::parse("https://gateway.example/").expect("valid gateway URL");
        let sql = mcp_resource(&public_url, "sql");
        assert_eq!(sql, "https://gateway.example/mcp");
        assert!(is_supported_mcp_resource(&public_url, &sql));
        assert!(!is_supported_mcp_resource(
            &public_url,
            "https://attacker.example/mcp"
        ));
        assert_eq!(
            protected_resource_metadata_path("sql"),
            "/.well-known/oauth-protected-resource/mcp"
        );
    }

    #[test]
    fn upstream_pkce_verifier_is_reproducible_and_not_stored_in_request_row() {
        let one = derive_upstream_verifier(b"a test key with at least 32 bytes", "state-value")
            .expect("HMAC key is valid");
        let two = derive_upstream_verifier(b"a test key with at least 32 bytes", "state-value")
            .expect("same input gives same verifier");
        assert_eq!(one, two);
        assert_eq!(one.len(), 43);
        assert_eq!(code_challenge(&one).len(), 43);
        assert_ne!(one, "state-value");
    }

    #[test]
    fn redirect_uri_validation_only_allows_https_or_loopback_http() {
        assert!(is_allowed_redirect_uri("https://client.example/callback"));
        assert!(is_allowed_redirect_uri("http://127.0.0.1:3210/callback"));
        assert!(is_allowed_redirect_uri("http://[::1]:3210/callback"));
        assert!(!is_allowed_redirect_uri("http://client.example/callback"));
        assert!(!is_allowed_redirect_uri(
            "https://client.example/callback#fragment"
        ));
        assert!(!is_allowed_redirect_uri(
            "https://user@client.example/callback"
        ));
    }

    #[test]
    fn consent_page_escapes_untrusted_client_names() {
        assert_eq!(html_escape("<script&\"'"), "&lt;script&amp;&quot;&#39;");
    }
}
