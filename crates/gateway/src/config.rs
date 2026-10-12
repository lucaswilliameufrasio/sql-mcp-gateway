use std::{collections::HashSet, net::SocketAddr, time::Duration};

use url::Url;
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Settings {
    pub bind_address: SocketAddr,
    pub control_database_url: Zeroizing<String>,
    pub oidc_issuer: Url,
    pub oidc_client_id: String,
    pub oidc_access_token_audience: String,
    pub bootstrap_admin_subjects: HashSet<String>,
    pub vault_address: Url,
    pub vault_admin_token: Zeroizing<String>,
    pub vault_runtime_token: Zeroizing<String>,
    pub max_query_rows: usize,
    pub max_query_duration: Duration,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("BIND_ADDRESS must be a valid socket address")]
    InvalidBindAddress,
    #[error("CONTROL_DATABASE_URL is required")]
    MissingControlDatabaseUrl,
    #[error("OIDC_ISSUER_URL must use HTTPS, except on loopback for local development")]
    InvalidOidcIssuer,
    #[error("OIDC_CLIENT_ID is required")]
    MissingOidcClientId,
    #[error("OIDC_ACCESS_TOKEN_AUDIENCE is required")]
    MissingOidcAccessTokenAudience,
    #[error("OIDC_BOOTSTRAP_ADMIN_SUBJECTS must contain at least one subject")]
    MissingBootstrapAdministrators,
    #[error("VAULT_ADDRESS must use HTTPS, except on loopback for local development")]
    InvalidVaultAddress,
    #[error("VAULT_ADMIN_TOKEN and VAULT_RUNTIME_TOKEN are required")]
    MissingVaultTokens,
    #[error("VAULT_ADMIN_TOKEN and VAULT_RUNTIME_TOKEN must be different")]
    VaultTokensMustDiffer,
    #[error("MAX_QUERY_ROWS must be between 1 and 1000")]
    InvalidMaxQueryRows,
    #[error("MAX_QUERY_DURATION_SECONDS must be between 1 and 300")]
    InvalidMaxQueryDuration,
}

impl Settings {
    pub fn from_environment() -> Result<Self, ConfigError> {
        let bind_address = std::env::var("BIND_ADDRESS")
            .unwrap_or_else(|_| "127.0.0.1:8080".to_owned())
            .parse()
            .map_err(|_| ConfigError::InvalidBindAddress)?;
        let control_database_url = required_environment("CONTROL_DATABASE_URL")
            .ok_or(ConfigError::MissingControlDatabaseUrl)?;
        let oidc_issuer = parse_http_url("OIDC_ISSUER_URL", ConfigError::InvalidOidcIssuer)?;
        let oidc_client_id =
            required_environment("OIDC_CLIENT_ID").ok_or(ConfigError::MissingOidcClientId)?;
        let oidc_access_token_audience = required_environment("OIDC_ACCESS_TOKEN_AUDIENCE")
            .ok_or(ConfigError::MissingOidcAccessTokenAudience)?;
        let bootstrap_admin_subjects = std::env::var("OIDC_BOOTSTRAP_ADMIN_SUBJECTS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|subject| !subject.is_empty())
            .map(str::to_owned)
            .collect::<HashSet<_>>();

        if bootstrap_admin_subjects.is_empty() {
            return Err(ConfigError::MissingBootstrapAdministrators);
        }

        let vault_address = parse_http_url("VAULT_ADDRESS", ConfigError::InvalidVaultAddress)?;
        let vault_admin_token =
            required_environment("VAULT_ADMIN_TOKEN").ok_or(ConfigError::MissingVaultTokens)?;
        let vault_runtime_token =
            required_environment("VAULT_RUNTIME_TOKEN").ok_or(ConfigError::MissingVaultTokens)?;
        if !vault_tokens_are_distinct(&vault_admin_token, &vault_runtime_token) {
            return Err(ConfigError::VaultTokensMustDiffer);
        }
        let max_query_rows = parse_bounded_number("MAX_QUERY_ROWS", 200, 1, 1000)
            .ok_or(ConfigError::InvalidMaxQueryRows)?;
        let max_query_duration_seconds =
            parse_bounded_number("MAX_QUERY_DURATION_SECONDS", 30, 1, 300)
                .ok_or(ConfigError::InvalidMaxQueryDuration)?;

        Ok(Self {
            bind_address,
            control_database_url: Zeroizing::new(control_database_url),
            oidc_issuer,
            oidc_client_id,
            oidc_access_token_audience,
            bootstrap_admin_subjects,
            vault_address,
            vault_admin_token: Zeroizing::new(vault_admin_token),
            vault_runtime_token: Zeroizing::new(vault_runtime_token),
            max_query_rows,
            max_query_duration: Duration::from_secs(max_query_duration_seconds as u64),
        })
    }
}

fn required_environment(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn parse_http_url(name: &str, error: ConfigError) -> Result<Url, ConfigError> {
    let value = required_environment(name).ok_or_else(|| error.clone())?;
    let url = Url::parse(&value).map_err(|_| error.clone())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !has_secure_transport(&url)
    {
        return Err(error);
    }

    Ok(url)
}

fn is_loopback_host(host: Option<&str>) -> bool {
    host.is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

fn has_secure_transport(url: &Url) -> bool {
    url.scheme() == "https" || (url.scheme() == "http" && is_loopback_host(url.host_str()))
}

fn vault_tokens_are_distinct(admin_token: &str, runtime_token: &str) -> bool {
    !admin_token.is_empty() && !runtime_token.is_empty() && admin_token != runtime_token
}

fn parse_bounded_number(
    name: &str,
    default: usize,
    minimum: usize,
    maximum: usize,
) -> Option<usize> {
    match std::env::var(name) {
        Ok(value) => value
            .parse::<usize>()
            .ok()
            .filter(|value| (minimum..=maximum).contains(value)),
        Err(_) => Some(default),
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::{has_secure_transport, is_loopback_host, vault_tokens_are_distinct};

    #[test]
    fn only_loopback_http_hosts_are_allowed_for_local_service_urls() {
        assert!(is_loopback_host(Some("localhost")));
        assert!(is_loopback_host(Some("127.0.0.1")));
        assert!(is_loopback_host(Some("::1")));
        let ipv6_loopback = Url::parse("http://[::1]").expect("valid loopback URL");
        assert!(is_loopback_host(ipv6_loopback.host_str()));
        assert!(!is_loopback_host(Some("localhost.attacker.example")));
        assert!(!is_loopback_host(Some("192.0.2.1")));

        let remote_http = Url::parse("http://oidc.example.test").expect("valid URL");
        let remote_https = Url::parse("https://oidc.example.test").expect("valid URL");
        assert!(!has_secure_transport(&remote_http));
        assert!(has_secure_transport(&remote_https));
        assert!(has_secure_transport(&ipv6_loopback));
    }

    #[test]
    fn vault_admin_and_runtime_credentials_must_be_distinct() {
        assert!(vault_tokens_are_distinct("admin-token", "runtime-token"));
        assert!(!vault_tokens_are_distinct("same-token", "same-token"));
        assert!(!vault_tokens_are_distinct("", "runtime-token"));
    }
}
