use std::time::Instant;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, query, query_as};
use uuid::Uuid;
use vault_broker::{VaultDatabaseConfig, VaultDatabaseEngine, VaultRoleConfig};
use zeroize::Zeroizing;

use crate::{
    api_error::ApiError,
    audit::{AuditEvent, record_event},
    auth::{AuthenticatedPrincipal, ensure_admin},
    domain::{AccessLevel, DatabaseEngine, DatabaseGrant, DatabaseTarget, PlatformRole, Principal},
    state::AppState,
};

#[derive(Serialize)]
pub struct AuthConfigResponse {
    pub issuer: String,
    pub client_id: String,
    pub resource: String,
    pub scopes: [&'static str; 3],
}

pub async fn auth_config(State(state): State<AppState>) -> Json<AuthConfigResponse> {
    Json(AuthConfigResponse {
        issuer: state.oidc.issuer().to_owned(),
        client_id: state.oidc_client_id.to_string(),
        resource: state.oidc_access_token_audience.to_string(),
        scopes: ["openid", "profile", "email"],
    })
}

#[derive(Serialize)]
pub struct MeResponse {
    pub principal: Principal,
    pub database_grants: Vec<DatabaseGrant>,
}

pub async fn me(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<AuthenticatedPrincipal>,
) -> Result<Json<MeResponse>, ApiError> {
    let database_grants = query_as::<_, GrantRow>(
        "SELECT g.database_id, d.name AS database_name, g.access_level \
         FROM principal_database_grants g \
         JOIN database_targets d ON d.id = g.database_id \
         WHERE g.principal_id = $1 AND d.enabled = TRUE \
         ORDER BY d.name",
    )
    .bind(actor.0.id)
    .fetch_all(&state.control_pool)
    .await
    .map_err(|_| ApiError::DependencyUnavailable)?
    .into_iter()
    .map(|row| DatabaseGrant {
        database_id: row.database_id,
        database_name: row.database_name,
        access_level: row.access_level,
    })
    .collect();

    Ok(Json(MeResponse {
        principal: actor.0,
        database_grants,
    }))
}

#[derive(FromRow)]
struct GrantRow {
    database_id: Uuid,
    database_name: String,
    access_level: AccessLevel,
}

#[derive(Serialize)]
pub struct PrincipalSummary {
    pub id: Uuid,
    pub issuer: String,
    pub subject: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub platform_role: PlatformRole,
    pub enabled: bool,
    pub last_seen_at: Option<DateTime<Utc>>,
}

pub async fn list_principals(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<AuthenticatedPrincipal>,
) -> Result<Json<Vec<PrincipalSummary>>, ApiError> {
    ensure_admin(&actor.0)?;
    let principals = query_as::<_, Principal>(
        "SELECT id, issuer, subject, email, display_name, platform_role, enabled, created_at, last_seen_at \
         FROM principals ORDER BY last_seen_at DESC NULLS LAST, created_at DESC LIMIT 500",
    )
    .fetch_all(&state.control_pool)
    .await
    .map_err(|_| ApiError::DependencyUnavailable)?
    .into_iter()
    .map(|principal| PrincipalSummary {
        id: principal.id,
        issuer: principal.issuer,
        subject: principal.subject,
        email: principal.email,
        display_name: principal.display_name,
        platform_role: principal.platform_role,
        enabled: principal.enabled,
        last_seen_at: principal.last_seen_at,
    })
    .collect();

    Ok(Json(principals))
}

#[derive(Serialize)]
pub struct VisibleDatabase {
    #[serde(flatten)]
    pub target: DatabaseTarget,
    pub access_level: Option<AccessLevel>,
}

pub async fn list_databases(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<AuthenticatedPrincipal>,
) -> Result<Json<Vec<VisibleDatabase>>, ApiError> {
    let rows = if actor.0.platform_role == PlatformRole::Admin {
        query_as::<_, TargetGrantRow>(
            "SELECT d.id, d.name, d.engine, d.host, d.port, d.database_name, d.schema_name, \
                    d.enabled, d.created_at, NULL::access_level AS access_level \
             FROM database_targets d WHERE d.enabled = TRUE ORDER BY d.name",
        )
        .fetch_all(&state.control_pool)
        .await
    } else {
        query_as::<_, TargetGrantRow>(
            "SELECT d.id, d.name, d.engine, d.host, d.port, d.database_name, d.schema_name, \
                    d.enabled, d.created_at, g.access_level \
             FROM database_targets d \
             JOIN principal_database_grants g ON g.database_id = d.id \
             WHERE d.enabled = TRUE AND g.principal_id = $1 ORDER BY d.name",
        )
        .bind(actor.0.id)
        .fetch_all(&state.control_pool)
        .await
    }
    .map_err(|_| ApiError::DependencyUnavailable)?;

    Ok(Json(
        rows.into_iter()
            .map(|row| {
                let access_level = row.access_level;
                let target = row.target();
                VisibleDatabase {
                    target,
                    access_level,
                }
            })
            .collect(),
    ))
}

pub async fn list_targets_admin(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<AuthenticatedPrincipal>,
) -> Result<Json<Vec<DatabaseTarget>>, ApiError> {
    ensure_admin(&actor.0)?;
    let targets = query_as::<_, DatabaseTarget>(
        "SELECT id, name, engine, host, port, database_name, schema_name, enabled, created_at \
         FROM database_targets ORDER BY name",
    )
    .fetch_all(&state.control_pool)
    .await
    .map_err(|_| ApiError::DependencyUnavailable)?;

    Ok(Json(targets))
}

#[derive(FromRow)]
struct TargetGrantRow {
    id: Uuid,
    name: String,
    engine: DatabaseEngine,
    host: String,
    port: i32,
    database_name: String,
    schema_name: String,
    enabled: bool,
    created_at: DateTime<Utc>,
    access_level: Option<AccessLevel>,
}

impl TargetGrantRow {
    fn target(self) -> DatabaseTarget {
        DatabaseTarget {
            id: self.id,
            name: self.name,
            engine: self.engine,
            host: self.host,
            port: self.port,
            database_name: self.database_name,
            schema_name: self.schema_name,
            enabled: self.enabled,
            created_at: self.created_at,
        }
    }
}

#[derive(Deserialize)]
pub struct RegisterTargetRequest {
    pub name: String,
    pub engine: DatabaseEngine,
    pub host: String,
    pub port: u16,
    pub database_name: String,
    #[serde(default)]
    pub schema_name: Option<String>,
    pub root_username: String,
    pub root_password: SensitiveInput,
    #[serde(default)]
    pub tls_ca_pem: Option<SensitiveInput>,
}

#[derive(Deserialize)]
#[serde(try_from = "String")]
pub struct SensitiveInput(Zeroizing<String>);

impl TryFrom<String> for SensitiveInput {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() || value.len() > 16_384 {
            return Err("sensitive input is outside the accepted size");
        }
        Ok(Self(Zeroizing::new(value)))
    }
}

impl SensitiveInput {
    fn expose(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Serialize)]
pub struct CreatedTargetResponse {
    pub target: DatabaseTarget,
    pub credential_ttl_seconds: u64,
}

pub async fn register_target(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<AuthenticatedPrincipal>,
    Json(input): Json<RegisterTargetRequest>,
) -> Result<(StatusCode, Json<CreatedTargetResponse>), ApiError> {
    ensure_admin(&actor.0)?;
    validate_target_input(&input)?;

    let exists = query("SELECT 1 FROM database_targets WHERE name = $1")
        .bind(input.name.trim())
        .fetch_optional(&state.control_pool)
        .await
        .map_err(|_| ApiError::DependencyUnavailable)?;
    if exists.is_some() {
        return Err(ApiError::Conflict);
    }

    let id = Uuid::now_v7();
    let vault_suffix = id.simple().to_string();
    let connection_name = format!("mcp-db-{vault_suffix}");
    let read_role = format!("mcp-read-{vault_suffix}");
    let write_role = format!("mcp-write-{vault_suffix}");
    let schema_name = input
        .schema_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(match input.engine {
            DatabaseEngine::Postgresql => "public",
            DatabaseEngine::Mysql => input.database_name.as_str(),
        });
    let engine = match input.engine {
        DatabaseEngine::Postgresql => VaultDatabaseEngine::Postgresql,
        DatabaseEngine::Mysql => VaultDatabaseEngine::Mysql,
    };
    let start = Instant::now();

    let vault_setup = async {
        state
            .vault_admin
            .configure_database(VaultDatabaseConfig {
                name: &connection_name,
                engine,
                host: input.host.trim(),
                port: input.port,
                database_name: input.database_name.trim(),
                username: input.root_username.trim(),
                password: input.root_password.expose(),
                tls_ca_pem: input.tls_ca_pem.as_ref().map(SensitiveInput::expose),
                read_role: &read_role,
                write_role: &write_role,
            })
            .await?;

        let (read_creation, read_revocation) =
            role_statements(input.engine, input.database_name.trim(), schema_name, false);
        state
            .vault_admin
            .configure_role(VaultRoleConfig {
                role_name: &read_role,
                connection_name: &connection_name,
                creation_statements: read_creation,
                revocation_statements: read_revocation,
            })
            .await?;

        let (write_creation, write_revocation) =
            role_statements(input.engine, input.database_name.trim(), schema_name, true);
        state
            .vault_admin
            .configure_role(VaultRoleConfig {
                role_name: &write_role,
                connection_name: &connection_name,
                creation_statements: write_creation,
                revocation_statements: write_revocation,
            })
            .await
    }
    .await;

    if vault_setup.is_err() {
        cleanup_vault_target(&state, &connection_name, &read_role, &write_role).await;
        tracing::warn!(database_id = %id, "Vault database registration failed");
        record_event(
            &state.control_pool,
            &actor.0,
            AuditEvent::new("database.register", "failure")
                .error("VAULT_CONFIGURATION_FAILED")
                .duration(start.elapsed().as_millis() as i64),
        )
        .await;
        return Err(ApiError::DependencyUnavailable);
    }

    let target = query_as::<_, DatabaseTarget>(
        "INSERT INTO database_targets \
         (id, name, engine, host, port, database_name, schema_name, tls_mode, tls_ca_pem, \
          vault_connection_name, vault_read_role, vault_write_role, enabled, created_by) \
         VALUES ($1, $2, $3::database_engine, $4, $5, $6, $7, 'verify_full', $8, $9, $10, $11, TRUE, $12) \
         RETURNING id, name, engine, host, port, database_name, schema_name, enabled, created_at",
    )
    .bind(id)
    .bind(input.name.trim())
    .bind(match input.engine {
        DatabaseEngine::Postgresql => "postgresql",
        DatabaseEngine::Mysql => "mysql",
    })
    .bind(input.host.trim())
    .bind(i32::from(input.port))
    .bind(input.database_name.trim())
    .bind(schema_name)
    .bind(input.tls_ca_pem.as_ref().map(SensitiveInput::expose))
    .bind(&connection_name)
    .bind(&read_role)
    .bind(&write_role)
    .bind(actor.0.id)
    .fetch_one(&state.control_pool)
    .await;

    let target = match target {
        Ok(target) => target,
        Err(_) => {
            cleanup_vault_target(&state, &connection_name, &read_role, &write_role).await;
            tracing::error!(database_id = %id, "Vault target configured but metadata insert failed");
            record_event(
                &state.control_pool,
                &actor.0,
                AuditEvent::new("database.register", "failure")
                    .error("CONTROL_DATABASE_WRITE_FAILED")
                    .duration(start.elapsed().as_millis() as i64),
            )
            .await;
            return Err(ApiError::DependencyUnavailable);
        }
    };

    record_event(
        &state.control_pool,
        &actor.0,
        AuditEvent::new("database.register", "success")
            .database(target.id)
            .duration(start.elapsed().as_millis() as i64),
    )
    .await;

    Ok((
        StatusCode::CREATED,
        Json(CreatedTargetResponse {
            target,
            credential_ttl_seconds: 900,
        }),
    ))
}

async fn cleanup_vault_target(
    state: &AppState,
    connection_name: &str,
    read_role: &str,
    write_role: &str,
) {
    let read_result = state.vault_admin.delete_database_role(read_role).await;
    let write_result = state.vault_admin.delete_database_role(write_role).await;
    let connection_result = state
        .vault_admin
        .delete_database_config(connection_name)
        .await;
    if read_result.is_err() || write_result.is_err() || connection_result.is_err() {
        tracing::error!("failed to clean up a partial Vault database registration");
    }
}

fn validate_target_input(input: &RegisterTargetRequest) -> Result<(), ApiError> {
    if input.name.trim().is_empty()
        || input.name.trim().len() > 80
        || input.root_username.trim().is_empty()
        || input.root_username.trim().len() > 128
        || input.host.trim().is_empty()
        || input.host.len() > 253
        || !input
            .host
            .trim()
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
        || !valid_identifier(&input.database_name)
    {
        return Err(ApiError::InvalidRequest);
    }

    if let Some(schema_name) = input.schema_name.as_deref()
        && !schema_name.trim().is_empty()
        && !valid_identifier(schema_name)
    {
        return Err(ApiError::InvalidRequest);
    }

    if let Some(ca_pem) = input.tls_ca_pem.as_ref()
        && !ca_pem.expose().contains("-----BEGIN CERTIFICATE-----")
    {
        return Err(ApiError::InvalidRequest);
    }

    Ok(())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn role_statements(
    engine: DatabaseEngine,
    database_name: &str,
    schema_name: &str,
    write: bool,
) -> (Vec<String>, Vec<String>) {
    match engine {
        DatabaseEngine::Postgresql => {
            let mut statements = vec![
                "CREATE ROLE \"{{name}}\" WITH LOGIN PASSWORD '{{password}}' VALID UNTIL '{{expiration}}'".to_owned(),
                format!("GRANT CONNECT ON DATABASE \"{database_name}\" TO \"{{{{name}}}}\""),
                format!("GRANT USAGE ON SCHEMA \"{schema_name}\" TO \"{{{{name}}}}\""),
                format!("GRANT SELECT ON ALL TABLES IN SCHEMA \"{schema_name}\" TO \"{{{{name}}}}\""),
            ];
            if write {
                statements.push(format!(
                    "GRANT INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA \"{schema_name}\" TO \"{{{{name}}}}\""
                ));
                statements.push(format!(
                    "GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA \"{schema_name}\" TO \"{{{{name}}}}\""
                ));
            }

            let mut revocation_statements = vec![
                format!(
                    "REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA \"{schema_name}\" FROM \"{{{{name}}}}\""
                ),
                format!("REVOKE USAGE ON SCHEMA \"{schema_name}\" FROM \"{{{{name}}}}\""),
                format!("REVOKE CONNECT ON DATABASE \"{database_name}\" FROM \"{{{{name}}}}\""),
            ];
            if write {
                revocation_statements.push(format!(
                    "REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA \"{schema_name}\" FROM \"{{{{name}}}}\""
                ));
            }
            revocation_statements.push("DROP ROLE IF EXISTS \"{{name}}\"".to_owned());

            (statements, revocation_statements)
        }
        DatabaseEngine::Mysql => {
            let privileges = if write {
                "SELECT, INSERT, UPDATE, DELETE"
            } else {
                "SELECT"
            };
            (
                vec![
                    "CREATE USER '{{name}}'@'%' IDENTIFIED BY '{{password}}'".to_owned(),
                    format!("GRANT {privileges} ON `{database_name}`.* TO '{{{{name}}}}'@'%'"),
                ],
                vec!["DROP USER IF EXISTS '{{name}}'@'%'".to_owned()],
            )
        }
    }
}

#[derive(Deserialize)]
pub struct SetGrantRequest {
    pub access_level: AccessLevel,
}

pub async fn set_grant(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<AuthenticatedPrincipal>,
    Path((principal_id, database_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<SetGrantRequest>,
) -> Result<StatusCode, ApiError> {
    ensure_admin(&actor.0)?;
    let enabled_target = query("SELECT 1 FROM database_targets WHERE id = $1 AND enabled = TRUE")
        .bind(database_id)
        .fetch_optional(&state.control_pool)
        .await
        .map_err(|_| ApiError::DependencyUnavailable)?;
    let enabled_principal = query("SELECT 1 FROM principals WHERE id = $1 AND enabled = TRUE")
        .bind(principal_id)
        .fetch_optional(&state.control_pool)
        .await
        .map_err(|_| ApiError::DependencyUnavailable)?;
    if enabled_target.is_none() || enabled_principal.is_none() {
        return Err(ApiError::NotFound);
    }

    query(
        "INSERT INTO principal_database_grants (principal_id, database_id, access_level, granted_by) \
         VALUES ($1, $2, $3::access_level, $4) \
         ON CONFLICT (principal_id, database_id) DO UPDATE SET \
             access_level = EXCLUDED.access_level, granted_by = EXCLUDED.granted_by, granted_at = NOW()",
    )
    .bind(principal_id)
    .bind(database_id)
    .bind(match input.access_level {
        AccessLevel::Read => "read",
        AccessLevel::Write => "write",
    })
    .bind(actor.0.id)
    .execute(&state.control_pool)
    .await
    .map_err(|_| ApiError::DependencyUnavailable)?;

    record_event(
        &state.control_pool,
        &actor.0,
        AuditEvent::new("grant.set", "success")
            .database(database_id)
            .access(input.access_level),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_grant(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<AuthenticatedPrincipal>,
    Path((principal_id, database_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    ensure_admin(&actor.0)?;
    let deleted =
        query("DELETE FROM principal_database_grants WHERE principal_id = $1 AND database_id = $2")
            .bind(principal_id)
            .bind(database_id)
            .execute(&state.control_pool)
            .await
            .map_err(|_| ApiError::DependencyUnavailable)?;

    record_event(
        &state.control_pool,
        &actor.0,
        AuditEvent::new("grant.delete", "success").database(database_id),
    )
    .await;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct AdminGrantView {
    pub database_id: Uuid,
    pub database_name: String,
    pub access_level: AccessLevel,
    pub granted_at: DateTime<Utc>,
}

pub async fn list_principal_grants(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<AuthenticatedPrincipal>,
    Path(principal_id): Path<Uuid>,
) -> Result<Json<Vec<AdminGrantView>>, ApiError> {
    ensure_admin(&actor.0)?;
    let grants = query_as::<_, AdminGrantViewRow>(
        "SELECT g.database_id, d.name AS database_name, g.access_level, g.granted_at \
         FROM principal_database_grants g JOIN database_targets d ON d.id = g.database_id \
         WHERE g.principal_id = $1 ORDER BY d.name",
    )
    .bind(principal_id)
    .fetch_all(&state.control_pool)
    .await
    .map_err(|_| ApiError::DependencyUnavailable)?
    .into_iter()
    .map(|row| AdminGrantView {
        database_id: row.database_id,
        database_name: row.database_name,
        access_level: row.access_level,
        granted_at: row.granted_at,
    })
    .collect();

    Ok(Json(grants))
}

#[derive(FromRow)]
struct AdminGrantViewRow {
    database_id: Uuid,
    database_name: String,
    access_level: AccessLevel,
    granted_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct SetTargetEnabledRequest {
    pub enabled: bool,
}

pub async fn set_target_enabled(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<AuthenticatedPrincipal>,
    Path(database_id): Path<Uuid>,
    Json(input): Json<SetTargetEnabledRequest>,
) -> Result<StatusCode, ApiError> {
    ensure_admin(&actor.0)?;
    let changed =
        query("UPDATE database_targets SET enabled = $1, updated_at = NOW() WHERE id = $2")
            .bind(input.enabled)
            .bind(database_id)
            .execute(&state.control_pool)
            .await
            .map_err(|_| ApiError::DependencyUnavailable)?;
    if changed.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }

    record_event(
        &state.control_pool,
        &actor.0,
        AuditEvent::new("database.enabled.set", "success").database(database_id),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct AuditQuery {
    pub limit: Option<i64>,
}

#[derive(Serialize, FromRow)]
pub struct AuditEventView {
    pub id: Uuid,
    pub principal_id: Option<Uuid>,
    pub issuer: String,
    pub subject: String,
    pub database_id: Option<Uuid>,
    pub operation: String,
    pub access_level: Option<AccessLevel>,
    pub outcome: String,
    pub error_code: Option<String>,
    pub query_fingerprint: Option<String>,
    pub database_principal: Option<String>,
    pub vault_lease_fingerprint: Option<String>,
    pub duration_ms: Option<i64>,
    pub created_at: DateTime<Utc>,
}

pub async fn list_audit_events(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<AuthenticatedPrincipal>,
    Query(input): Query<AuditQuery>,
) -> Result<Json<Vec<AuditEventView>>, ApiError> {
    ensure_admin(&actor.0)?;
    let limit = input.limit.unwrap_or(100).clamp(1, 200);
    let events = query_as::<_, AuditEventView>(
        "SELECT id, principal_id, issuer, subject, database_id, operation, access_level, \
                outcome, error_code, query_fingerprint, database_principal, \
                vault_lease_fingerprint, duration_ms, created_at \
         FROM audit_events ORDER BY created_at DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&state.control_pool)
    .await
    .map_err(|_| ApiError::DependencyUnavailable)?;

    Ok(Json(events))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde::Deserialize;
    use sqlx::{
        Row,
        mysql::{MySqlConnectOptions, MySqlPool, MySqlPoolOptions, MySqlSslMode},
        postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgSslMode},
    };
    use url::Url;
    use uuid::Uuid;
    use vault_broker::{VaultClient, VaultDatabaseConfig, VaultDatabaseEngine, VaultRoleConfig};
    use zeroize::Zeroizing;

    use crate::domain::DatabaseEngine;

    use super::{role_statements, valid_identifier};

    type IntegrationResult<T = ()> = Result<T, &'static str>;

    #[derive(Clone, Copy)]
    struct IntegrationDatabase<'a> {
        engine: DatabaseEngine,
        vault_host: &'a str,
        local_port: u16,
        root_username: &'a str,
        root_password: &'a str,
    }

    #[test]
    fn postgres_leases_grant_only_existing_objects_and_remove_every_grant_on_revoke() {
        let (read_creation, read_revocation) =
            role_statements(DatabaseEngine::Postgresql, "warehouse", "reporting", false);
        let (write_creation, write_revocation) =
            role_statements(DatabaseEngine::Postgresql, "warehouse", "reporting", true);

        assert!(
            read_creation
                .iter()
                .any(|statement| statement.contains("GRANT SELECT ON ALL TABLES"))
        );
        assert!(
            !read_creation
                .iter()
                .any(|statement| statement.contains("DEFAULT PRIVILEGES"))
        );
        assert!(
            !read_creation
                .iter()
                .any(|statement| statement.contains("INSERT"))
        );
        assert!(
            read_revocation
                .iter()
                .any(|statement| statement.contains("REVOKE ALL PRIVILEGES ON ALL TABLES"))
        );
        assert!(
            read_revocation
                .iter()
                .any(|statement| statement.contains("DROP ROLE"))
        );

        assert!(
            write_creation
                .iter()
                .any(|statement| statement.contains("INSERT, UPDATE, DELETE"))
        );
        assert!(
            write_creation
                .iter()
                .any(|statement| statement.contains("ON ALL SEQUENCES"))
        );
        assert!(
            write_revocation
                .iter()
                .any(|statement| statement.contains("ON ALL SEQUENCES"))
        );
        assert!(
            !write_creation
                .iter()
                .any(|statement| statement.contains("DEFAULT PRIVILEGES"))
        );
        assert!(
            !write_revocation
                .iter()
                .any(|statement| statement.contains("DEFAULT PRIVILEGES"))
        );
    }

    #[test]
    fn mysql_write_lease_is_dml_only_and_target_identifiers_are_restricted() {
        let (read_creation, _) =
            role_statements(DatabaseEngine::Mysql, "warehouse", "warehouse", false);
        let (write_creation, _) =
            role_statements(DatabaseEngine::Mysql, "warehouse", "warehouse", true);

        assert!(
            read_creation
                .iter()
                .any(|statement| statement.contains("GRANT SELECT"))
        );
        assert!(
            !read_creation
                .iter()
                .any(|statement| statement.contains("INSERT"))
        );
        assert!(
            write_creation
                .iter()
                .any(|statement| statement.contains("SELECT, INSERT, UPDATE, DELETE"))
        );
        assert!(valid_identifier("analytics_2026"));
        assert!(!valid_identifier("analytics`; DROP DATABASE x;--"));
    }

    #[tokio::test]
    #[ignore = "requires the isolated Vault/PostgreSQL/MySQL stack from make test-integration"]
    async fn live_vault_postgres_mysql_dynamic_leases_enforce_database_permissions() {
        if let Err(error) = run_live_database_integration().await {
            panic!("live database integration failed: {error}");
        }
    }

    async fn run_live_database_integration() -> IntegrationResult {
        let environment_path =
            std::env::var_os("SQL_MCP_GATEWAY_TEST_ENV").ok_or("missing test environment path")?;
        dotenvy::from_path_override(environment_path)
            .map_err(|_| "could not load test environment")?;

        let vault_address = Url::parse(&integration_value("SQL_MCP_GATEWAY_TEST_VAULT_ADDRESS")?)
            .map_err(|_| "invalid local Vault address")?;
        let admin_token =
            Zeroizing::new(integration_value("SQL_MCP_GATEWAY_TEST_VAULT_ADMIN_TOKEN")?);
        let postgres_password =
            Zeroizing::new(integration_value("SQL_MCP_GATEWAY_TEST_POSTGRES_PASSWORD")?);
        let mysql_password =
            Zeroizing::new(integration_value("SQL_MCP_GATEWAY_TEST_MYSQL_PASSWORD")?);
        let postgres_port = integration_port("SQL_MCP_GATEWAY_TEST_POSTGRES_PORT")?;
        let mysql_port = integration_port("SQL_MCP_GATEWAY_TEST_MYSQL_PORT")?;
        let tls_ca = std::fs::read_to_string(integration_value("SQL_MCP_GATEWAY_TEST_TLS_CA")?)
            .map_err(|_| "could not load the local test CA")?;

        validate_control_plane_migrations(
            postgres_port,
            "postgres",
            postgres_password.as_str(),
            &tls_ca,
        )
        .await?;
        ensure_vault_database_engine(&vault_address, admin_token.as_str()).await?;
        let runtime_token =
            create_runtime_vault_token(&vault_address, admin_token.as_str()).await?;
        let admin = VaultClient::new(vault_address.clone(), admin_token.clone())
            .map_err(|_| "could not construct Vault admin client")?;
        let runtime = VaultClient::new(vault_address.clone(), runtime_token.clone())
            .map_err(|_| "could not construct Vault runtime client")?;

        let denied_configuration = reqwest::Client::new()
            .post(vault_api_url(
                &vault_address,
                &["database", "config", "not-authorized"],
            ))
            .header("X-Vault-Token", runtime_token.as_str())
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|_| "could not check the Vault runtime policy")?;
        if denied_configuration.status() != reqwest::StatusCode::FORBIDDEN {
            return Err("Vault runtime token was not restricted to lease operations");
        }

        let suffix = Uuid::now_v7().simple().to_string();
        exercise_database_engine(
            &admin,
            &runtime,
            &tls_ca,
            IntegrationDatabase {
                engine: DatabaseEngine::Postgresql,
                vault_host: "postgres",
                local_port: postgres_port,
                root_username: "postgres",
                root_password: postgres_password.as_str(),
            },
            &suffix,
        )
        .await?;
        exercise_database_engine(
            &admin,
            &runtime,
            &tls_ca,
            IntegrationDatabase {
                engine: DatabaseEngine::Mysql,
                vault_host: "mysql",
                local_port: mysql_port,
                root_username: "root",
                root_password: mysql_password.as_str(),
            },
            &suffix,
        )
        .await
    }

    async fn validate_control_plane_migrations(
        local_port: u16,
        root_username: &str,
        root_password: &str,
        tls_ca: &str,
    ) -> IntegrationResult {
        let root_pool = postgres_pool(
            "127.0.0.1",
            local_port,
            "sql_mcp_gateway_it",
            root_username,
            root_password,
            tls_ca,
        )
        .await?;
        sqlx::query("DROP DATABASE IF EXISTS sql_mcp_gateway_control_it")
            .execute(&root_pool)
            .await
            .map_err(|_| "could not reset the isolated control-plane test database")?;
        sqlx::query("CREATE DATABASE sql_mcp_gateway_control_it")
            .execute(&root_pool)
            .await
            .map_err(|_| "could not create the isolated control-plane test database")?;
        let control_pool = postgres_pool(
            "127.0.0.1",
            local_port,
            "sql_mcp_gateway_control_it",
            root_username,
            root_password,
            tls_ca,
        )
        .await?;
        let migrations = sqlx::migrate!("./migrations");
        migrations
            .run(&control_pool)
            .await
            .map_err(|_| "control-plane migrations failed on the real PostgreSQL server")?;
        migrations
            .run(&control_pool)
            .await
            .map_err(|_| "reapplying control-plane migrations was not idempotent")?;
        let schema_is_present: bool = sqlx::query_scalar(
            "SELECT to_regtype('access_level') IS NOT NULL \
             AND to_regclass('principals') IS NOT NULL \
             AND to_regclass('database_targets') IS NOT NULL \
             AND to_regclass('principal_database_grants') IS NOT NULL \
             AND to_regclass('audit_events') IS NOT NULL",
        )
        .fetch_one(&control_pool)
        .await
        .map_err(|_| "could not verify the migrated control-plane schema")?;
        if !schema_is_present {
            return Err("control-plane migration did not create the expected schema");
        }
        control_pool.close().await;
        sqlx::query("DROP DATABASE sql_mcp_gateway_control_it")
            .execute(&root_pool)
            .await
            .map_err(|_| "could not clean up the isolated control-plane test database")?;
        root_pool.close().await;
        Ok(())
    }

    fn integration_value(name: &str) -> IntegrationResult<String> {
        std::env::var(name).map_err(|_| "missing required local integration setting")
    }

    fn integration_port(name: &str) -> IntegrationResult<u16> {
        integration_value(name)?
            .parse()
            .map_err(|_| "invalid local integration port")
    }

    fn vault_api_url(address: &Url, path: &[&str]) -> Url {
        let mut endpoint = address.clone();
        {
            let mut segments = endpoint
                .path_segments_mut()
                .expect("Vault integration URL must support path segments");
            segments.pop_if_empty();
            segments.push("v1");
            for segment in path {
                segments.push(segment);
            }
        }
        endpoint
    }

    async fn ensure_vault_database_engine(address: &Url, token: &str) -> IntegrationResult {
        let client = reqwest::Client::new();
        let mounts_response = client
            .get(vault_api_url(address, &["sys", "mounts"]))
            .header("X-Vault-Token", token)
            .send()
            .await
            .map_err(|_| "could not inspect local Vault mounts")?;
        if !mounts_response.status().is_success() {
            return Err("could not inspect local Vault mounts");
        }
        let mounts = mounts_response
            .json::<serde_json::Value>()
            .await
            .map_err(|_| "local Vault returned invalid mount metadata")?;
        if mounts
            .get("data")
            .and_then(|data| data.get("database/"))
            .is_some()
        {
            return Ok(());
        }

        let enable_response = client
            .post(vault_api_url(address, &["sys", "mounts", "database"]))
            .header("X-Vault-Token", token)
            .json(&serde_json::json!({ "type": "database" }))
            .send()
            .await
            .map_err(|_| "could not enable the local Vault database engine")?;
        if !enable_response.status().is_success() {
            return Err("could not enable the local Vault database engine");
        }
        Ok(())
    }

    async fn create_runtime_vault_token(
        address: &Url,
        admin_token: &str,
    ) -> IntegrationResult<Zeroizing<String>> {
        let client = reqwest::Client::new();
        let policy = "path \"database/creds/*\" { capabilities = [\"read\"] }\n\
                      path \"sys/leases/revoke\" { capabilities = [\"update\"] }";
        let policy_response = client
            .put(vault_api_url(
                address,
                &[
                    "sys",
                    "policies",
                    "acl",
                    "sql-mcp-gateway-integration-runtime",
                ],
            ))
            .header("X-Vault-Token", admin_token)
            .json(&serde_json::json!({ "policy": policy }))
            .send()
            .await
            .map_err(|_| "could not create the scoped Vault runtime policy")?;
        if !policy_response.status().is_success() {
            return Err("could not create the scoped Vault runtime policy");
        }

        let token_response = client
            .post(vault_api_url(address, &["auth", "token", "create"]))
            .header("X-Vault-Token", admin_token)
            .json(&serde_json::json!({
                "policies": ["sql-mcp-gateway-integration-runtime"],
                "ttl": "15m",
                "renewable": false,
                "no_default_policy": true
            }))
            .send()
            .await
            .map_err(|_| "could not create a scoped Vault runtime token")?;
        if !token_response.status().is_success() {
            return Err("could not create a scoped Vault runtime token");
        }
        let response = token_response
            .json::<VaultTokenResponse>()
            .await
            .map_err(|_| "local Vault returned an invalid runtime token")?;
        if response.auth.client_token.is_empty() {
            return Err("local Vault returned an empty runtime token");
        }
        Ok(response.auth.client_token)
    }

    #[derive(Deserialize)]
    struct VaultTokenResponse {
        auth: VaultTokenData,
    }

    #[derive(Deserialize)]
    struct VaultTokenData {
        #[serde(deserialize_with = "deserialize_zeroizing_string")]
        client_token: Zeroizing<String>,
    }

    fn deserialize_zeroizing_string<'de, D>(deserializer: D) -> Result<Zeroizing<String>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct SecretStringVisitor;

        impl<'de> serde::de::Visitor<'de> for SecretStringVisitor {
            type Value = Zeroizing<String>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a secret string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Zeroizing::new(value.to_owned()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Zeroizing::new(value))
            }
        }

        deserializer.deserialize_string(SecretStringVisitor)
    }

    async fn exercise_database_engine(
        admin: &VaultClient,
        runtime: &VaultClient,
        tls_ca: &str,
        target: IntegrationDatabase<'_>,
        suffix: &str,
    ) -> IntegrationResult {
        let engine = target.engine;
        match engine {
            DatabaseEngine::Postgresql => {
                let pool = postgres_pool(
                    "127.0.0.1",
                    target.local_port,
                    "sql_mcp_gateway_it",
                    target.root_username,
                    target.root_password,
                    tls_ca,
                )
                .await?;
                sqlx::query("DROP TABLE IF EXISTS gateway_integration_records")
                    .execute(&pool)
                    .await
                    .map_err(|_| "could not reset local PostgreSQL integration data")?;
                sqlx::query(
                    "CREATE TABLE gateway_integration_records (id BIGSERIAL PRIMARY KEY, value TEXT NOT NULL)",
                )
                .execute(&pool)
                .await
                .map_err(|_| "could not prepare local PostgreSQL integration data")?;
                sqlx::query("INSERT INTO gateway_integration_records (value) VALUES ('seed')")
                    .execute(&pool)
                    .await
                    .map_err(|_| "could not seed local PostgreSQL integration data")?;
                pool.close().await;
            }
            DatabaseEngine::Mysql => {
                let pool = mysql_pool(
                    "127.0.0.1",
                    target.local_port,
                    "sql_mcp_gateway_it",
                    target.root_username,
                    target.root_password,
                    tls_ca,
                )
                .await?;
                sqlx::query("DROP TABLE IF EXISTS gateway_integration_records")
                    .execute(&pool)
                    .await
                    .map_err(|_| "could not reset local MySQL integration data")?;
                sqlx::query(
                    "CREATE TABLE gateway_integration_records (id BIGINT AUTO_INCREMENT PRIMARY KEY, value VARCHAR(255) NOT NULL)",
                )
                .execute(&pool)
                .await
                .map_err(|_| "could not prepare local MySQL integration data")?;
                sqlx::query("INSERT INTO gateway_integration_records (value) VALUES ('seed')")
                    .execute(&pool)
                    .await
                    .map_err(|_| "could not seed local MySQL integration data")?;
                pool.close().await;
            }
        }

        let suffix_prefix = match engine {
            DatabaseEngine::Postgresql => "pg",
            DatabaseEngine::Mysql => "my",
        };
        let connection_name = format!("mcp-it-{suffix_prefix}-{suffix}");
        let read_role = format!("mcp-it-{suffix_prefix}-r-{suffix}");
        let write_role = format!("mcp-it-{suffix_prefix}-w-{suffix}");
        let vault_engine = match engine {
            DatabaseEngine::Postgresql => VaultDatabaseEngine::Postgresql,
            DatabaseEngine::Mysql => VaultDatabaseEngine::Mysql,
        };
        let (database_name, schema_name) = match engine {
            DatabaseEngine::Postgresql => ("sql_mcp_gateway_it", "public"),
            DatabaseEngine::Mysql => ("sql_mcp_gateway_it", "sql_mcp_gateway_it"),
        };
        admin
            .configure_database(VaultDatabaseConfig {
                name: &connection_name,
                engine: vault_engine,
                host: target.vault_host,
                port: match engine {
                    DatabaseEngine::Postgresql => 5432,
                    DatabaseEngine::Mysql => 3306,
                },
                database_name,
                username: target.root_username,
                password: target.root_password,
                tls_ca_pem: Some(tls_ca),
                read_role: &read_role,
                write_role: &write_role,
            })
            .await
            .map_err(|_| "Vault could not configure the local database connection")?;

        for (role_name, writable) in [(&read_role, false), (&write_role, true)] {
            let (creation_statements, revocation_statements) =
                role_statements(engine, database_name, schema_name, writable);
            admin
                .configure_role(VaultRoleConfig {
                    role_name,
                    connection_name: &connection_name,
                    creation_statements,
                    revocation_statements,
                })
                .await
                .map_err(|_| "Vault could not configure a local database role")?;
        }

        let exercise = match engine {
            DatabaseEngine::Postgresql => {
                exercise_postgres_leases(
                    runtime,
                    target.local_port,
                    database_name,
                    &read_role,
                    &write_role,
                    tls_ca,
                )
                .await
            }
            DatabaseEngine::Mysql => {
                exercise_mysql_leases(
                    runtime,
                    target.local_port,
                    database_name,
                    &read_role,
                    &write_role,
                    tls_ca,
                )
                .await
            }
        };

        let read_role_cleanup = admin.delete_database_role(&read_role).await;
        let write_role_cleanup = admin.delete_database_role(&write_role).await;
        let connection_cleanup = admin.delete_database_config(&connection_name).await;
        exercise?;
        read_role_cleanup.map_err(|_| "Vault could not clean up the local read role")?;
        write_role_cleanup.map_err(|_| "Vault could not clean up the local write role")?;
        connection_cleanup.map_err(|_| "Vault could not clean up the local database config")?;
        Ok(())
    }

    async fn postgres_pool(
        host: &str,
        port: u16,
        database: &str,
        username: &str,
        password: &str,
        tls_ca: &str,
    ) -> IntegrationResult<PgPool> {
        let mut options = PgConnectOptions::new()
            .host(host)
            .port(port)
            .database(database)
            .username(username)
            .password(password)
            .ssl_mode(PgSslMode::VerifyFull);
        options = options.ssl_root_cert_from_pem(tls_ca.as_bytes().to_vec());
        PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(options)
            .await
            .map_err(|_| "could not connect to the local PostgreSQL test database")
    }

    async fn mysql_pool(
        host: &str,
        port: u16,
        database: &str,
        username: &str,
        password: &str,
        tls_ca: &str,
    ) -> IntegrationResult<MySqlPool> {
        let mut options = MySqlConnectOptions::new()
            .host(host)
            .port(port)
            .database(database)
            .username(username)
            .password(password)
            .ssl_mode(MySqlSslMode::VerifyIdentity);
        options = options.ssl_ca_from_pem(tls_ca.as_bytes().to_vec());
        MySqlPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(options)
            .await
            .map_err(|_| "could not connect to the local MySQL test database")
    }

    async fn exercise_postgres_leases(
        runtime: &VaultClient,
        local_port: u16,
        database_name: &str,
        read_role: &str,
        write_role: &str,
        tls_ca: &str,
    ) -> IntegrationResult {
        let first_read = runtime
            .issue_database_lease(read_role)
            .await
            .map_err(|_| "Vault could not issue a PostgreSQL read lease")?;
        let second_read = runtime
            .issue_database_lease(read_role)
            .await
            .map_err(|_| "Vault could not issue a second PostgreSQL read lease")?;
        if first_read.username() == second_read.username() {
            return Err("PostgreSQL sessions did not receive distinct database identities");
        }
        if first_read.expires_in().is_zero() || second_read.expires_in().is_zero() {
            return Err("Vault returned a PostgreSQL lease without a positive TTL");
        }

        let read_pool = postgres_pool(
            "127.0.0.1",
            local_port,
            database_name,
            first_read.username(),
            first_read.password(),
            tls_ca,
        )
        .await?;
        let row = sqlx::query("SELECT value FROM gateway_integration_records")
            .fetch_one(&read_pool)
            .await
            .map_err(|_| "PostgreSQL read lease could not select existing data")?;
        let value: String = row
            .try_get("value")
            .map_err(|_| "PostgreSQL read result could not be decoded")?;
        if value != "seed" {
            return Err("PostgreSQL read lease returned unexpected test data");
        }
        let denied_write = sqlx::query(
            "INSERT INTO gateway_integration_records (value) VALUES ('read-role-write-denied')",
        )
        .execute(&read_pool)
        .await;
        if !denied_write
            .as_ref()
            .is_err_and(|error| database_error_code_is(error, "42501"))
        {
            return Err("PostgreSQL read role was able to mutate data");
        }
        read_pool.close().await;
        runtime
            .revoke_database_lease(&first_read)
            .await
            .map_err(|_| "Vault could not revoke the PostgreSQL read lease")?;
        runtime
            .revoke_database_lease(&second_read)
            .await
            .map_err(|_| "Vault could not revoke the second PostgreSQL read lease")?;
        if postgres_pool(
            "127.0.0.1",
            local_port,
            database_name,
            first_read.username(),
            first_read.password(),
            tls_ca,
        )
        .await
        .is_ok()
        {
            return Err("revoked PostgreSQL credentials could still connect");
        }

        let write_lease = runtime
            .issue_database_lease(write_role)
            .await
            .map_err(|_| "Vault could not issue a PostgreSQL write lease")?;
        let write_pool = postgres_pool(
            "127.0.0.1",
            local_port,
            database_name,
            write_lease.username(),
            write_lease.password(),
            tls_ca,
        )
        .await?;
        sqlx::query("INSERT INTO gateway_integration_records (value) VALUES ($1)")
            .bind("write-role-insert")
            .execute(&write_pool)
            .await
            .map_err(|_| "PostgreSQL write role could not insert data")?;
        sqlx::query("UPDATE gateway_integration_records SET value = $1 WHERE value = $2")
            .bind("write-role-updated")
            .bind("write-role-insert")
            .execute(&write_pool)
            .await
            .map_err(|_| "PostgreSQL write role could not update data")?;
        sqlx::query("DELETE FROM gateway_integration_records WHERE value = $1")
            .bind("write-role-updated")
            .execute(&write_pool)
            .await
            .map_err(|_| "PostgreSQL write role could not delete data")?;
        let denied_ddl = sqlx::query("CREATE TABLE gateway_integration_ddl_denied (id INTEGER)")
            .execute(&write_pool)
            .await;
        if !denied_ddl
            .as_ref()
            .is_err_and(|error| database_error_code_is(error, "42501"))
        {
            return Err("PostgreSQL write role was able to perform DDL");
        }
        write_pool.close().await;
        runtime
            .revoke_database_lease(&write_lease)
            .await
            .map_err(|_| "Vault could not revoke the PostgreSQL write lease")?;
        if postgres_pool(
            "127.0.0.1",
            local_port,
            database_name,
            write_lease.username(),
            write_lease.password(),
            tls_ca,
        )
        .await
        .is_ok()
        {
            return Err("revoked PostgreSQL write credentials could still connect");
        }
        Ok(())
    }

    async fn exercise_mysql_leases(
        runtime: &VaultClient,
        local_port: u16,
        database_name: &str,
        read_role: &str,
        write_role: &str,
        tls_ca: &str,
    ) -> IntegrationResult {
        let first_read = runtime
            .issue_database_lease(read_role)
            .await
            .map_err(|_| "Vault could not issue a MySQL read lease")?;
        let second_read = runtime
            .issue_database_lease(read_role)
            .await
            .map_err(|_| "Vault could not issue a second MySQL read lease")?;
        if first_read.username() == second_read.username() {
            return Err("MySQL sessions did not receive distinct database identities");
        }
        if first_read.expires_in().is_zero() || second_read.expires_in().is_zero() {
            return Err("Vault returned a MySQL lease without a positive TTL");
        }

        let read_pool = mysql_pool(
            "127.0.0.1",
            local_port,
            database_name,
            first_read.username(),
            first_read.password(),
            tls_ca,
        )
        .await?;
        let row = sqlx::query("SELECT value FROM gateway_integration_records")
            .fetch_one(&read_pool)
            .await
            .map_err(|_| "MySQL read lease could not select existing data")?;
        let value: String = row
            .try_get("value")
            .map_err(|_| "MySQL read result could not be decoded")?;
        if value != "seed" {
            return Err("MySQL read lease returned unexpected test data");
        }
        let denied_write = sqlx::query(
            "INSERT INTO gateway_integration_records (value) VALUES ('read-role-write-denied')",
        )
        .execute(&read_pool)
        .await;
        if !denied_write
            .as_ref()
            .is_err_and(|error| database_error_code_is(error, "42000"))
        {
            return Err("MySQL read role was able to mutate data");
        }
        read_pool.close().await;
        runtime
            .revoke_database_lease(&first_read)
            .await
            .map_err(|_| "Vault could not revoke the MySQL read lease")?;
        runtime
            .revoke_database_lease(&second_read)
            .await
            .map_err(|_| "Vault could not revoke the second MySQL read lease")?;
        if mysql_pool(
            "127.0.0.1",
            local_port,
            database_name,
            first_read.username(),
            first_read.password(),
            tls_ca,
        )
        .await
        .is_ok()
        {
            return Err("revoked MySQL credentials could still connect");
        }

        let write_lease = runtime
            .issue_database_lease(write_role)
            .await
            .map_err(|_| "Vault could not issue a MySQL write lease")?;
        let write_pool = mysql_pool(
            "127.0.0.1",
            local_port,
            database_name,
            write_lease.username(),
            write_lease.password(),
            tls_ca,
        )
        .await?;
        sqlx::query("INSERT INTO gateway_integration_records (value) VALUES (?)")
            .bind("write-role-insert")
            .execute(&write_pool)
            .await
            .map_err(|_| "MySQL write role could not insert data")?;
        sqlx::query("UPDATE gateway_integration_records SET value = ? WHERE value = ?")
            .bind("write-role-updated")
            .bind("write-role-insert")
            .execute(&write_pool)
            .await
            .map_err(|_| "MySQL write role could not update data")?;
        sqlx::query("DELETE FROM gateway_integration_records WHERE value = ?")
            .bind("write-role-updated")
            .execute(&write_pool)
            .await
            .map_err(|_| "MySQL write role could not delete data")?;
        let denied_ddl = sqlx::query("CREATE TABLE gateway_integration_ddl_denied (id INTEGER)")
            .execute(&write_pool)
            .await;
        if !denied_ddl
            .as_ref()
            .is_err_and(|error| database_error_code_is(error, "42000"))
        {
            return Err("MySQL write role was able to perform DDL");
        }
        write_pool.close().await;
        runtime
            .revoke_database_lease(&write_lease)
            .await
            .map_err(|_| "Vault could not revoke the MySQL write lease")?;
        if mysql_pool(
            "127.0.0.1",
            local_port,
            database_name,
            write_lease.username(),
            write_lease.password(),
            tls_ca,
        )
        .await
        .is_ok()
        {
            return Err("revoked MySQL write credentials could still connect");
        }
        Ok(())
    }

    fn database_error_code_is(error: &sqlx::Error, expected: &str) -> bool {
        error
            .as_database_error()
            .and_then(|database_error| database_error.code())
            .as_deref()
            == Some(expected)
    }
}
