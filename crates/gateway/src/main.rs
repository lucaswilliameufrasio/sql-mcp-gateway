mod api_error;
mod audit;
mod auth;
mod cli;
mod config;
mod domain;
mod handlers;
mod identity;
mod mcp;
mod mcp_transport;
mod metrics;
mod oauth;
mod sql_access;
mod state;
mod stdio;
mod stdio_auth;
mod workspace;
mod workspace_store;

use std::{process::ExitCode, sync::Arc, time::Duration};

use axum::{
    Router, middleware,
    routing::{get, post},
};
use clap::Parser;
use config::{ConfigError, Settings};
use sqlx::postgres::PgPoolOptions;
use state::AppState;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;
use vault_broker::VaultClient;

#[derive(Debug, thiserror::Error)]
enum StartupError {
    #[error(transparent)]
    Configuration(#[from] ConfigError),
    #[error("failed to discover the configured OIDC provider")]
    OidcDiscovery,
    #[error("failed to construct a Vault client")]
    VaultConfiguration,
    #[error("failed to connect to the control database")]
    DatabaseConnection,
    #[error("failed to apply database migrations")]
    DatabaseMigration,
    #[error("failed to bind the HTTP listener")]
    ListenerBind,
    #[error("the HTTP server stopped unexpectedly")]
    Server,
    #[error(transparent)]
    Client(#[from] cli::CliError),
}

#[tokio::main]
async fn main() -> ExitCode {
    let _ = dotenvy::dotenv();
    let command = cli::Cli::parse().into_command();
    initialize_tracing();

    match run(command).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(StartupError::Client(cli::CliError::Cancelled)) => ExitCode::from(130),
        Err(error) => {
            eprintln!("error: {error}");
            if matches!(error, StartupError::Configuration(_)) {
                eprintln!(
                    "This starts the gateway server. To use a gateway instead, run `sql-mcp-gateway login`; see `sql-mcp-gateway --help`."
                );
            }
            ExitCode::FAILURE
        }
    }
}

async fn run(command: cli::Command) -> Result<(), StartupError> {
    match command {
        cli::Command::Login(workspace) => Ok(cli::login(workspace).await?),
        cli::Command::Logout(workspace) => Ok(cli::logout(workspace).await?),
        cli::Command::Workspaces => Ok(cli::workspaces().await?),
        cli::Command::Remove(workspace) => Ok(cli::remove(workspace).await?),
        cli::Command::Stdio(workspace) => Ok(cli::stdio(workspace).await?),
        cli::Command::Serve => {
            let settings = Settings::from_environment()?;
            let state = create_state(settings).await?;
            serve_http(state).await
        }
    }
}

async fn create_state(settings: Settings) -> Result<AppState, StartupError> {
    let control_pool = PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(Duration::from_secs(5))
        .connect(settings.control_database_url.as_str())
        .await
        .map_err(|_| StartupError::DatabaseConnection)?;

    sqlx::migrate!("./migrations")
        .run(&control_pool)
        .await
        .map_err(|_| StartupError::DatabaseMigration)?;

    let oidc = identity::OidcClient::discover(
        settings.oidc_issuer.clone(),
        settings.oidc_access_token_audience.clone(),
    )
    .await
    .map_err(|_| StartupError::OidcDiscovery)?;
    let vault_admin = VaultClient::new(settings.vault_address.clone(), settings.vault_admin_token)
        .map_err(|_| StartupError::VaultConfiguration)?;
    let vault_runtime = VaultClient::new(settings.vault_address, settings.vault_runtime_token)
        .map_err(|_| StartupError::VaultConfiguration)?;

    Ok(AppState {
        bind_address: settings.bind_address,
        control_pool,
        oidc: Arc::new(oidc),
        oidc_client_id: Arc::from(settings.oidc_client_id),
        oidc_access_token_audience: Arc::from(settings.oidc_access_token_audience),
        mcp_public_url: Arc::new(settings.mcp_public_url),
        mcp_oauth_state_key: Arc::new(settings.mcp_oauth_state_key),
        bootstrap_admin_subjects: Arc::new(settings.bootstrap_admin_subjects),
        vault_admin: Arc::new(vault_admin),
        vault_runtime: Arc::new(vault_runtime),
        max_query_rows: settings.max_query_rows,
        max_query_duration: settings.max_query_duration,
        workspaces: Arc::new(settings.workspaces),
    })
}

async fn serve_http(state: AppState) -> Result<(), StartupError> {
    let app = app_router(state.clone());
    let listener = TcpListener::bind(state.bind_address)
        .await
        .map_err(|_| StartupError::ListenerBind)?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|_| StartupError::Server)
}

fn app_router(state: AppState) -> Router {
    let protected_routes = Router::new()
        .route("/v1/me", get(handlers::me))
        .route("/v1/metrics", get(metrics::get_metrics))
        .route("/v1/databases", get(handlers::list_databases))
        .route("/v1/admin/principals", get(handlers::list_principals))
        .route(
            "/v1/admin/principals/{principal_id}/database-grants",
            get(handlers::list_principal_grants),
        )
        .route(
            "/v1/admin/principals/{principal_id}/database-grants/{database_id}",
            axum::routing::put(handlers::set_grant).delete(handlers::delete_grant),
        )
        .route(
            "/v1/admin/database-targets",
            get(handlers::list_targets_admin).post(handlers::register_target),
        )
        .route(
            "/v1/admin/database-targets/{database_id}/enabled",
            axum::routing::put(handlers::set_target_enabled),
        )
        .route("/v1/admin/audit-events", get(handlers::list_audit_events))
        .route(
            "/mcp",
            post(mcp_transport::post_message)
                .get(mcp_transport::listen)
                .delete(mcp_transport::terminate),
        )
        .layer(axum::extract::DefaultBodyLimit::max(256 * 1024))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_identity,
        ));

    Router::new()
        .route("/health", get(health))
        .route("/readyz", get(readiness))
        .route("/v1/auth/config", get(handlers::auth_config))
        .merge(protected_routes)
        .merge(oauth::routes())
        .with_state(state)
}

fn initialize_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}

async fn health() -> axum::http::StatusCode {
    axum::http::StatusCode::NO_CONTENT
}

async fn readiness(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> axum::http::StatusCode {
    match sqlx::query("SELECT 1").execute(&state.control_pool).await {
        Ok(_) => axum::http::StatusCode::NO_CONTENT,
        Err(_) => axum::http::StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
