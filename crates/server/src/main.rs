mod config;
mod error;
mod export;
mod extract;
mod metrics;
mod routes;
mod state;

use clap::Parser;
use config::AppConfig;
use state::AppState;
use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "panda", about = "Panda notes server")]
struct Cli {
    /// Path to YAML config (defaults to config/default.yml)
    #[arg(long, env = "PANDA_CONFIG", default_value = "config/default.yml")]
    config: String,

    /// Override bind address
    #[arg(long, env = "PANDA_BIND")]
    bind: Option<String>,

    /// Override database URL
    #[arg(long, env = "PANDA_DATABASE_URL")]
    database_url: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info,sqlx=warn")),
        )
        .init();

    let cli = Cli::parse();
    let mut cfg = AppConfig::load(&cli.config)?;
    if let Some(bind) = cli.bind {
        cfg.server.bind = bind;
    }
    if let Some(url) = cli.database_url {
        cfg.database.url = url;
    }

    let state = AppState::bootstrap(cfg.clone()).await?;
    let app = routes::router(state);

    let addr: SocketAddr = cfg
        .server
        .bind
        .parse()
        .map_err(|e| anyhow::anyhow!("bad bind {}: {e}", cfg.server.bind))?;
    tracing::info!(%addr, "panda listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
