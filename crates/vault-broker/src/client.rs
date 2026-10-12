use std::time::Duration;

use reqwest::{Client, StatusCode, redirect::Policy};
use serde::{Deserialize, Deserializer, Serialize, de::Visitor};
use url::Url;
use zeroize::Zeroizing;

use crate::{DatabaseLease, VaultError};

pub struct VaultClient {
    address: Url,
    token: Zeroizing<String>,
    http: Client,
}

#[derive(Debug, Clone, Copy)]
pub enum VaultDatabaseEngine {
    Postgresql,
    Mysql,
}

pub struct VaultDatabaseConfig<'a> {
    pub name: &'a str,
    pub engine: VaultDatabaseEngine,
    pub host: &'a str,
    pub port: u16,
    pub database_name: &'a str,
    pub username: &'a str,
    pub password: &'a str,
    pub tls_ca_pem: Option<&'a str>,
    pub read_role: &'a str,
    pub write_role: &'a str,
}

pub struct VaultRoleConfig<'a> {
    pub role_name: &'a str,
    pub connection_name: &'a str,
    pub creation_statements: Vec<String>,
    pub revocation_statements: Vec<String>,
}

impl VaultClient {
    pub fn new(address: Url, token: Zeroizing<String>) -> Result<Self, VaultError> {
        if token.is_empty() {
            return Err(VaultError::EmptyToken);
        }

        if !matches!(address.scheme(), "http" | "https")
            || address.host_str().is_none()
            || !address.username().is_empty()
            || address.password().is_some()
            || address.query().is_some()
            || address.fragment().is_some()
            || (address.scheme() != "https" && !is_loopback_host(address.host_str()))
        {
            return Err(VaultError::InvalidAddress);
        }

        let http = Client::builder()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| VaultError::RequestFailed)?;

        Ok(Self {
            address,
            token,
            http,
        })
    }

    pub async fn issue_database_lease(&self, role_name: &str) -> Result<DatabaseLease, VaultError> {
        validate_role_name(role_name)?;

        let endpoint = self.endpoint(&["database", "creds", role_name])?;
        let response = self
            .http
            .get(endpoint)
            .header("X-Vault-Token", self.token.as_str())
            .send()
            .await
            .map_err(|_| VaultError::RequestFailed)?;

        let status = response.status();
        if !status.is_success() {
            return Err(rejected(status));
        }

        let response = response
            .json::<CredentialResponse>()
            .await
            .map_err(|_| VaultError::InvalidLease)?;

        let data = response.data;
        let lease_id = response.lease_id;
        if data.username.as_str().is_empty()
            || data.password.as_str().is_empty()
            || lease_id.as_str().is_empty()
            || response.lease_duration == 0
        {
            return Err(VaultError::InvalidLease);
        }

        Ok(DatabaseLease::new(
            data.username.into_inner(),
            data.password.into_inner(),
            lease_id.into_inner(),
            Duration::from_secs(response.lease_duration),
        ))
    }

    pub async fn configure_database(
        &self,
        config: VaultDatabaseConfig<'_>,
    ) -> Result<(), VaultError> {
        validate_role_name(config.name)?;
        validate_role_name(config.read_role)?;
        validate_role_name(config.write_role)?;
        if !valid_connection_host(config.host)
            || !valid_database_name(config.database_name)
            || config.username.is_empty()
            || config.password.is_empty()
            || config.port == 0
            || config
                .tls_ca_pem
                .is_some_and(|ca| ca.len() > 16_384 || !ca.contains("-----BEGIN CERTIFICATE-----"))
        {
            return Err(VaultError::InvalidDatabaseConfig);
        }

        let endpoint = self.endpoint(&["database", "config", config.name])?;
        let connection_url = match config.engine {
            VaultDatabaseEngine::Postgresql => format!(
                "postgresql://{{{{username}}}}:{{{{password}}}}@{}:{}/{}?sslmode=verify-full",
                config.host, config.port, config.database_name
            ),
            VaultDatabaseEngine::Mysql => format!(
                "{{{{username}}}}:{{{{password}}}}@tcp({}:{})/{}?tls=true",
                config.host, config.port, config.database_name
            ),
        };
        let plugin_name = match config.engine {
            VaultDatabaseEngine::Postgresql => "postgresql-database-plugin",
            VaultDatabaseEngine::Mysql => "mysql-database-plugin",
        };
        let username_template = match config.engine {
            VaultDatabaseEngine::Postgresql => "mcp-{{.RoleName | truncate 18}}-{{random 24}}",
            VaultDatabaseEngine::Mysql => "m-{{random 10}}-{{unix_time}}",
        };
        let payload = ConfigureDatabaseRequest {
            plugin_name,
            connection_url: &connection_url,
            allowed_roles: [config.read_role, config.write_role],
            username: config.username,
            password: config.password,
            username_template,
            tls_ca: config.tls_ca_pem,
            verify_connection: true,
        };
        let response = self
            .http
            .post(endpoint)
            .header("X-Vault-Token", self.token.as_str())
            .json(&payload)
            .send()
            .await
            .map_err(|_| VaultError::RequestFailed)?;

        let status = response.status();
        if !status.is_success() {
            return Err(rejected(status));
        }

        Ok(())
    }

    pub async fn configure_role(&self, config: VaultRoleConfig<'_>) -> Result<(), VaultError> {
        validate_role_name(config.role_name)?;
        validate_role_name(config.connection_name)?;

        let endpoint = self.endpoint(&["database", "roles", config.role_name])?;
        let payload = ConfigureRoleRequest {
            db_name: config.connection_name.to_owned(),
            creation_statements: config.creation_statements,
            revocation_statements: config.revocation_statements,
            default_ttl: "15m",
            max_ttl: "1h",
        };
        let response = self
            .http
            .post(endpoint)
            .header("X-Vault-Token", self.token.as_str())
            .json(&payload)
            .send()
            .await
            .map_err(|_| VaultError::RequestFailed)?;

        let status = response.status();
        if !status.is_success() {
            return Err(rejected(status));
        }

        Ok(())
    }

    pub async fn delete_database_role(&self, role_name: &str) -> Result<(), VaultError> {
        validate_role_name(role_name)?;
        self.delete_path(&["database", "roles", role_name]).await
    }

    pub async fn delete_database_config(&self, connection_name: &str) -> Result<(), VaultError> {
        validate_role_name(connection_name)?;
        self.delete_path(&["database", "config", connection_name])
            .await
    }

    async fn delete_path(&self, path: &[&str]) -> Result<(), VaultError> {
        let endpoint = self.endpoint(path)?;
        let response = self
            .http
            .delete(endpoint)
            .header("X-Vault-Token", self.token.as_str())
            .send()
            .await
            .map_err(|_| VaultError::RequestFailed)?;
        let status = response.status();
        if !status.is_success() && status != StatusCode::NOT_FOUND {
            return Err(rejected(status));
        }

        Ok(())
    }

    pub async fn revoke_database_lease(&self, lease: &DatabaseLease) -> Result<(), VaultError> {
        let endpoint = self.endpoint(&["sys", "leases", "revoke"])?;
        let body = RevokeRequest {
            lease_id: lease.lease_id(),
        };
        let response = self
            .http
            .post(endpoint)
            .header("X-Vault-Token", self.token.as_str())
            .json(&body)
            .send()
            .await
            .map_err(|_| VaultError::RequestFailed)?;

        let status = response.status();
        if !status.is_success() {
            return Err(rejected(status));
        }

        Ok(())
    }

    fn endpoint(&self, path: &[&str]) -> Result<Url, VaultError> {
        let mut endpoint = self.address.clone();
        {
            let mut segments = endpoint
                .path_segments_mut()
                .map_err(|_| VaultError::InvalidAddress)?;
            segments.pop_if_empty();
            segments.push("v1");
            for segment in path {
                segments.push(segment);
            }
        }

        Ok(endpoint)
    }
}

fn validate_role_name(role_name: &str) -> Result<(), VaultError> {
    if role_name.is_empty()
        || matches!(role_name, "." | "..")
        || !role_name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(VaultError::InvalidRoleName);
    }

    Ok(())
}

fn valid_database_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn valid_connection_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
}

fn is_loopback_host(host: Option<&str>) -> bool {
    matches!(host, Some("localhost" | "127.0.0.1" | "[::1]"))
}

fn rejected(status: StatusCode) -> VaultError {
    VaultError::Rejected(status.as_u16())
}

#[derive(Deserialize)]
struct CredentialResponse {
    lease_id: SecretValue,
    lease_duration: u64,
    data: CredentialData,
}

#[derive(Deserialize)]
struct CredentialData {
    username: SecretValue,
    password: SecretValue,
}

struct SecretValue(Zeroizing<String>);

impl SecretValue {
    fn as_str(&self) -> &str {
        self.0.as_str()
    }

    fn into_inner(self) -> Zeroizing<String> {
        self.0
    }
}

impl<'de> Deserialize<'de> for SecretValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SecretValueVisitor;

        impl Visitor<'_> for SecretValueVisitor {
            type Value = SecretValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a secret string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(SecretValue(Zeroizing::new(value.to_owned())))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(SecretValue(Zeroizing::new(value)))
            }
        }

        deserializer.deserialize_string(SecretValueVisitor)
    }
}

#[derive(Serialize)]
struct RevokeRequest<'a> {
    lease_id: &'a str,
}

#[derive(Serialize)]
struct ConfigureDatabaseRequest<'a> {
    plugin_name: &'a str,
    connection_url: &'a str,
    allowed_roles: [&'a str; 2],
    username: &'a str,
    password: &'a str,
    username_template: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tls_ca: Option<&'a str>,
    verify_connection: bool,
}

#[derive(Serialize)]
struct ConfigureRoleRequest {
    db_name: String,
    creation_statements: Vec<String>,
    revocation_statements: Vec<String>,
    default_ttl: &'static str,
    max_ttl: &'static str,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        Json, Router,
        extract::{Path, State},
        http::{HeaderMap, StatusCode},
        routing::{get, post},
    };
    use serde_json::{Value, json};
    use tokio::{net::TcpListener, sync::Mutex, task::JoinHandle};
    use url::Url;
    use zeroize::Zeroizing;

    use crate::{VaultClient, VaultError};

    #[derive(Clone)]
    struct TestState {
        issue_roles: Arc<Mutex<Vec<String>>>,
        revoked_leases: Arc<Mutex<Vec<String>>>,
    }

    async fn issue_credentials(
        State(state): State<TestState>,
        Path(role_name): Path<String>,
        headers: HeaderMap,
    ) -> (StatusCode, Json<Value>) {
        if headers
            .get("X-Vault-Token")
            .and_then(|value| value.to_str().ok())
            != Some("synthetic-test-token")
        {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"errors": ["fixture denied"]})),
            );
        }

        state.issue_roles.lock().await.push(role_name.clone());
        if role_name == "forbidden" {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"errors": ["fixture response must not leak"]})),
            );
        }

        (
            StatusCode::OK,
            Json(json!({
                "lease_id": "database/creds/pg-read/synthetic-lease",
                "lease_duration": 300,
                "data": {
                    "username": "synthetic-dynamic-user",
                    "password": "synthetic-dynamic-password"
                }
            })),
        )
    }

    async fn revoke_lease(
        State(state): State<TestState>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> StatusCode {
        if headers
            .get("X-Vault-Token")
            .and_then(|value| value.to_str().ok())
            != Some("synthetic-test-token")
        {
            return StatusCode::FORBIDDEN;
        }

        if let Some(lease_id) = body.get("lease_id").and_then(Value::as_str) {
            state.revoked_leases.lock().await.push(lease_id.to_owned());
            return StatusCode::NO_CONTENT;
        }

        StatusCode::BAD_REQUEST
    }

    async fn test_server() -> (Url, TestState, JoinHandle<()>) {
        let state = TestState {
            issue_roles: Arc::new(Mutex::new(Vec::new())),
            revoked_leases: Arc::new(Mutex::new(Vec::new())),
        };
        let app = Router::new()
            .route("/v1/database/creds/{role_name}", get(issue_credentials))
            .route("/v1/sys/leases/revoke", post(revoke_lease))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener should bind");
        let address = Url::parse(&format!(
            "http://{}/",
            listener
                .local_addr()
                .expect("test listener should have an address")
        ))
        .expect("test server URL should be valid");
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        (address, state, task)
    }

    fn test_client(address: Url) -> VaultClient {
        VaultClient::new(address, Zeroizing::new("synthetic-test-token".to_owned()))
            .expect("synthetic test client should be valid")
    }

    #[tokio::test]
    async fn issues_and_revokes_a_dynamic_database_lease() {
        let (address, state, server) = test_server().await;
        let client = test_client(address);

        let lease = client
            .issue_database_lease("pg-read")
            .await
            .expect("Vault fixture should return a lease");

        assert_eq!(lease.username(), "synthetic-dynamic-user");
        assert_eq!(lease.password(), "synthetic-dynamic-password");
        assert_eq!(lease.expires_in().as_secs(), 300);
        assert_eq!(
            state.issue_roles.lock().await.as_slice(),
            &["pg-read".to_owned()]
        );

        client
            .revoke_database_lease(&lease)
            .await
            .expect("Vault fixture should revoke the lease");

        assert_eq!(
            state.revoked_leases.lock().await.as_slice(),
            &["database/creds/pg-read/synthetic-lease".to_owned()]
        );
        server.abort();
    }

    #[tokio::test]
    async fn does_not_expose_vault_error_bodies() {
        let (address, server_state, server) = test_server().await;
        let client = test_client(address);

        let result = client.issue_database_lease("forbidden").await;

        assert!(matches!(&result, Err(VaultError::Rejected(403))));
        if let Err(error) = result {
            assert!(!error.to_string().contains("fixture response must not leak"));
            assert!(!error.to_string().contains("synthetic-test-token"));
        }
        assert_eq!(server_state.issue_roles.lock().await.len(), 1);
        server.abort();
    }

    #[tokio::test]
    async fn rejects_role_names_that_could_escape_the_vault_path() {
        let (address, _state, server) = test_server().await;
        let client = test_client(address);

        for role_name in ["../admin", ".."] {
            let result = client.issue_database_lease(role_name).await;

            assert!(matches!(result, Err(VaultError::InvalidRoleName)));
        }
        server.abort();
    }
}
