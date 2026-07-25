//! SQLite-backed store with serial write queue and domain repositories.

mod db;
mod memos;
mod notebooks;
mod postgres;
mod resources;
mod sync_repo;
mod todos;
mod users;

pub use db::{Db, WritePermit};
pub use memos::{MemoListQuery, MemoRepo};
pub use notebooks::NotebookRepo;
pub use postgres::provider_from_url;
pub use resources::ResourceRepo;
pub use sync_repo::SyncRepo;
pub use todos::{Todo, TodoCreate, TodoListQuery, TodoRepo, TodoUpdate};
pub use users::{SessionRow, UserRepo, UserRow, WorkspaceMembership};

use domain::{PandaError, PandaResult};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;
use std::time::Duration;

#[derive(Clone)]
pub struct Store {
    pub db: Db,
    pub inline_threshold: u64,
}

impl Store {
    pub async fn connect(database_url: &str, inline_threshold: u64) -> PandaResult<Self> {
        let provider = provider_from_url(database_url);
        if provider == "postgres" {
            return Err(PandaError::invalid(
                "postgres URL provided: rebuild with --features postgres and wire PgPool (scaffold in store::postgres)",
            ));
        }

        let url = if database_url.starts_with("sqlite://") {
            database_url.to_string()
        } else {
            format!("sqlite://{database_url}")
        };

        // Ensure parent dir exists for file-backed sqlite
        if let Some(path) = url.strip_prefix("sqlite://") {
            let path = path.split('?').next().unwrap_or(path);
            if path != ":memory:" && !path.is_empty() {
                if let Some(parent) = std::path::Path::new(path).parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| PandaError::internal(format!("create db dir: {e}")))?;
                }
            }
        }

        let options = SqliteConnectOptions::from_str(&url)
            .map_err(|e| PandaError::internal(format!("sqlite options: {e}")))?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await
            .map_err(|e| PandaError::internal(format!("sqlite connect: {e}")))?;

        sqlx::query("PRAGMA wal_autocheckpoint = 1000;")
            .execute(&pool)
            .await
            .ok();

        let db = Db::new(pool);
        db.migrate().await?;
        Ok(Self {
            db,
            inline_threshold,
        })
    }

    pub fn users(&self) -> UserRepo<'_> {
        UserRepo { store: self }
    }

    pub fn notebooks(&self) -> NotebookRepo<'_> {
        NotebookRepo { store: self }
    }

    pub fn memos(&self) -> MemoRepo<'_> {
        MemoRepo { store: self }
    }

    pub fn sync(&self) -> SyncRepo<'_> {
        SyncRepo { store: self }
    }

    pub fn todos(&self) -> TodoRepo<'_> {
        TodoRepo { store: self }
    }

    pub fn resources(&self) -> ResourceRepo<'_> {
        ResourceRepo { store: self }
    }
}

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
