use crate::config::AppConfig;
use crate::metrics::Metrics;
use auth::AuthService;
use blob::BlobStore;
use domain::PandaResult;
use mcp::McpHandler;
use std::sync::Arc;
use store::Store;
use sync::{LocalNotifyBus, NotifyBus};

#[derive(Clone)]
pub struct AppState {
    pub cfg: AppConfig,
    pub store: Store,
    pub blobs: BlobStore,
    pub auth: AuthService,
    pub bus: Arc<dyn NotifyBus>,
    pub mcp: Arc<McpHandler>,
    pub metrics: Arc<Metrics>,
}

impl AppState {
    pub async fn bootstrap(cfg: AppConfig) -> anyhow::Result<Self> {
        let store = Store::connect(&cfg.database.url, cfg.blob.inline_threshold_bytes).await?;
        let blobs = BlobStore::new(&cfg.blob.root);
        blobs.ensure_root().await?;

        let auth = AuthService::new(
            store.clone(),
            cfg.auth.session_ttl_days,
            cfg.auth.argon2_memory_kib,
            cfg.auth.argon2_iterations,
            cfg.auth.argon2_parallelism,
        );
        auth.ensure_bootstrap(&cfg.auth.bootstrap_username, &cfg.auth.bootstrap_password)
            .await?;

        let bus: Arc<dyn NotifyBus> = Arc::new(LocalNotifyBus::new());
        let mcp = Arc::new(McpHandler {
            store: store.clone(),
            blobs: blobs.clone(),
            bus: bus.clone(),
        });
        let metrics = Metrics::new();

        let store_bg = store.clone();
        let ttl = cfg.sync.device_cursor_ttl_days;
        let compact_every = cfg.sync.compaction_interval_secs;
        tokio::spawn(async move {
            let mut tick = 0u64;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let _ = store_bg.memos().process_search_dirty(64).await;
                tick += 2;
                if compact_every > 0 && tick % compact_every == 0 {
                    if let Ok(rows) = sqlx_workspace_ids(&store_bg).await {
                        for ws in rows {
                            let _ = store_bg.sync().compact(&ws, ttl).await;
                        }
                    }
                }
            }
        });

        Ok(Self {
            cfg,
            store,
            blobs,
            auth,
            bus,
            mcp,
            metrics,
        })
    }
}

async fn sqlx_workspace_ids(store: &Store) -> PandaResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as("SELECT id FROM workspaces")
        .fetch_all(store.db.pool())
        .await
        .map_err(|e| domain::PandaError::internal(e.to_string()))?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}
