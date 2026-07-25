use domain::{PandaError, PandaResult};
use sqlx::{Sqlite, SqlitePool};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared pool + exclusive write lock for SQLite.
#[derive(Clone)]
pub struct Db {
    pub pool: SqlitePool,
    write: Arc<Mutex<()>>,
}

pub struct WritePermit<'a> {
    _guard: tokio::sync::MutexGuard<'a, ()>,
}

impl Db {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            write: Arc::new(Mutex::new(())),
        }
    }

    pub async fn write(&self) -> WritePermit<'_> {
        WritePermit {
            _guard: self.write.lock().await,
        }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn migrate(&self) -> PandaResult<()> {
        // Embed migration SQL (single file for v1).
        let sql = include_str!("../../../migrations/001_initial.sql");
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| PandaError::internal(format!("migrate begin: {e}")))?;

        // Split on statement boundaries carefully: FTS and PRAGMA included.
        for stmt in split_sql(sql) {
            let stmt = stmt.trim();
            if stmt.is_empty() {
                continue;
            }
            sqlx::query(stmt)
                .execute(&mut *tx)
                .await
                .map_err(|e| PandaError::internal(format!("migrate stmt failed: {e}\n{stmt}")))?;
        }

        tx.commit()
            .await
            .map_err(|e| PandaError::internal(format!("migrate commit: {e}")))?;
        Ok(())
    }
}

fn split_sql(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in sql.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("--") {
            continue;
        }
        cur.push_str(line);
        cur.push('\n');
        if trimmed.ends_with(';') {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}
