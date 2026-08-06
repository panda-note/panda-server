use crate::{now_rfc3339, Store};
use domain::{PandaError, PandaResult};
use proto::{SyncChange, SyncInventoryResponse, PROTOCOL_VERSION};
use sqlx::FromRow;
use std::collections::HashMap;

pub struct SyncRepo<'a> {
    pub store: &'a Store,
}

impl SyncRepo<'_> {
    pub async fn append(
        &self,
        workspace_id: &str,
        entity_type: &str,
        entity_id: &str,
        operation: &str,
        payload_kind: &str,
        payload_json: Option<&str>,
        fold_key: &str,
    ) -> PandaResult<u64> {
        let _w = self.store.db.write().await;
        self.append_unlocked(
            workspace_id,
            entity_type,
            entity_id,
            operation,
            payload_kind,
            payload_json,
            fold_key,
        )
        .await
    }

    /// Caller must hold the write lock (or accept concurrent writers via SQLite busy).
    pub async fn append_unlocked(
        &self,
        workspace_id: &str,
        entity_type: &str,
        entity_id: &str,
        operation: &str,
        payload_kind: &str,
        payload_json: Option<&str>,
        fold_key: &str,
    ) -> PandaResult<u64> {
        let now = now_rfc3339();
        let res = sqlx::query(
            "INSERT INTO sync_changes (workspace_id, entity_type, entity_id, operation, payload_kind, payload_json, fold_key, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(workspace_id)
        .bind(entity_type)
        .bind(entity_id)
        .bind(operation)
        .bind(payload_kind)
        .bind(payload_json)
        .bind(fold_key)
        .bind(&now)
        .execute(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        Ok(res.last_insert_rowid() as u64)
    }

    pub async fn epoch(&self, workspace_id: &str) -> PandaResult<u64> {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT sync_epoch FROM sync_meta WHERE workspace_id = ?")
                .bind(workspace_id)
                .fetch_optional(self.store.db.pool())
                .await
                .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(row.map(|r| r.0 as u64).unwrap_or(1))
    }

    pub async fn max_cursor(&self, workspace_id: &str) -> PandaResult<u64> {
        let row: (Option<i64>,) =
            sqlx::query_as("SELECT MAX(id) FROM sync_changes WHERE workspace_id = ?")
                .bind(workspace_id)
                .fetch_one(self.store.db.pool())
                .await
                .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(row.0.unwrap_or(0) as u64)
    }

    pub async fn pull(
        &self,
        workspace_id: &str,
        after: u64,
        limit: i64,
    ) -> PandaResult<(Vec<SyncChange>, u64, bool)> {
        let limit = limit.clamp(1, 500);
        let rows: Vec<ChangeRow> = sqlx::query_as(
            "SELECT id, entity_type, entity_id, operation, payload_kind, payload_json, fold_key, created_at
             FROM sync_changes WHERE workspace_id = ? AND id > ? ORDER BY id ASC LIMIT ?",
        )
        .bind(workspace_id)
        .bind(after as i64)
        .bind(limit + 1)
        .fetch_all(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        let mut has_more = false;
        let mut rows = rows;
        if rows.len() as i64 > limit {
            has_more = true;
            rows.pop();
        }
        let cursor = rows.last().map(|r| r.id as u64).unwrap_or(after);
        // Fold only the page being returned. Keeping the append-only log above
        // the cursor is important: deleting an older row at write time could
        // make a client that was offline skip the entity entirely.
        let mut latest = HashMap::new();
        for row in rows {
            latest.insert(row.fold_key.clone(), row);
        }
        let mut rows: Vec<_> = latest.into_values().collect();
        rows.sort_by_key(|row| row.id);
        let changes = rows
            .into_iter()
            .map(|r| SyncChange {
                id: r.id as u64,
                entity_type: r.entity_type,
                entity_id: r.entity_id,
                operation: r.operation,
                payload_kind: r.payload_kind,
                payload_json: r.payload_json,
                created_at: r.created_at,
            })
            .collect();
        Ok((changes, cursor, has_more))
    }

    pub async fn inventory(
        &self,
        workspace_id: &str,
        memo_after: Option<&str>,
        limit: i64,
    ) -> PandaResult<SyncInventoryResponse> {
        let notebooks = self.store.notebooks().list(workspace_id).await?;
        let (memos, has_more, next) = self
            .store
            .memos()
            .inventory_page(workspace_id, memo_after, limit)
            .await?;
        let todos = self
            .store
            .todos()
            .inventory(workspace_id, 2_000)
            .await?
            .iter()
            .map(|todo| todo.to_proto())
            .collect();
        let sync_epoch = self.epoch(workspace_id).await?;
        let cursor = self.max_cursor(workspace_id).await?;
        Ok(SyncInventoryResponse {
            protocol_version: PROTOCOL_VERSION,
            sync_epoch,
            cursor,
            notebooks,
            memos,
            has_more,
            next_memo_cursor: next,
            todos,
        })
    }

    pub async fn upsert_device_cursor(
        &self,
        workspace_id: &str,
        device_id: &str,
        user_id: &str,
        cursor: u64,
    ) -> PandaResult<()> {
        let now = now_rfc3339();
        let epoch = self.epoch(workspace_id).await?;
        sqlx::query(
            "INSERT INTO device_cursors (workspace_id, device_id, user_id, cursor, sync_epoch, last_seen_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(workspace_id, device_id) DO UPDATE SET
               cursor = excluded.cursor,
               sync_epoch = excluded.sync_epoch,
               last_seen_at = excluded.last_seen_at,
               user_id = excluded.user_id",
        )
        .bind(workspace_id)
        .bind(device_id)
        .bind(user_id)
        .bind(cursor as i64)
        .bind(epoch as i64)
        .bind(&now)
        .bind(&now)
        .execute(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(())
    }

    pub async fn compact(&self, workspace_id: &str, device_ttl_days: i64) -> PandaResult<u64> {
        let _w = self.store.db.write().await;
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(device_ttl_days))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        sqlx::query("DELETE FROM device_cursors WHERE workspace_id = ? AND last_seen_at < ?")
            .bind(workspace_id)
            .bind(&cutoff)
            .execute(self.store.db.pool())
            .await
            .ok();

        let min_cursor: (Option<i64>,) =
            sqlx::query_as("SELECT MIN(cursor) FROM device_cursors WHERE workspace_id = ?")
                .bind(workspace_id)
                .fetch_one(self.store.db.pool())
                .await
                .map_err(|e| PandaError::internal(e.to_string()))?;

        let watermark = min_cursor.0.unwrap_or(0);
        if watermark <= 0 {
            return Ok(0);
        }

        let res = sqlx::query("DELETE FROM sync_changes WHERE workspace_id = ? AND id < ?")
            .bind(workspace_id)
            .bind(watermark)
            .execute(self.store.db.pool())
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;

        let deleted = res.rows_affected();
        if deleted > 0 {
            let now = now_rfc3339();
            sqlx::query(
                "UPDATE sync_meta SET sync_epoch = sync_epoch + 1, updated_at = ? WHERE workspace_id = ?",
            )
            .bind(&now)
            .bind(workspace_id)
            .execute(self.store.db.pool())
            .await
            .ok();
        }
        Ok(deleted)
    }
}

#[derive(FromRow)]
struct ChangeRow {
    id: i64,
    entity_type: String,
    entity_id: String,
    operation: String,
    payload_kind: String,
    payload_json: Option<String>,
    fold_key: String,
    created_at: String,
}
