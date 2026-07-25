use crate::{now_rfc3339, Store};
use domain::{new_id, PandaError, PandaResult};
use proto::Resource;
use sqlx::FromRow;

pub struct ResourceRepo<'a> {
    pub store: &'a Store,
}

#[derive(FromRow)]
struct ResourceRow {
    id: String,
    memo_id: Option<String>,
    content_hash: String,
    kind: String,
    mime_type: Option<String>,
    filename: Option<String>,
    byte_size: i64,
    created_at: String,
}

impl ResourceRepo<'_> {
    pub async fn find_by_hash(
        &self,
        workspace_id: &str,
        hash: &str,
    ) -> PandaResult<Option<Resource>> {
        let row: Option<ResourceRow> = sqlx::query_as(
            "SELECT id, memo_id, content_hash, kind, mime_type, filename, byte_size, created_at
             FROM resources WHERE workspace_id = ? AND content_hash = ? AND is_deleted = 0 LIMIT 1",
        )
        .bind(workspace_id)
        .bind(hash)
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(row.map(|r| self.to_proto(r)))
    }

    fn to_proto(&self, r: ResourceRow) -> Resource {
        Resource {
            id: r.id.clone(),
            memo_id: r.memo_id,
            content_hash: r.content_hash.clone(),
            kind: r.kind,
            mime_type: r.mime_type,
            filename: r.filename,
            byte_size: r.byte_size as u64,
            url: format!("/api/v1/resources/{}/blob", r.id),
            created_at: r.created_at,
        }
    }

    pub async fn create(
        &self,
        workspace_id: &str,
        memo_id: Option<&str>,
        hash: &str,
        kind: &str,
        mime_type: Option<&str>,
        filename: Option<&str>,
        byte_size: u64,
    ) -> PandaResult<Resource> {
        if let Some(memo_id) = memo_id {
            let exists = self
                .store
                .memos()
                .get_summary(workspace_id, memo_id)
                .await?
                .is_some();
            if !exists {
                return Err(PandaError::not_found("memo not found in workspace"));
            }
        }
        // Dedup by hash
        if let Some(existing) = self.find_by_hash(workspace_id, hash).await? {
            return Ok(existing);
        }

        let _w = self.store.db.write().await;
        let now = now_rfc3339();
        sqlx::query(
            "INSERT INTO content_blobs (hash, workspace_id, byte_size, storage_key, refcount, created_at)
             VALUES (?, ?, ?, ?, 1, ?)
             ON CONFLICT(hash) DO UPDATE SET refcount = refcount + 1",
        )
        .bind(hash)
        .bind(workspace_id)
        .bind(byte_size as i64)
        .bind(hash)
        .bind(&now)
        .execute(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        let id = new_id();
        sqlx::query(
            "INSERT INTO resources (id, workspace_id, memo_id, content_hash, kind, mime_type, filename, byte_size, is_deleted, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(&id)
        .bind(workspace_id)
        .bind(memo_id)
        .bind(hash)
        .bind(kind)
        .bind(mime_type)
        .bind(filename)
        .bind(byte_size as i64)
        .bind(&now)
        .bind(&now)
        .execute(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        Ok(Resource {
            id: id.clone(),
            memo_id: memo_id.map(|s| s.to_string()),
            content_hash: hash.to_string(),
            kind: kind.to_string(),
            mime_type: mime_type.map(|s| s.to_string()),
            filename: filename.map(|s| s.to_string()),
            byte_size,
            url: format!("/api/v1/resources/{id}/blob"),
            created_at: now,
        })
    }

    pub async fn get(&self, workspace_id: &str, id: &str) -> PandaResult<Option<Resource>> {
        let row: Option<ResourceRow> = sqlx::query_as(
            "SELECT id, memo_id, content_hash, kind, mime_type, filename, byte_size, created_at
             FROM resources WHERE workspace_id = ? AND id = ? AND is_deleted = 0",
        )
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(row.map(|r| self.to_proto(r)))
    }

    pub async fn list(&self, workspace_id: &str, limit: i64) -> PandaResult<Vec<Resource>> {
        let rows: Vec<ResourceRow> = sqlx::query_as(
            "SELECT id, memo_id, content_hash, kind, mime_type, filename, byte_size, created_at
             FROM resources WHERE workspace_id = ? AND is_deleted = 0 ORDER BY created_at DESC LIMIT ?",
        )
        .bind(workspace_id)
        .bind(limit.clamp(1, 500))
        .fetch_all(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(rows.into_iter().map(|r| self.to_proto(r)).collect())
    }
}
