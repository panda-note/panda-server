use serde::Deserialize;
use std::path::Path;

#[derive(Clone, Debug, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub blob: BlobConfig,
    pub auth: AuthConfig,
    pub sync: SyncConfig,
    pub limits: LimitsConfig,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerConfig {
    pub bind: String,
    pub protocol_version: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BlobConfig {
    pub root: String,
    pub inline_threshold_bytes: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AuthConfig {
    pub bootstrap_username: String,
    pub bootstrap_password: String,
    pub session_ttl_days: i64,
    pub argon2_memory_kib: u32,
    pub argon2_iterations: u32,
    pub argon2_parallelism: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SyncConfig {
    pub device_cursor_ttl_days: i64,
    pub revision_snapshot_interval_secs: i64,
    pub compaction_interval_secs: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LimitsConfig {
    pub max_request_body_bytes: usize,
    pub max_batch_size: usize,
    pub max_memos_per_workspace: u64,
    pub max_blob_bytes_per_workspace: u64,
}

impl AppConfig {
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path.as_ref()).or_else(|_| {
            Ok::<String, std::io::Error>(include_str!("../../../config/default.yml").to_string())
        })?;
        Ok(serde_yaml::from_str(&text)?)
    }
}
