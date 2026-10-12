use axum::{
    extract::State,
    http::{Request, header::AUTHORIZATION},
    middleware::Next,
    response::Response,
};
use sqlx::query_as;
use uuid::Uuid;

use crate::{
    api_error::ApiError,
    domain::{PlatformRole, Principal},
    state::AppState,
};

#[derive(Clone)]
pub struct AuthenticatedPrincipal(pub Principal);

pub async fn require_identity(
    State(state): State<AppState>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let authorization = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;
    let Some(access_token) = authorization.strip_prefix("Bearer ") else {
        return Err(ApiError::Unauthorized);
    };
    if access_token.trim().is_empty() || access_token.contains(char::is_whitespace) {
        return Err(ApiError::Unauthorized);
    }

    let principal = authenticate_access_token(&state, access_token).await?;
    request.extensions_mut().insert(principal);
    Ok(next.run(request).await)
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
