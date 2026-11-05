use axum::{
    Extension, Json,
    extract::{Query, State, rejection::QueryRejection},
    http::header,
    response::IntoResponse,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

use crate::{
    api_error::ApiError, auth::AuthenticatedPrincipal, domain::PlatformRole, state::AppState,
};

#[derive(Debug, Default, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    #[default]
    #[serde(rename = "24h")]
    Day,
    #[serde(rename = "7d")]
    Week,
    #[serde(rename = "30d")]
    Month,
}

impl Period {
    fn duration(self) -> Duration {
        match self {
            Self::Day => Duration::hours(24),
            Self::Week => Duration::days(7),
            Self::Month => Duration::days(30),
        }
    }

    fn bucket_seconds(self) -> i32 {
        match self {
            Self::Day => 3600,
            Self::Week | Self::Month => 86400,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricsQuery {
    #[serde(default)]
    period: Period,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Summary {
    pub total_queries: i64,
    pub successful_queries: i64,
    pub failed_queries: i64,
    pub denied_queries: i64,
    pub read_queries: i64,
    pub write_queries: i64,
    pub active_users: i64,
    pub used_databases: i64,
    pub timed_successful_queries: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_duration_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p95_duration_ms: Option<f64>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Bucket {
    pub start: DateTime<Utc>,
    pub total_queries: i64,
    pub successful_queries: i64,
    pub failed_queries: i64,
    pub denied_queries: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct DatabaseUsage {
    pub database_id: Uuid,
    pub name: String,
    pub total_queries: i64,
    pub successful_queries: i64,
    pub failed_queries: i64,
    pub denied_queries: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct ErrorCount {
    pub error_code: String,
    pub outcome: String,
    pub count: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Inventory {
    pub active_databases: i64,
    pub paused_databases: i64,
    pub tls_disabled_databases: i64,
    pub enabled_members: i64,
    pub active_grants: i64,
    pub members_without_access: i64,
}

#[derive(Debug, Serialize)]
pub struct Metrics {
    pub scope: &'static str,
    pub period: Period,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub bucket_seconds: i32,
    pub summary: Summary,
    pub series: Vec<Bucket>,
    pub top_databases: Vec<DatabaseUsage>,
    pub top_errors: Vec<ErrorCount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inventory: Option<Inventory>,
}

pub async fn get_metrics(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthenticatedPrincipal>,
    input: Result<Query<MetricsQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Query(input) = input.map_err(|_| ApiError::InvalidRequest)?;
    if !actor.0.enabled {
        return Err(ApiError::Forbidden);
    }
    let principal_id = match actor.0.platform_role {
        PlatformRole::Admin => None,
        PlatformRole::Member => Some(actor.0.id),
    };
    let result = load_metrics(&state.control_pool, principal_id, input.period, Utc::now())
        .await
        .map_err(|_| ApiError::DependencyUnavailable)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(result)))
}

async fn load_metrics(
    pool: &PgPool,
    principal_id: Option<Uuid>,
    period: Period,
    to: DateTime<Utc>,
) -> Result<Metrics, sqlx::Error> {
    let from = to - period.duration();
    let bucket_seconds = period.bucket_seconds();
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL TIME ZONE 'UTC'")
        .execute(&mut *tx)
        .await?;

    let summary = sqlx::query_as::<_, Summary>(
        "SELECT COUNT(*) AS total_queries, \
         COUNT(*) FILTER (WHERE outcome = 'success') AS successful_queries, \
         COUNT(*) FILTER (WHERE outcome = 'failure') AS failed_queries, \
         COUNT(*) FILTER (WHERE outcome = 'denied') AS denied_queries, \
         COUNT(*) FILTER (WHERE operation = 'database.read_query') AS read_queries, \
         COUNT(*) FILTER (WHERE operation = 'database.write_query') AS write_queries, \
         COUNT(DISTINCT principal_id) AS active_users, \
         COUNT(DISTINCT database_id) AS used_databases, \
         COUNT(duration_ms) FILTER (WHERE outcome = 'success') AS timed_successful_queries, \
         (AVG(duration_ms) FILTER (WHERE outcome = 'success'))::float8 AS avg_duration_ms, \
         PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY duration_ms::float8) \
             FILTER (WHERE outcome = 'success') AS p95_duration_ms \
         FROM audit_events WHERE created_at >= $1 AND created_at < $2 \
         AND ($3::uuid IS NULL OR principal_id = $3) \
         AND operation IN ('database.read_query', 'database.write_query')",
    )
    .bind(from)
    .bind(to)
    .bind(principal_id)
    .fetch_one(&mut *tx)
    .await?;

    let series = sqlx::query_as::<_, Bucket>(
        "WITH buckets AS ( \
             SELECT generate_series($1::timestamptz, $2::timestamptz - make_interval(secs => $4::int), \
                                    make_interval(secs => $4::int)) AS start \
         ), counts AS ( \
             SELECT date_bin(make_interval(secs => $4::int), created_at, $1) AS start, \
             COUNT(*) AS total_queries, \
             COUNT(*) FILTER (WHERE outcome = 'success') AS successful_queries, \
             COUNT(*) FILTER (WHERE outcome = 'failure') AS failed_queries, \
             COUNT(*) FILTER (WHERE outcome = 'denied') AS denied_queries \
             FROM audit_events WHERE created_at >= $1 AND created_at < $2 \
             AND ($3::uuid IS NULL OR principal_id = $3) \
             AND operation IN ('database.read_query', 'database.write_query') \
             GROUP BY 1 \
         ) SELECT b.start, COALESCE(c.total_queries, 0) AS total_queries, \
             COALESCE(c.successful_queries, 0) AS successful_queries, \
             COALESCE(c.failed_queries, 0) AS failed_queries, \
             COALESCE(c.denied_queries, 0) AS denied_queries \
         FROM buckets b LEFT JOIN counts c USING (start) ORDER BY b.start",
    )
    .bind(from)
    .bind(to)
    .bind(principal_id)
    .bind(bucket_seconds)
    .fetch_all(&mut *tx)
    .await?;

    let top_databases = sqlx::query_as::<_, DatabaseUsage>(
        "SELECT d.id AS database_id, d.name, COUNT(*) AS total_queries, \
         COUNT(*) FILTER (WHERE a.outcome = 'success') AS successful_queries, \
         COUNT(*) FILTER (WHERE a.outcome = 'failure') AS failed_queries, \
         COUNT(*) FILTER (WHERE a.outcome = 'denied') AS denied_queries \
         FROM audit_events a JOIN database_targets d ON d.id = a.database_id \
         WHERE a.created_at >= $1 AND a.created_at < $2 \
         AND ($3::uuid IS NULL OR a.principal_id = $3) \
         AND a.operation IN ('database.read_query', 'database.write_query') \
         AND ($3::uuid IS NULL OR (d.enabled AND EXISTS ( \
             SELECT 1 FROM principal_database_grants g WHERE g.database_id = d.id AND g.principal_id = $3))) \
         GROUP BY d.id, d.name ORDER BY total_queries DESC, d.name, d.id LIMIT 5",
    )
    .bind(from)
    .bind(to)
    .bind(principal_id)
    .fetch_all(&mut *tx)
    .await?;

    let top_errors = sqlx::query_as::<_, ErrorCount>(
        "SELECT COALESCE(error_code, 'UNCLASSIFIED') AS error_code, outcome, COUNT(*) AS count \
         FROM audit_events WHERE created_at >= $1 AND created_at < $2 \
         AND ($3::uuid IS NULL OR principal_id = $3) \
         AND operation IN ('database.read_query', 'database.write_query') \
         AND outcome IN ('failure', 'denied') \
         GROUP BY COALESCE(error_code, 'UNCLASSIFIED'), outcome \
         ORDER BY count DESC, error_code, outcome LIMIT 5",
    )
    .bind(from)
    .bind(to)
    .bind(principal_id)
    .fetch_all(&mut *tx)
    .await?;

    let inventory = if principal_id.is_none() {
        Some(load_inventory(&mut tx).await?)
    } else {
        None
    };
    tx.commit().await?;
    Ok(Metrics {
        scope: if principal_id.is_some() {
            "personal"
        } else {
            "workspace"
        },
        period,
        from,
        to,
        bucket_seconds,
        summary,
        series,
        top_databases,
        top_errors,
        inventory,
    })
}

async fn load_inventory(connection: &mut PgConnection) -> Result<Inventory, sqlx::Error> {
    sqlx::query_as::<_, Inventory>(
        "SELECT \
         (SELECT COUNT(*) FROM database_targets WHERE enabled) AS active_databases, \
         (SELECT COUNT(*) FROM database_targets WHERE NOT enabled) AS paused_databases, \
         (SELECT COUNT(*) FROM database_targets WHERE enabled AND tls_mode = 'disable') AS tls_disabled_databases, \
         (SELECT COUNT(*) FROM principals WHERE enabled AND platform_role = 'member') AS enabled_members, \
         (SELECT COUNT(*) FROM principal_database_grants g \
             JOIN principals p ON p.id = g.principal_id JOIN database_targets d ON d.id = g.database_id \
             WHERE p.enabled AND d.enabled) AS active_grants, \
         (SELECT COUNT(*) FROM principals p WHERE p.enabled AND p.platform_role = 'member' AND NOT EXISTS ( \
             SELECT 1 FROM principal_database_grants g JOIN database_targets d ON d.id = g.database_id \
             WHERE g.principal_id = p.id AND d.enabled)) AS members_without_access",
    )
    .fetch_one(connection)
    .await
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn periods_are_bounded_and_invalid_parameters_fail() {
        assert_eq!(Period::Day.duration().num_hours(), 24);
        assert_eq!(Period::Week.duration().num_days(), 7);
        assert_eq!(Period::Month.duration().num_days(), 30);
        assert_eq!(Period::Day.bucket_seconds(), 3600);
        assert_eq!(Period::Month.bucket_seconds(), 86400);
        for input in [r#"{"period":"365d"}"#, r#"{"principal_id":"other"}"#] {
            assert!(serde_json::from_str::<MetricsQuery>(input).is_err());
        }
        assert_eq!(
            serde_json::from_str::<MetricsQuery>("{}").unwrap().period,
            Period::Day
        );
    }

    // Called by the existing isolated control-plane migration integration test.
    pub(crate) async fn verify_metrics_snapshot(pool: &PgPool) {
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();
        let database = Uuid::new_v4();
        let to = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        for (id, subject) in [(alice, "metrics-alice"), (bob, "metrics-bob")] {
            sqlx::query("INSERT INTO principals (id, issuer, subject) VALUES ($1, 'https://identity.example.test', $2)")
                .bind(id).bind(subject).execute(pool).await.unwrap();
        }
        sqlx::query("INSERT INTO database_targets (id, name, engine, host, port, database_name, tls_mode, vault_connection_name, vault_read_role, created_by) \
            VALUES ($1, 'metrics-test', 'postgresql', 'db.example.test', 5432, 'metrics', 'disable', 'metrics-connection', 'metrics-role', $2)")
            .bind(database).bind(alice).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO principal_database_grants (principal_id, database_id, access_level, granted_by) VALUES ($1, $2, 'read', $1)")
            .bind(alice).bind(database).execute(pool).await.unwrap();

        // More rows than the audit UI's 100-row limit; duration 1..110ms.
        sqlx::query("INSERT INTO audit_events (id, principal_id, issuer, subject, database_id, operation, outcome, duration_ms, created_at) \
            SELECT gen_random_uuid(), $1, 'https://identity.example.test', 'metrics-alice', $2, \
            'database.read_query', 'success', n, $3::timestamptz - INTERVAL '1 hour' FROM generate_series(1, 110) n")
            .bind(alice).bind(database).bind(to).execute(pool).await.unwrap();

        // Inclusive start, exclusive end, and successful null timing.
        for (actor, operation, outcome, duration, when) in [
            (
                alice,
                "database.write_query",
                "success",
                Some(400_i64),
                to - Duration::hours(24),
            ),
            (
                alice,
                "database.read_query",
                "success",
                None,
                to - Duration::minutes(30),
            ),
            (
                alice,
                "database.read_query",
                "failure",
                Some(999_999),
                to - Duration::minutes(20),
            ),
            (
                alice,
                "database.read_query",
                "denied",
                Some(1),
                to - Duration::minutes(10),
            ),
            (alice, "database.read_query", "success", Some(800), to),
            (
                alice,
                "database.read_query",
                "success",
                Some(800),
                to - Duration::hours(24) - Duration::seconds(1),
            ),
            (
                alice,
                "database.read_query",
                "success",
                Some(800),
                to - Duration::days(31),
            ),
            (
                alice,
                "database.lease.revoke",
                "failure",
                Some(999_999),
                to - Duration::minutes(5),
            ),
            (
                alice,
                "database.grant.set",
                "success",
                None,
                to - Duration::minutes(5),
            ),
            (
                bob,
                "database.read_query",
                "success",
                Some(5_000),
                to - Duration::minutes(5),
            ),
        ] {
            sqlx::query("INSERT INTO audit_events (id, principal_id, issuer, subject, database_id, operation, outcome, duration_ms, created_at) \
                VALUES ($1, $2, 'https://identity.example.test', 'metrics-test', $3, $4, $5, $6, $7)")
                .bind(Uuid::new_v4()).bind(actor).bind(database).bind(operation).bind(outcome).bind(duration).bind(when)
                .execute(pool).await.unwrap();
        }

        let personal = load_metrics(pool, Some(alice), Period::Day, to)
            .await
            .unwrap();
        assert_eq!(personal.scope, "personal");
        assert_eq!(personal.summary.total_queries, 114);
        assert_eq!(personal.summary.successful_queries, 112);
        assert_eq!(personal.summary.failed_queries, 1);
        assert_eq!(personal.summary.denied_queries, 1);
        assert_eq!(personal.summary.read_queries, 113);
        assert_eq!(personal.summary.write_queries, 1);
        assert_eq!(personal.summary.active_users, 1);
        assert_eq!(personal.summary.timed_successful_queries, 111);
        assert!((personal.summary.avg_duration_ms.unwrap() - 6505.0 / 111.0).abs() < 0.001);
        assert!((personal.summary.p95_duration_ms.unwrap() - 105.5).abs() < 0.001);
        assert_eq!(personal.series.len(), 24);
        assert_eq!(personal.series[0].successful_queries, 1);
        assert_eq!(
            personal
                .series
                .iter()
                .map(|bucket| bucket.total_queries)
                .sum::<i64>(),
            114
        );
        assert!(
            personal
                .series
                .iter()
                .any(|bucket| bucket.total_queries == 0)
        );
        assert!(personal.inventory.is_none());
        assert_eq!(personal.top_databases.len(), 1);
        assert_eq!(personal.top_databases[0].total_queries, 114);
        assert_eq!(personal.top_errors.len(), 2);
        assert!(
            personal
                .top_errors
                .iter()
                .all(|error| error.error_code == "UNCLASSIFIED" && error.count == 1)
        );

        let workspace = load_metrics(pool, None, Period::Day, to).await.unwrap();
        assert_eq!(workspace.summary.total_queries, 115);
        assert_eq!(workspace.summary.active_users, 2);
        let inventory = workspace.inventory.unwrap();
        assert_eq!(inventory.active_databases, 1);
        assert_eq!(inventory.tls_disabled_databases, 1);
        assert_eq!(inventory.enabled_members, 2);
        assert_eq!(inventory.members_without_access, 1);
        assert_eq!(inventory.active_grants, 1);
        for (period, bucket_count) in [(Period::Week, 7), (Period::Month, 30)] {
            let longer = load_metrics(pool, Some(alice), period, to).await.unwrap();
            assert_eq!(longer.summary.total_queries, 115);
            assert_eq!(longer.series.len(), bucket_count);
        }
        let bob_metrics = load_metrics(pool, Some(bob), Period::Day, to)
            .await
            .unwrap();
        assert_eq!(bob_metrics.summary.total_queries, 1);
        assert!(
            bob_metrics.top_databases.is_empty(),
            "an ungranted database name must remain private"
        );
        sqlx::query("DELETE FROM principal_database_grants WHERE principal_id = $1")
            .bind(alice)
            .execute(pool)
            .await
            .unwrap();
        let revoked = load_metrics(pool, Some(alice), Period::Day, to)
            .await
            .unwrap();
        assert_eq!(revoked.summary.total_queries, 114);
        assert!(revoked.top_databases.is_empty());
        let empty = load_metrics(pool, Some(alice), Period::Day, to + Duration::days(100))
            .await
            .unwrap();
        assert_eq!(empty.summary.total_queries, 0);
        assert!(empty.series.iter().all(|bucket| bucket.total_queries == 0));
        let serialized = serde_json::to_value(empty).unwrap();
        assert!(serialized.get("inventory").is_none());
        assert!(serialized["summary"].get("avg_duration_ms").is_none());
        assert!(serialized["summary"].get("p95_duration_ms").is_none());
    }
}
