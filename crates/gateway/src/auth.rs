use axum::{
    extract::State,
    http::{HeaderValue, Request, StatusCode, header::AUTHORIZATION},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sqlx::query_as;
use uuid::Uuid;

use crate::{
    api_error::ApiError,
    domain::{PlatformRole, Principal},
    oauth,
    state::AppState,
};

#[derive(Clone)]
pub struct AuthenticatedPrincipal(pub Principal);

pub async fn require_identity(
    State(state): State<AppState>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let is_mcp_request = request.uri().path() == "/mcp";
    let authorization = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Unauthorized);
    let authorization = match authorization {
        Ok(authorization) => authorization,
        Err(error) if is_mcp_request => {
            return Ok(mcp_unauthorized_response(&state, error));
        }
        Err(error) => return Err(error),
    };
    let Some(access_token) = authorization.strip_prefix("Bearer ") else {
        if is_mcp_request {
            return Ok(mcp_unauthorized_response(&state, ApiError::Unauthorized));
        }
        return Err(ApiError::Unauthorized);
    };
    if access_token.trim().is_empty() || access_token.contains(char::is_whitespace) {
        if is_mcp_request {
            return Ok(mcp_unauthorized_response(&state, ApiError::Unauthorized));
        }
        return Err(ApiError::Unauthorized);
    }

    let principal_result = if access_token.starts_with("sgmcp_") {
        oauth::authenticate_access_token(&state.control_pool, access_token, &state.mcp_public_url)
            .await
            .map_err(|_| ApiError::Unauthorized)
    } else {
        authenticate_access_token(&state, access_token).await
    };
    let principal = match principal_result {
        Ok(principal) => principal,
        Err(error) if is_mcp_request && matches!(error, ApiError::Unauthorized) => {
            return Ok(mcp_unauthorized_response(&state, error));
        }
        Err(error) => return Err(error),
    };
    request.extensions_mut().insert(principal);
    Ok(next.run(request).await)
}

fn mcp_unauthorized_response(state: &AppState, error: ApiError) -> Response {
    let mut response = if error.status() == StatusCode::UNAUTHORIZED {
        error.into_response()
    } else {
        (StatusCode::UNAUTHORIZED, "Authentication required").into_response()
    };
    let metadata_url = state
        .mcp_public_url
        .join(".well-known/oauth-protected-resource/mcp")
        .map(|url| url.to_string())
        .unwrap_or_else(|_| ".well-known/oauth-protected-resource".to_owned());
    let challenge = format!("Bearer resource_metadata=\"{metadata_url}\", scope=\"mcp\"");
    if let Ok(value) = HeaderValue::from_str(&challenge) {
        response.headers_mut().insert("www-authenticate", value);
    }
    response
}

pub async fn authenticate_access_token(
    state: &AppState,
    access_token: &str,
) -> Result<AuthenticatedPrincipal, ApiError> {
    let identity = state.oidc.authenticate(access_token).await?;
    let initial_role = if state.bootstrap_admin_subjects.contains(&identity.subject) {
        PlatformRole::Admin
    } else {
        PlatformRole::Member
    };
    let role_text = match initial_role {
        PlatformRole::Admin => "admin",
        PlatformRole::Member => "member",
    };

    let principal = query_as::<_, Principal>(
        "INSERT INTO principals (id, issuer, subject, email, display_name, platform_role, last_seen_at) \
         VALUES ($1, $2, $3, $4, $5, $6::platform_role, NOW()) \
         ON CONFLICT (issuer, subject) DO UPDATE SET \
             email = EXCLUDED.email, \
             display_name = EXCLUDED.display_name, \
             platform_role = CASE \
                 WHEN principals.platform_role = 'admin' OR EXCLUDED.platform_role = 'admin' \
                 THEN 'admin'::platform_role ELSE principals.platform_role END, \
             last_seen_at = NOW(), \
             updated_at = NOW() \
         RETURNING id, issuer, subject, email, display_name, platform_role, enabled, created_at, last_seen_at",
    )
    .bind(Uuid::now_v7())
    .bind(&identity.issuer)
    .bind(&identity.subject)
    .bind(identity.email)
    .bind(identity.display_name)
    .bind(role_text)
    .fetch_one(&state.control_pool)
    .await
    .map_err(|_| ApiError::DependencyUnavailable)?;

    if !principal.enabled {
        return Err(ApiError::Forbidden);
    }

    Ok(AuthenticatedPrincipal(principal))
}

pub fn ensure_admin(principal: &Principal) -> Result<(), ApiError> {
    match principal.platform_role {
        PlatformRole::Admin if principal.enabled => Ok(()),
        _ => Err(ApiError::Forbidden),
    }
}
