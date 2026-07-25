//! Lightweight process metrics (no Prometheus dependency required).

use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Default)]
pub struct Metrics {
    pub http_requests: AtomicU64,
    pub memo_saves: AtomicU64,
    pub sync_pulls: AtomicU64,
    pub conflicts: AtomicU64,
    latencies_ms: Mutex<Vec<u64>>,
}

impl Metrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn inc_http(&self) {
        self.http_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_saves(&self) {
        self.memo_saves.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_pulls(&self) {
        self.sync_pulls.fetch_add(1, Ordering::Relaxed);
    }

    pub fn observe_ms(&self, ms: u64) {
        let mut v = self.latencies_ms.lock();
        if v.len() < 1024 {
            v.push(ms);
        }
    }

    pub fn render_prometheus(&self) -> String {
        let req = self.http_requests.load(Ordering::Relaxed);
        let saves = self.memo_saves.load(Ordering::Relaxed);
        let pulls = self.sync_pulls.load(Ordering::Relaxed);
        let conflicts = self.conflicts.load(Ordering::Relaxed);
        format!(
            "# HELP panda_http_requests_total Total HTTP requests\n\
             # TYPE panda_http_requests_total counter\n\
             panda_http_requests_total {req}\n\
             # HELP panda_memo_saves_total Memo save ACKs\n\
             # TYPE panda_memo_saves_total counter\n\
             panda_memo_saves_total {saves}\n\
             # HELP panda_sync_pulls_total Sync pull calls\n\
             # TYPE panda_sync_pulls_total counter\n\
             panda_sync_pulls_total {pulls}\n\
             # HELP panda_conflicts_total Conflict responses\n\
             # TYPE panda_conflicts_total counter\n\
             panda_conflicts_total {conflicts}\n"
        )
    }
}
