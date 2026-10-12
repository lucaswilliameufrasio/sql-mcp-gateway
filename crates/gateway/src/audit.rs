use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{AccessLevel, Principal};

pub struct AuditEvent<'a> {
    database_id: Option<Uuid>,
    operation: &'a str,
    access_level: Option<AccessLevel>,
    outcome: &'a str,
    error_code: Option<&'a str>,
    query_fingerprint: Option<&'a str>,
    database_principal: Option<&'a str>,
    vault_lease_fingerprint: Option<&'a str>,
    duration_ms: Option<i64>,
}

impl<'a> AuditEvent<'a> {
    pub fn new(operation: &'a str, outcome: &'a str) -> Self {
        Self {
            database_id: None,
            operation,
            access_level: None,
            outcome,
            error_code: None,
            query_fingerprint: None,
            database_principal: None,
            vault_lease_fingerprint: None,
            duration_ms: None,
        }
    }

    pub fn database(mut self, database_id: Uuid) -> Self {
        self.database_id = Some(database_id);
        self
    }

    pub fn access(mut self, access_level: AccessLevel) -> Self {
        self.access_level = Some(access_level);
        self
    }

    pub fn error(mut self, error_code: &'a str) -> Self {
        self.error_code = Some(error_code);
        self
    }

    pub fn query(mut self, fingerprint: &'a str) -> Self {
        self.query_fingerprint = Some(fingerprint);
        self
    }

    pub fn database_principal(mut self, principal: &'a str) -> Self {
        self.database_principal = Some(principal);
        self
    }

    pub fn lease(mut self, fingerprint: &'a str) -> Self {
        self.vault_lease_fingerprint = Some(fingerprint);
        self
    }

    pub fn duration(mut self, duration_ms: i64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }
}

pub async fn record_event(pool: &PgPool, principal: &Principal, event: AuditEvent<'_>) {
    let access_text = event.access_level.map(|value| match value {
        AccessLevel::Read => "read",
        AccessLevel::Write => "write",
    });

    let result = sqlx::query(
        "INSERT INTO audit_events \
         (id, principal_id, issuer, subject, database_id, operation, access_level, outcome, \
          error_code, query_fingerprint, database_principal, vault_lease_fingerprint, duration_ms) \
         VALUES ($1, $2, $3, $4, $5, $6, $7::access_level, $8, $9, $10, $11, $12, $13)",
    )
    .bind(Uuid::now_v7())
    .bind(principal.id)
    .bind(&principal.issuer)
    .bind(&principal.subject)
    .bind(event.database_id)
    .bind(event.operation)
    .bind(access_text)
    .bind(event.outcome)
    .bind(event.error_code)
    .bind(event.query_fingerprint)
    .bind(event.database_principal)
    .bind(event.vault_lease_fingerprint)
    .bind(event.duration_ms)
    .execute(pool)
    .await;

    if result.is_err() {
        tracing::error!(operation = event.operation, "failed to persist audit event");
    }
}
