use std::{time::Duration, time::Instant};

use bigdecimal::BigDecimal;
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{
    AssertSqlSafe, Column, Row, TypeInfo,
    mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlRow, MySqlSslMode},
    postgres::{PgConnectOptions, PgPoolOptions, PgRow, PgSslMode},
};
use uuid::Uuid;
use vault_broker::DatabaseLease;

use crate::{
    audit::{AuditEvent, record_event},
    auth::AuthenticatedPrincipal,
    domain::{AccessLevel, DatabaseEngine, DatabaseTarget},
    state::AppState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryMode {
    Read,
    Write,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum QueryError {
    #[error("query syntax is outside the allowed operation")]
    InvalidQuery,
    #[error("access to the database or requested operation is not granted")]
    Denied,
    #[error("database operation failed")]
    DatabaseFailure,
    #[error("database operation exceeded the configured time limit")]
    TimedOut,
    #[error("credential broker is unavailable")]
    BrokerUnavailable,
}

#[derive(Debug, Serialize)]
pub struct QueryResponse {
    pub columns: Vec<String>,
    pub rows: Vec<Value>,
    pub affected_rows: Option<u64>,
    pub truncated: bool,
}

struct AuthorizedTarget {
    target: DatabaseTarget,
    tls_ca_pem: Option<String>,
    read_role: String,
    write_role: String,
}

pub async fn run_database_query(
    state: &AppState,
    actor: &AuthenticatedPrincipal,
    database_id: Uuid,
    query_text: &str,
    mode: QueryMode,
) -> Result<QueryResponse, QueryError> {
    let start = Instant::now();
    let fingerprint = query_fingerprint(query_text);
    let operation = match mode {
        QueryMode::Read => "database.read_query",
        QueryMode::Write => "database.write_query",
    };
    if let Err(error) = validate_query(query_text, mode) {
        record_event(
            &state.control_pool,
            &actor.0,
            AuditEvent::new(operation, "failure")
                .database(database_id)
                .access(mode.access_level())
                .error("INVALID_QUERY")
                .query(&fingerprint)
                .duration(start.elapsed().as_millis() as i64),
        )
        .await;
        return Err(error);
    }

    let authorized = authorized_target(state, actor, database_id, mode).await;
    let authorized = match authorized {
        Ok(target) => target,
        Err(error) => {
            if matches!(error, QueryError::Denied) {
                record_event(
                    &state.control_pool,
                    &actor.0,
                    AuditEvent::new(operation, "denied")
                        .database(database_id)
                        .access(mode.access_level())
                        .error("ACCESS_DENIED")
                        .query(&fingerprint)
                        .duration(start.elapsed().as_millis() as i64),
                )
                .await;
            }
            return Err(error);
        }
    };

    let role_name = match mode {
        QueryMode::Read => &authorized.read_role,
        QueryMode::Write => &authorized.write_role,
    };
    let lease = state.vault_runtime.issue_database_lease(role_name).await;
    let lease = match lease {
        Ok(lease) => lease,
        Err(_) => {
            record_event(
                &state.control_pool,
                &actor.0,
                AuditEvent::new(operation, "failure")
                    .database(database_id)
                    .access(mode.access_level())
                    .error("CREDENTIAL_BROKER_UNAVAILABLE")
                    .query(&fingerprint)
                    .duration(start.elapsed().as_millis() as i64),
            )
            .await;
            return Err(QueryError::BrokerUnavailable);
        }
    };
    let lease_fingerprint = hash_identifier(lease.lease_id_for_audit());
    let database_principal = lease.username().to_owned();
    let query_future = run_with_lease(
        &authorized.target,
        authorized.tls_ca_pem.as_deref(),
        &lease,
        query_text,
        mode,
        state.max_query_rows,
        state.max_query_duration,
    );
    let result = match tokio::time::timeout(state.max_query_duration, query_future).await {
        Ok(result) => result,
        Err(_) => Err(QueryError::TimedOut),
    };

    let revoke_result = state.vault_runtime.revoke_database_lease(&lease).await;
    if revoke_result.is_err() {
        tracing::error!(database_id = %database_id, "Vault database lease revocation failed; TTL remains the cleanup boundary");
        record_event(
            &state.control_pool,
            &actor.0,
            AuditEvent::new("database.lease.revoke", "failure")
                .database(database_id)
                .access(mode.access_level())
                .error("LEASE_REVOCATION_FAILED")
                .query(&fingerprint)
                .database_principal(&database_principal)
                .lease(&lease_fingerprint)
                .duration(start.elapsed().as_millis() as i64),
        )
        .await;
    }

    let outcome = if result.is_ok() { "success" } else { "failure" };
    let error_code = result.as_ref().err().map(|error| match error {
        QueryError::InvalidQuery => "INVALID_QUERY",
        QueryError::Denied => "ACCESS_DENIED",
        QueryError::DatabaseFailure => "DATABASE_OPERATION_FAILED",
        QueryError::TimedOut => "QUERY_TIMEOUT",
        QueryError::BrokerUnavailable => "CREDENTIAL_BROKER_UNAVAILABLE",
    });
    record_event(&state.control_pool, &actor.0, {
        let mut event = AuditEvent::new(operation, outcome)
            .database(database_id)
            .access(mode.access_level())
            .query(&fingerprint)
            .database_principal(&database_principal)
            .lease(&lease_fingerprint)
            .duration(start.elapsed().as_millis() as i64);
        if let Some(error_code) = error_code {
            event = event.error(error_code);
        }
        event
    })
    .await;

    result
}

impl QueryMode {
    fn access_level(self) -> AccessLevel {
        match self {
            Self::Read => AccessLevel::Read,
            Self::Write => AccessLevel::Write,
        }
    }
}

async fn authorized_target(
    state: &AppState,
    actor: &AuthenticatedPrincipal,
    database_id: Uuid,
    mode: QueryMode,
) -> Result<AuthorizedTarget, QueryError> {
    let row = sqlx::query_as::<_, AuthorizedTargetRow>(
        "SELECT d.id, d.name, d.engine, d.host, d.port, d.database_name, d.schema_name, d.tls_ca_pem, \
                d.enabled, d.created_at, d.vault_read_role, d.vault_write_role, g.access_level \
         FROM database_targets d \
         LEFT JOIN principal_database_grants g \
           ON g.database_id = d.id AND g.principal_id = $2 \
         WHERE d.id = $1",
    )
    .bind(database_id)
    .bind(actor.0.id)
    .fetch_optional(&state.control_pool)
    .await
    .map_err(|_| QueryError::DatabaseFailure)?
    .ok_or(QueryError::Denied)?;

    if !row.enabled {
        return Err(QueryError::Denied);
    }
    let access_level = row.access_level.ok_or(QueryError::Denied)?;
    if !access_level.allows(mode.access_level()) {
        return Err(QueryError::Denied);
    }

    let AuthorizedTargetRow {
        id,
        name,
        engine,
        host,
        port,
        database_name,
        schema_name,
        tls_ca_pem,
        enabled,
        created_at,
        vault_read_role: read_role,
        vault_write_role: write_role,
        access_level: _,
    } = row;

    Ok(AuthorizedTarget {
        target: DatabaseTarget {
            id,
            name,
            engine,
            host,
            port,
            database_name,
            schema_name,
            enabled,
            created_at,
        },
        tls_ca_pem,
        read_role,
        write_role,
    })
}

#[derive(sqlx::FromRow)]
struct AuthorizedTargetRow {
    id: Uuid,
    name: String,
    engine: DatabaseEngine,
    host: String,
    port: i32,
    database_name: String,
    schema_name: String,
    tls_ca_pem: Option<String>,
    enabled: bool,
    created_at: chrono::DateTime<chrono::Utc>,
    vault_read_role: String,
    vault_write_role: String,
    access_level: Option<AccessLevel>,
}

fn validate_query(query_text: &str, mode: QueryMode) -> Result<(), QueryError> {
    let trimmed = query_text.trim();
    if trimmed.is_empty() || trimmed.len() > 65_536 || trimmed.contains('\0') {
        return Err(QueryError::InvalidQuery);
    }

    let statement = trimmed.strip_suffix(';').unwrap_or(trimmed).trim_end();
    if statement.contains(';') {
        return Err(QueryError::InvalidQuery);
    }

    let keyword = statement
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    match mode {
        QueryMode::Read if keyword == "SELECT" => Ok(()),
        QueryMode::Write
            if matches!(keyword.as_str(), "INSERT" | "UPDATE" | "DELETE")
                && !statement.to_ascii_uppercase().contains("RETURNING") =>
        {
            Ok(())
        }
        _ => Err(QueryError::InvalidQuery),
    }
}

fn query_fingerprint(query_text: &str) -> String {
    hash_identifier(query_text)
}

fn hash_identifier(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

async fn run_with_lease(
    target: &DatabaseTarget,
    tls_ca_pem: Option<&str>,
    lease: &DatabaseLease,
    query_text: &str,
    mode: QueryMode,
    max_rows: usize,
    max_duration: Duration,
) -> Result<QueryResponse, QueryError> {
    let pool_idle_timeout = Duration::from_secs(1);
    match target.engine {
        DatabaseEngine::Postgresql => {
            let mut options = PgConnectOptions::new()
                .host(&target.host)
                .port(u16::try_from(target.port).map_err(|_| QueryError::DatabaseFailure)?)
                .database(&target.database_name)
                .username(lease.username())
                .password(lease.password())
                .ssl_mode(PgSslMode::VerifyFull);
            if let Some(ca_pem) = tls_ca_pem {
                options = options.ssl_root_cert_from_pem(ca_pem.as_bytes().to_vec());
            }
            let pool = PgPoolOptions::new()
                .max_connections(1)
                .idle_timeout(pool_idle_timeout)
                .acquire_timeout(max_duration)
                .connect_with(options)
                .await
                .map_err(|_| QueryError::DatabaseFailure)?;
            let result = run_postgresql(&pool, query_text, mode, max_rows, max_duration).await;
            pool.close().await;
            result
        }
        DatabaseEngine::Mysql => {
            let mut options = MySqlConnectOptions::new()
                .host(&target.host)
                .port(u16::try_from(target.port).map_err(|_| QueryError::DatabaseFailure)?)
                .database(&target.database_name)
                .username(lease.username())
                .password(lease.password())
                .ssl_mode(MySqlSslMode::VerifyIdentity);
            if let Some(ca_pem) = tls_ca_pem {
                options = options.ssl_ca_from_pem(ca_pem.as_bytes().to_vec());
            }
            let pool = MySqlPoolOptions::new()
                .max_connections(1)
                .idle_timeout(pool_idle_timeout)
                .acquire_timeout(max_duration)
                .connect_with(options)
                .await
                .map_err(|_| QueryError::DatabaseFailure)?;
            let result = run_mysql(&pool, query_text, mode, max_rows, max_duration).await;
            pool.close().await;
            result
        }
    }
}

async fn run_postgresql(
    pool: &sqlx::PgPool,
    query_text: &str,
    mode: QueryMode,
    max_rows: usize,
    max_duration: Duration,
) -> Result<QueryResponse, QueryError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|_| QueryError::DatabaseFailure)?;
    if mode == QueryMode::Read {
        sqlx::query("SET TRANSACTION READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(|_| QueryError::DatabaseFailure)?;
    }
    let timeout = format!("{}ms", max_duration.as_millis());
    sqlx::query("SELECT set_config('statement_timeout', $1, true)")
        .bind(timeout)
        .execute(&mut *transaction)
        .await
        .map_err(|_| QueryError::DatabaseFailure)?;

    // The SQLx safety wrapper is intentional: the input is explicit SQL from an
    // authorized database user, constrained by validate_query and DB privileges.
    let query = sqlx::query(AssertSqlSafe(query_text));
    if mode == QueryMode::Write {
        let affected_rows = query
            .execute(&mut *transaction)
            .await
            .map_err(|_| QueryError::DatabaseFailure)?
            .rows_affected();
        transaction
            .commit()
            .await
            .map_err(|_| QueryError::DatabaseFailure)?;
        return Ok(QueryResponse {
            columns: Vec::new(),
            rows: Vec::new(),
            affected_rows: Some(affected_rows),
            truncated: false,
        });
    }

    let mut stream = query.fetch(&mut *transaction);
    let mut columns = Vec::new();
    let mut rows = Vec::new();
    let mut truncated = false;
    while let Some(row) = stream.next().await {
        let row = row.map_err(|_| QueryError::DatabaseFailure)?;
        if columns.is_empty() {
            columns = row
                .columns()
                .iter()
                .map(|column| column.name().to_owned())
                .collect();
        }
        if rows.len() >= max_rows {
            truncated = true;
            break;
        }
        rows.push(postgresql_row_to_json(&row));
    }
    drop(stream);
    if truncated {
        transaction
            .rollback()
            .await
            .map_err(|_| QueryError::DatabaseFailure)?;
    } else {
        transaction
            .commit()
            .await
            .map_err(|_| QueryError::DatabaseFailure)?;
    }

    Ok(QueryResponse {
        columns,
        rows,
        affected_rows: None,
        truncated,
    })
}

async fn run_mysql(
    pool: &sqlx::MySqlPool,
    query_text: &str,
    mode: QueryMode,
    max_rows: usize,
    max_duration: Duration,
) -> Result<QueryResponse, QueryError> {
    let timeout_ms = max_duration.as_millis().min(u128::from(u64::MAX)) as u64;
    sqlx::query("SET SESSION MAX_EXECUTION_TIME = ?")
        .bind(timeout_ms)
        .execute(pool)
        .await
        .map_err(|_| QueryError::DatabaseFailure)?;
    if mode == QueryMode::Read {
        sqlx::query("SET TRANSACTION READ ONLY")
            .execute(pool)
            .await
            .map_err(|_| QueryError::DatabaseFailure)?;
    }

    let mut transaction = pool
        .begin()
        .await
        .map_err(|_| QueryError::DatabaseFailure)?;
    // See the equivalent PostgreSQL path: user-authored SQL is deliberately
    // passed through after operation validation and native DB authorization.
    let query = sqlx::query(AssertSqlSafe(query_text));
    if mode == QueryMode::Write {
        let affected_rows = query
            .execute(&mut *transaction)
            .await
            .map_err(|_| QueryError::DatabaseFailure)?
            .rows_affected();
        transaction
            .commit()
            .await
            .map_err(|_| QueryError::DatabaseFailure)?;
        return Ok(QueryResponse {
            columns: Vec::new(),
            rows: Vec::new(),
            affected_rows: Some(affected_rows),
            truncated: false,
        });
    }

    let mut stream = query.fetch(&mut *transaction);
    let mut columns = Vec::new();
    let mut rows = Vec::new();
    let mut truncated = false;
    while let Some(row) = stream.next().await {
        let row = row.map_err(|_| QueryError::DatabaseFailure)?;
        if columns.is_empty() {
            columns = row
                .columns()
                .iter()
                .map(|column| column.name().to_owned())
                .collect();
        }
        if rows.len() >= max_rows {
            truncated = true;
            break;
        }
        rows.push(mysql_row_to_json(&row));
    }
    drop(stream);
    if truncated {
        transaction
            .rollback()
            .await
            .map_err(|_| QueryError::DatabaseFailure)?;
    } else {
        transaction
            .commit()
            .await
            .map_err(|_| QueryError::DatabaseFailure)?;
    }

    Ok(QueryResponse {
        columns,
        rows,
        affected_rows: None,
        truncated,
    })
}

fn postgresql_row_to_json(row: &PgRow) -> Value {
    let mut values = Vec::with_capacity(row.columns().len());
    for (index, column) in row.columns().iter().enumerate() {
        let value = decode_postgresql_value(row, index, column.type_info().name());
        values.push(value);
    }
    Value::Array(values)
}

fn decode_postgresql_value(row: &PgRow, index: usize, type_name: &str) -> Value {
    let value = match type_name {
        "BOOL" => row
            .try_get::<Option<bool>, _>(index)
            .map(|v| v.map(Value::Bool)),
        "INT2" => row
            .try_get::<Option<i16>, _>(index)
            .map(|v| v.map(|v| json!(v))),
        "INT4" => row
            .try_get::<Option<i32>, _>(index)
            .map(|v| v.map(|v| json!(v))),
        "INT8" => row
            .try_get::<Option<i64>, _>(index)
            .map(|v| v.map(|v| json!(v))),
        "DATE" => row
            .try_get::<Option<chrono::NaiveDate>, _>(index)
            .map(|v| v.map(|v| Value::String(v.to_string()))),
        "TIME" => row
            .try_get::<Option<chrono::NaiveTime>, _>(index)
            .map(|v| v.map(|v| Value::String(v.to_string()))),
        "TIMESTAMP" => row
            .try_get::<Option<chrono::NaiveDateTime>, _>(index)
            .map(|v| v.map(|v| Value::String(v.to_string()))),
        "TIMESTAMPTZ" => row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(index)
            .map(|v| v.map(|v| Value::String(v.to_rfc3339()))),
        "FLOAT4" => row
            .try_get::<Option<f32>, _>(index)
            .map(|v| v.map(|v| json!(v))),
        "FLOAT8" => row
            .try_get::<Option<f64>, _>(index)
            .map(|v| v.map(|v| json!(v))),
        "NUMERIC" => row
            .try_get::<Option<BigDecimal>, _>(index)
            .map(|v| v.map(|v| Value::String(v.to_string()))),
        "JSON" | "JSONB" => row
            .try_get::<Option<sqlx::types::Json<Value>>, _>(index)
            .map(|v| v.map(|v| v.0)),
        "UUID" => row
            .try_get::<Option<Uuid>, _>(index)
            .map(|v| v.map(|v| Value::String(v.to_string()))),
        "BYTEA" => row
            .try_get::<Option<Vec<u8>>, _>(index)
            .map(|v| v.map(|v| Value::String(hex_encode(&v)))),
        "_BOOL" => row
            .try_get::<Option<Vec<bool>>, _>(index)
            .map(|v| v.map(|v| json!(v))),
        "_INT2" => row
            .try_get::<Option<Vec<i16>>, _>(index)
            .map(|v| v.map(|v| json!(v))),
        "_INT4" => row
            .try_get::<Option<Vec<i32>>, _>(index)
            .map(|v| v.map(|v| json!(v))),
        "_INT8" => row
            .try_get::<Option<Vec<i64>>, _>(index)
            .map(|v| v.map(|v| json!(v))),
        "_TEXT" | "_VARCHAR" => row
            .try_get::<Option<Vec<String>>, _>(index)
            .map(|v| v.map(|v| json!(v))),
        _ => row
            .try_get::<Option<String>, _>(index)
            .map(|v| v.map(Value::String)),
    };
    value.ok().flatten().unwrap_or(Value::Null)
}

fn mysql_row_to_json(row: &MySqlRow) -> Value {
    let mut values = Vec::with_capacity(row.columns().len());
    for (index, column) in row.columns().iter().enumerate() {
        let value = decode_mysql_value(row, index, column.type_info().name());
        values.push(value);
    }
    Value::Array(values)
}

fn decode_mysql_value(row: &MySqlRow, index: usize, type_name: &str) -> Value {
    let value = match type_name {
        "TINY" | "SHORT" | "LONG" | "LONGLONG" | "INT24" => row
            .try_get::<Option<i64>, _>(index)
            .map(|v| v.map(|v| json!(v))),
        "FLOAT" | "DOUBLE" => row
            .try_get::<Option<f64>, _>(index)
            .map(|v| v.map(|v| json!(v))),
        "NEWDECIMAL" | "DECIMAL" => row
            .try_get::<Option<BigDecimal>, _>(index)
            .map(|v| v.map(|v| Value::String(v.to_string()))),
        "DATE" => row
            .try_get::<Option<chrono::NaiveDate>, _>(index)
            .map(|v| v.map(|v| Value::String(v.to_string()))),
        "DATETIME" | "TIMESTAMP" => row
            .try_get::<Option<chrono::NaiveDateTime>, _>(index)
            .map(|v| v.map(|v| Value::String(v.to_string()))),
        "TIME" => row
            .try_get::<Option<chrono::TimeDelta>, _>(index)
            .map(|v| v.map(|v| Value::String(v.to_string()))),
        "YEAR" => row
            .try_get::<Option<i32>, _>(index)
            .map(|v| v.map(|v| json!(v))),
        "JSON" => row
            .try_get::<Option<sqlx::types::Json<Value>>, _>(index)
            .map(|v| v.map(|v| v.0)),
        "BIT" | "BLOB" | "TINY_BLOB" | "MEDIUM_BLOB" | "LONG_BLOB" => row
            .try_get::<Option<Vec<u8>>, _>(index)
            .map(|v| v.map(|v| Value::String(hex_encode(&v)))),
        _ => row
            .try_get::<Option<String>, _>(index)
            .map(|v| v.map(Value::String)),
    };
    value.ok().flatten().unwrap_or(Value::Null)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{QueryError, QueryMode, hash_identifier, validate_query};

    #[test]
    fn read_queries_only_allow_single_select_statements() {
        assert!(validate_query("SELECT 1", QueryMode::Read).is_ok());
        assert!(validate_query("SELECT 1;", QueryMode::Read).is_ok());
        assert_eq!(
            validate_query("SELECT 1; DELETE FROM users", QueryMode::Read),
            Err(QueryError::InvalidQuery)
        );
        assert_eq!(
            validate_query(
                "WITH changed AS (DELETE FROM users RETURNING *) SELECT * FROM changed",
                QueryMode::Read
            ),
            Err(QueryError::InvalidQuery)
        );
        assert_eq!(
            validate_query("-- comment\nSELECT 1", QueryMode::Read),
            Err(QueryError::InvalidQuery)
        );
    }

    #[test]
    fn write_queries_allow_only_dml_without_returning_or_multiple_statements() {
        assert!(validate_query("INSERT INTO t (id) VALUES (1)", QueryMode::Write).is_ok());
        assert!(validate_query("UPDATE t SET value = 1", QueryMode::Write).is_ok());
        assert!(validate_query("DELETE FROM t WHERE id = 1", QueryMode::Write).is_ok());
        assert_eq!(
            validate_query("INSERT INTO t VALUES (1) RETURNING id", QueryMode::Write),
            Err(QueryError::InvalidQuery)
        );
        assert_eq!(
            validate_query("DELETE FROM t; DROP TABLE t", QueryMode::Write),
            Err(QueryError::InvalidQuery)
        );
        assert_eq!(
            validate_query("CREATE TABLE t (id INT)", QueryMode::Write),
            Err(QueryError::InvalidQuery)
        );
    }

    #[test]
    fn query_fingerprint_is_stable_and_does_not_contain_query_text() {
        let fingerprint = hash_identifier("SELECT email FROM users");

        assert_eq!(fingerprint.len(), 64);
        assert!(!fingerprint.contains("SELECT"));
        assert_eq!(fingerprint, hash_identifier("SELECT email FROM users"));
    }
}
