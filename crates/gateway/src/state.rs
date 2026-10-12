use std::{collections::HashSet, net::SocketAddr, sync::Arc, time::Duration};

use sqlx::PgPool;
use vault_broker::VaultClient;

use crate::identity::OidcClient;

#[derive(Clone)]
pub struct AppState {
    pub bind_address: SocketAddr,
    pub control_pool: PgPool,
    pub oidc: Arc<OidcClient>,
    pub oidc_client_id: Arc<str>,
    pub oidc_access_token_audience: Arc<str>,
    pub bootstrap_admin_subjects: Arc<HashSet<String>>,
    pub vault_admin: Arc<VaultClient>,
    pub vault_runtime: Arc<VaultClient>,
    pub max_query_rows: usize,
    pub max_query_duration: Duration,
}
