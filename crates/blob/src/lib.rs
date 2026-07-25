//! Content-addressed blob store (local filesystem).

use domain::{PandaError, PandaResult};
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Clone, Debug)]
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn ensure_root(&self) -> PandaResult<()> {
        fs::create_dir_all(&self.root)
            .await
            .map_err(|e| PandaError::internal(format!("blob root: {e}")))
    }

    pub fn path_for_hash(&self, hash: &str) -> PathBuf {
        let (a, b) = hash.split_at(std::cmp::min(2, hash.len()));
        let (c, rest) = if b.len() >= 2 { b.split_at(2) } else { (b, "") };
        self.root
            .join(a)
            .join(c)
            .join(if rest.is_empty() { hash } else { rest })
    }

    pub async fn put(&self, data: &[u8]) -> PandaResult<(String, u64)> {
        let hash = blake3::hash(data).to_hex().to_string();
        let path = self.path_for_hash(&hash);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| PandaError::internal(format!("blob mkdir: {e}")))?;
        }
        if !path.exists() {
            fs::write(&path, data)
                .await
                .map_err(|e| PandaError::internal(format!("blob write: {e}")))?;
        }
        Ok((hash, data.len() as u64))
    }

    pub async fn get(&self, hash: &str) -> PandaResult<Vec<u8>> {
        let path = self.path_for_hash(hash);
        fs::read(&path)
            .await
            .map_err(|_| PandaError::not_found(format!("blob not found: {hash}")))
    }

    pub async fn exists(&self, hash: &str) -> bool {
        self.path_for_hash(hash).exists()
    }

    pub async fn delete_if_unreferenced(&self, hash: &str) -> PandaResult<()> {
        let path = self.path_for_hash(hash);
        if path.exists() {
            let _ = fs::remove_file(&path).await;
        }
        Ok(())
    }
}
