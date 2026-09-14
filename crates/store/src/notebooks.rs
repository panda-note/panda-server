use crate::{now_rfc3339, Store};
use domain::{new_id, PandaError, PandaResult};
use proto::Notebook;
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
struct NotebookRow {
    id: String,
    parent_id: Option<String>,
    name: String,
    slug: Option<String>,
    path: String,
    depth: i64,
    sort_order: i64,
    is_deleted: i64,
    created_at: String,
    updated_at: String,
    memo_count: i64,
}

pub struct NotebookRepo<'a> {
    pub store: &'a Store,
}

impl NotebookRepo<'_> {
    pub async fn list(&self, workspace_id: &str) -> PandaResult<Vec<Notebook>> {
        let rows: Vec<NotebookRow> = sqlx::query_as(
            "SELECT n.id, n.parent_id, n.name, n.slug, n.path, n.depth, n.sort_order, n.is_deleted,
                    n.created_at, n.updated_at,
                    (SELECT COUNT(*) FROM memos m WHERE m.notebook_id = n.id AND m.is_deleted = 0) AS memo_count
             FROM notebooks n
             WHERE n.workspace_id = ? AND n.is_deleted = 0
             ORDER BY n.depth, n.sort_order, n.name",
        )
        .bind(workspace_id)
        .fetch_all(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| Notebook {
                id: r.id,
                parent_id: r.parent_id,
                name: r.name,
                slug: r.slug,
                path: r.path,
                depth: r.depth as i32,
                sort_order: r.sort_order as i32,
                memo_count: r.memo_count,
                is_deleted: r.is_deleted != 0,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }

    pub async fn get(&self, workspace_id: &str, id: &str) -> PandaResult<Option<NotebookRowLite>> {
        sqlx::query_as::<_, NotebookRowLite>(
            "SELECT id, parent_id, name, path, depth, sort_order, is_deleted FROM notebooks WHERE workspace_id = ? AND id = ?",
        )
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))
    }

    pub async fn create(
        &self,
        workspace_id: &str,
        name: &str,
        parent_id: Option<&str>,
        sort_order: Option<i32>,
    ) -> PandaResult<Notebook> {
        let _w = self.store.db.write().await;
        let now = now_rfc3339();
        let id = new_id();
        let (path, depth) = if let Some(pid) = parent_id {
            let parent = self
                .get(workspace_id, pid)
                .await?
                .ok_or_else(|| PandaError::not_found("parent notebook not found"))?;
            (format!("{}/{}", parent.path, id), parent.depth + 1)
        } else {
            (format!("/{id}"), 0)
        };
        let sort_order = match sort_order {
            Some(sort_order) => sort_order,
            None => sqlx::query_scalar::<_, i32>(
                "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM notebooks WHERE workspace_id = ? AND parent_id IS ? AND is_deleted = 0",
            )
            .bind(workspace_id)
            .bind(parent_id)
            .fetch_one(self.store.db.pool())
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?,
        };

        sqlx::query(
            "INSERT INTO notebooks (id, workspace_id, parent_id, name, slug, path, depth, sort_order, is_deleted, created_at, updated_at)
             VALUES (?, ?, ?, ?, NULL, ?, ?, ?, 0, ?, ?)",
        )
        .bind(&id)
        .bind(workspace_id)
        .bind(parent_id)
        .bind(name)
        .bind(&path)
        .bind(depth)
        .bind(sort_order)
        .bind(&now)
        .bind(&now)
        .execute(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        self.store
            .sync()
            .append_unlocked(
                workspace_id,
                "notebook",
                &id,
                "upsert",
                "meta",
                Some(
                    &serde_json::json!({"id": id, "name": name, "parent_id": parent_id})
                        .to_string(),
                ),
                &format!("notebook:{id}"),
            )
            .await?;

        Ok(Notebook {
            id: id.clone(),
            parent_id: parent_id.map(|s| s.to_string()),
            name: name.to_string(),
            slug: None,
            path,
            depth: depth as i32,
            sort_order,
            memo_count: 0,
            is_deleted: false,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub async fn rename(&self, workspace_id: &str, id: &str, name: &str) -> PandaResult<Notebook> {
        let _w = self.store.db.write().await;
        let now = now_rfc3339();
        let row: Option<NotebookRow> = sqlx::query_as(
            "SELECT n.id, n.parent_id, n.name, n.slug, n.path, n.depth, n.sort_order, n.is_deleted,
                    n.created_at, n.updated_at,
                    (SELECT COUNT(*) FROM memos m WHERE m.notebook_id = n.id AND m.is_deleted = 0) AS memo_count
             FROM notebooks n
             WHERE n.workspace_id = ? AND n.id = ? AND n.is_deleted = 0",
        )
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        let row = row.ok_or_else(|| PandaError::not_found("notebook not found"))?;
        if row.slug.as_deref().is_some_and(|s| s == "inbox") {
            return Err(PandaError::invalid("cannot rename inbox"));
        }
        let name = name.trim();
        if name.is_empty() {
            return Err(PandaError::invalid("name required"));
        }
        sqlx::query(
            "UPDATE notebooks SET name = ?, updated_at = ? WHERE workspace_id = ? AND id = ? AND is_deleted = 0",
        )
        .bind(name)
        .bind(&now)
        .bind(workspace_id)
        .bind(id)
        .execute(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        self.store
            .sync()
            .append_unlocked(
                workspace_id,
                "notebook",
                id,
                "upsert",
                "meta",
                Some(&serde_json::json!({"id": id, "name": name}).to_string()),
                &format!("notebook:{id}"),
            )
            .await?;

        Ok(Notebook {
            id: row.id,
            parent_id: row.parent_id,
            name: name.to_string(),
            slug: row.slug,
            path: row.path,
            depth: row.depth as i32,
            sort_order: row.sort_order as i32,
            memo_count: row.memo_count,
            is_deleted: false,
            created_at: row.created_at,
            updated_at: now,
        })
    }

    /// Reorders a complete set of sibling notebooks. Supplying the complete set prevents a
    /// stale client from silently dropping siblings from the ordering.
    pub async fn reorder(
        &self,
        workspace_id: &str,
        parent_id: Option<&str>,
        notebook_ids: &[String],
    ) -> PandaResult<Vec<Notebook>> {
        let _w = self.store.db.write().await;
        let siblings: Vec<(String,)> = sqlx::query_as(
            "SELECT id FROM notebooks WHERE workspace_id = ? AND parent_id IS ? AND is_deleted = 0 ORDER BY sort_order, name",
        )
        .bind(workspace_id)
        .bind(parent_id)
        .fetch_all(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        let sibling_ids = siblings.into_iter().map(|(id,)| id).collect::<Vec<_>>();
        let supplied = notebook_ids
            .iter()
            .collect::<std::collections::HashSet<_>>();
        if supplied.len() != notebook_ids.len()
            || supplied.len() != sibling_ids.len()
            || !sibling_ids.iter().all(|id| supplied.contains(id))
        {
            return Err(PandaError::invalid(
                "notebook_ids must contain every sibling exactly once",
            ));
        }

        let now = now_rfc3339();
        let mut tx = self
            .store
            .db
            .pool()
            .begin()
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;
        for (sort_order, id) in notebook_ids.iter().enumerate() {
            sqlx::query(
                "UPDATE notebooks SET sort_order = ?, updated_at = ? WHERE workspace_id = ? AND id = ?",
            )
            .bind(sort_order as i32)
            .bind(&now)
            .bind(workspace_id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;

        for (sort_order, id) in notebook_ids.iter().enumerate() {
            self.store
                .sync()
                .append_unlocked(
                    workspace_id,
                    "notebook",
                    id,
                    "upsert",
                    "meta",
                    Some(
                        &serde_json::json!({
                            "id": id,
                            "parent_id": parent_id,
                            "sort_order": sort_order,
                        })
                        .to_string(),
                    ),
                    &format!("notebook:{id}"),
                )
                .await?;
        }
        self.list(workspace_id).await
    }

    pub async fn soft_delete(&self, workspace_id: &str, id: &str) -> PandaResult<()> {
        let _w = self.store.db.write().await;
        let now = now_rfc3339();
        let _nb = self
            .get(workspace_id, id)
            .await?
            .ok_or_else(|| PandaError::not_found("notebook not found"))?;
        let slug: Option<(Option<String>,)> =
            sqlx::query_as("SELECT slug FROM notebooks WHERE workspace_id = ? AND id = ?")
                .bind(workspace_id)
                .bind(id)
                .fetch_optional(self.store.db.pool())
                .await
                .map_err(|e| PandaError::internal(e.to_string()))?;
        if slug
            .and_then(|s| s.0)
            .as_deref()
            .is_some_and(|s| s == "inbox")
        {
            return Err(PandaError::invalid("cannot delete inbox"));
        }
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM memos
                 WHERE workspace_id = ? AND notebook_id = ? AND is_deleted = 0",
        )
        .bind(workspace_id)
        .bind(id)
        .fetch_one(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        if count.0 > 0 {
            return Err(PandaError::invalid("notebook not empty"));
        }
        sqlx::query(
            "UPDATE notebooks SET is_deleted = 1, updated_at = ? WHERE workspace_id = ? AND id = ?",
        )
        .bind(&now)
        .bind(workspace_id)
        .bind(id)
        .execute(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        self.store
            .sync()
            .append_unlocked(
                workspace_id,
                "notebook",
                id,
                "delete",
                "delete",
                None,
                &format!("notebook:{id}"),
            )
            .await?;
        Ok(())
    }

    pub async fn default_inbox_id(&self, workspace_id: &str) -> PandaResult<String> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM notebooks WHERE workspace_id = ? AND slug = 'inbox' AND is_deleted = 0 LIMIT 1",
        )
        .bind(workspace_id)
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        if let Some((id,)) = row {
            return Ok(id);
        }
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM notebooks WHERE workspace_id = ? AND is_deleted = 0 ORDER BY depth, sort_order LIMIT 1",
        )
        .bind(workspace_id)
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        row.map(|r| r.0)
            .ok_or_else(|| PandaError::not_found("no notebook"))
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct NotebookRowLite {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub path: String,
    pub depth: i64,
    pub sort_order: i64,
    pub is_deleted: i64,
}
