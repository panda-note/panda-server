//! Postgres provider hooks (feature `postgres`).
//!
//! Default builds use SQLite. Enabling `--features postgres` pulls in `sqlx/postgres`
//! and exposes [`provider_from_url`]. A full Postgres pool + LISTEN/NOTIFY `NotifyBus`
//! adapter can replace `Store::connect` without changing domain APIs.

/// Documented provider ids for config validation.
pub fn provider_from_url(url: &str) -> &'static str {
    if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        "postgres"
    } else {
        "sqlite"
    }
}

#[cfg(feature = "postgres")]
pub mod pg {
    //! Placeholder for `sqlx::PgPool` wiring and dialect helpers.
    pub const DIALECT: &str = "postgres";
}
