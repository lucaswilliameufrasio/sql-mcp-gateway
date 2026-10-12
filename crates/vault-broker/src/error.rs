use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VaultError {
    #[error(
        "Vault address must be an HTTP(S) URL without embedded credentials, query, or fragment"
    )]
    InvalidAddress,
    #[error("Vault token must not be empty")]
    EmptyToken,
    #[error("Vault role name contains unsupported characters")]
    InvalidRoleName,
    #[error("Vault database configuration is invalid")]
    InvalidDatabaseConfig,
    #[error("Vault request failed")]
    RequestFailed,
    #[error("Vault returned HTTP status {0}")]
    Rejected(u16),
    #[error("Vault returned an invalid database credential lease")]
    InvalidLease,
}
