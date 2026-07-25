//! Sync fanout bus + helpers.

use async_trait::async_trait;
use parking_lot::Mutex;
use proto::SyncHint;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;

#[async_trait]
pub trait NotifyBus: Send + Sync {
    async fn subscribe(&self, workspace_id: &str) -> broadcast::Receiver<SyncHint>;
    async fn publish(&self, workspace_id: &str, hint: SyncHint);
}

/// In-process workspace fanout (SQLite single-node default).
#[derive(Clone, Default)]
pub struct LocalNotifyBus {
    inner: Arc<Mutex<HashMap<String, broadcast::Sender<SyncHint>>>>,
}

impl LocalNotifyBus {
    pub fn new() -> Self {
        Self::default()
    }

    fn sender(&self, workspace_id: &str) -> broadcast::Sender<SyncHint> {
        let mut map = self.inner.lock();
        map.entry(workspace_id.to_string())
            .or_insert_with(|| broadcast::channel(256).0)
            .clone()
    }
}

#[async_trait]
impl NotifyBus for LocalNotifyBus {
    async fn subscribe(&self, workspace_id: &str) -> broadcast::Receiver<SyncHint> {
        self.sender(workspace_id).subscribe()
    }

    async fn publish(&self, workspace_id: &str, hint: SyncHint) {
        let tx = self.sender(workspace_id);
        let _ = tx.send(hint);
    }
}

pub async fn hint_after_change(
    bus: &dyn NotifyBus,
    store: &store::Store,
    workspace_id: &str,
    kinds: Vec<String>,
) {
    let cursor = store.sync().max_cursor(workspace_id).await.unwrap_or(0);
    let sync_epoch = store.sync().epoch(workspace_id).await.unwrap_or(1);
    bus.publish(
        workspace_id,
        SyncHint {
            cursor,
            sync_epoch,
            kinds,
        },
    )
    .await;
}
