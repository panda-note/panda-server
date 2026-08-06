use crate::{now_rfc3339, Store};
use domain::{format_etag, new_id, PandaError, PandaResult};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Deserialize)]
pub struct TodoListQuery {
    pub filter: Option<String>,
    pub limit: i64,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Todo {
    pub id: String,
    pub title: String,
    pub note: String,
    pub status: String,
    pub due_date: Option<String>,
    pub priority: i64,
    pub linked_memo_id: Option<String>,
    pub is_deleted: bool,
    pub revision: i64,
    pub etag: String,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TodoCreate {
    #[serde(default)]
    pub id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub linked_memo_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TodoUpdate {
    pub title: Option<String>,
    pub note: Option<String>,
    pub status: Option<String>,
    pub due_date: Option<Option<String>>,
    pub priority: Option<i64>,
    pub linked_memo_id: Option<Option<String>>,
    pub base_revision: Option<i64>,
    pub if_match_etag: Option<String>,
}

#[derive(Debug, FromRow)]
struct TodoRow {
    id: String,
    title: String,
    note: String,
    status: String,
    due_date: Option<String>,
    priority: i64,
    linked_memo_id: Option<String>,
    is_deleted: i64,
    revision: i64,
    etag: String,
    created_at: String,
    updated_at: String,
    completed_at: Option<String>,
    deleted_at: Option<String>,
}
impl From<TodoRow> for Todo {
    fn from(v: TodoRow) -> Self {
        Self {
            id: v.id,
            title: v.title,
            note: v.note,
            status: v.status,
            due_date: v.due_date,
            priority: v.priority,
            linked_memo_id: v.linked_memo_id,
            is_deleted: v.is_deleted != 0,
            revision: v.revision,
            etag: v.etag,
            created_at: v.created_at,
            updated_at: v.updated_at,
            completed_at: v.completed_at,
            deleted_at: v.deleted_at,
        }
    }
}

impl Todo {
    pub fn to_proto(&self) -> proto::Todo {
        proto::Todo {
            id: self.id.clone(),
            title: self.title.clone(),
            note: self.note.clone(),
            status: self.status.clone(),
            due_date: self.due_date.clone(),
            priority: self.priority as i32,
            linked_memo_id: self.linked_memo_id.clone(),
            is_deleted: self.is_deleted,
            revision: self.revision as u64,
            etag: self.etag.clone(),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
            completed_at: self.completed_at.clone(),
            deleted_at: self.deleted_at.clone(),
        }
    }
}

pub struct TodoRepo<'a> {
    pub store: &'a Store,
}
impl TodoRepo<'_> {
    fn validate(status: &str, priority: i64) -> PandaResult<()> {
        if !matches!(status, "inbox" | "open" | "completed") {
            return Err(PandaError::invalid("invalid todo status"));
        }
        if !(0..=3).contains(&priority) {
            return Err(PandaError::invalid("priority must be between 0 and 3"));
        }
        Ok(())
    }
    async fn require_linked_memo(
        &self,
        workspace_id: &str,
        linked_memo_id: Option<&str>,
    ) -> PandaResult<()> {
        if let Some(memo_id) = linked_memo_id {
            let exists = self
                .store
                .memos()
                .get_summary(workspace_id, memo_id)
                .await?
                .is_some();
            if !exists {
                return Err(PandaError::not_found("linked memo not found in workspace"));
            }
        }
        Ok(())
    }
    async fn append_unlocked(
        &self,
        workspace_id: &str,
        todo: &Todo,
        operation: &str,
    ) -> PandaResult<()> {
        let payload =
            serde_json::to_string(todo).map_err(|e| PandaError::internal(e.to_string()))?;
        self.store
            .sync()
            .append_unlocked(
                workspace_id,
                "todo",
                &todo.id,
                operation,
                "json",
                Some(&payload),
                &format!("todo:{}", todo.id),
            )
            .await?;
        Ok(())
    }
    pub async fn list(
        &self,
        workspace_id: &str,
        q: TodoListQuery,
    ) -> PandaResult<(Vec<Todo>, Option<String>)> {
        let limit = q.limit.clamp(1, 200);
        let filter = q.filter.unwrap_or_else(|| "inbox".into());
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let (deleted_sql, where_sql, bind_date) = match filter.as_str() {
            "trash" => ("is_deleted = 1", "1=1", None),
            "completed" => ("is_deleted = 0", "status = 'completed'", None),
            "today" => (
                "is_deleted = 0",
                "status != 'completed' AND due_date = ?",
                Some(today),
            ),
            "upcoming" => (
                "is_deleted = 0",
                "status != 'completed' AND due_date > ?",
                Some(today),
            ),
            "all" => ("is_deleted = 0", "1=1", None),
            _ => ("is_deleted = 0", "status = 'inbox'", None),
        };
        let mut sql=format!("SELECT id,title,note,status,due_date,priority,linked_memo_id,is_deleted,revision,etag,created_at,updated_at,completed_at,deleted_at FROM todos WHERE workspace_id = ? AND {deleted_sql} AND {where_sql}");
        if q.cursor.is_some() {
            sql.push_str(" AND (updated_at, id) < (?, ?)");
        }
        sql.push_str(" ORDER BY CASE WHEN due_date IS NULL THEN 1 ELSE 0 END, due_date ASC, priority DESC, updated_at DESC, id DESC LIMIT ?");
        let mut query = sqlx::query_as::<_, TodoRow>(&sql).bind(workspace_id);
        if let Some(date) = bind_date {
            query = query.bind(date);
        }
        if let Some(c) = q.cursor {
            let (at, id) = c.split_once('|').unwrap_or((&c, ""));
            query = query.bind(at.to_string()).bind(id.to_string());
        }
        let mut rows = query
            .bind(limit + 1)
            .fetch_all(self.store.db.pool())
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;
        let next = if rows.len() as i64 > limit {
            let last = rows.pop().unwrap();
            Some(format!("{}|{}", last.updated_at, last.id))
        } else {
            None
        };
        Ok((rows.into_iter().map(Into::into).collect(), next))
    }
    pub async fn get(&self, workspace_id: &str, id: &str) -> PandaResult<Todo> {
        sqlx::query_as::<_,TodoRow>("SELECT id,title,note,status,due_date,priority,linked_memo_id,is_deleted,revision,etag,created_at,updated_at,completed_at,deleted_at FROM todos WHERE workspace_id=? AND id=? AND is_deleted=0").bind(workspace_id).bind(id).fetch_optional(self.store.db.pool()).await.map_err(|e| PandaError::internal(e.to_string()))?.map(Into::into).ok_or_else(|| PandaError::not_found("todo not found"))
    }
    async fn get_including_deleted(&self, workspace_id: &str, id: &str) -> PandaResult<Todo> {
        sqlx::query_as::<_,TodoRow>("SELECT id,title,note,status,due_date,priority,linked_memo_id,is_deleted,revision,etag,created_at,updated_at,completed_at,deleted_at FROM todos WHERE workspace_id=? AND id=?").bind(workspace_id).bind(id).fetch_optional(self.store.db.pool()).await.map_err(|e| PandaError::internal(e.to_string()))?.map(Into::into).ok_or_else(|| PandaError::not_found("todo not found"))
    }
    pub async fn inventory(&self, workspace_id: &str, limit: i64) -> PandaResult<Vec<Todo>> {
        let rows = sqlx::query_as::<_, TodoRow>("SELECT id,title,note,status,due_date,priority,linked_memo_id,is_deleted,revision,etag,created_at,updated_at,completed_at,deleted_at FROM todos WHERE workspace_id=? ORDER BY updated_at DESC, id DESC LIMIT ?")
            .bind(workspace_id).bind(limit.clamp(1, 2_000)).fetch_all(self.store.db.pool()).await
            .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }
    pub async fn create(&self, workspace_id: &str, req: TodoCreate) -> PandaResult<Todo> {
        if req.title.trim().is_empty() {
            return Err(PandaError::invalid("todo title is required"));
        }
        let status = req.status.unwrap_or_else(|| "inbox".into());
        let priority = req.priority.unwrap_or(0);
        Self::validate(&status, priority)?;
        self.require_linked_memo(workspace_id, req.linked_memo_id.as_deref())
            .await?;
        let id = req.id.unwrap_or_else(new_id);
        let now = now_rfc3339();
        let etag = format_etag(1, &id);
        let completed = if status == "completed" {
            Some(now.clone())
        } else {
            None
        };
        let _w = self.store.db.write().await;
        // A sync retry reuses the client-generated id. Return the existing
        // record instead of creating a second task.
        if let Some(existing) = sqlx::query_as::<_, TodoRow>(
            "SELECT id,title,note,status,due_date,priority,linked_memo_id,is_deleted,revision,etag,created_at,updated_at,completed_at,deleted_at
             FROM todos WHERE workspace_id=? AND id=?",
        )
        .bind(workspace_id)
        .bind(&id)
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?
        {
            return Ok(existing.into());
        }
        sqlx::query("INSERT INTO todos(id,workspace_id,title,note,status,due_date,priority,linked_memo_id,is_deleted,revision,etag,created_at,updated_at,completed_at) VALUES(?,?,?,?,?,?,?,?,0,1,?,?,?,?)").bind(&id).bind(workspace_id).bind(req.title.trim()).bind(req.note).bind(&status).bind(req.due_date).bind(priority).bind(req.linked_memo_id).bind(&etag).bind(&now).bind(&now).bind(completed).execute(self.store.db.pool()).await.map_err(|e| PandaError::internal(e.to_string()))?;
        let todo = self.get(workspace_id, &id).await?;
        self.append_unlocked(workspace_id, &todo, "upsert").await?;
        Ok(todo)
    }
    pub async fn update(&self, workspace_id: &str, id: &str, req: TodoUpdate) -> PandaResult<Todo> {
        let _w = self.store.db.write().await;
        let old = self.get(workspace_id, id).await?;
        if let Some(rev) = req.base_revision {
            if rev != old.revision {
                return Err(PandaError::new(
                    domain::ErrorCode::Conflict,
                    "todo revision conflict",
                ));
            }
        }
        if let Some(etag) = req.if_match_etag.as_deref() {
            if etag != old.etag {
                return Err(PandaError::new(
                    domain::ErrorCode::Conflict,
                    "todo etag conflict",
                ));
            }
        }
        let title = req.title.unwrap_or(old.title);
        if title.trim().is_empty() {
            return Err(PandaError::invalid("todo title is required"));
        }
        let note = req.note.unwrap_or(old.note);
        let status = req.status.unwrap_or(old.status);
        let priority = req.priority.unwrap_or(old.priority);
        Self::validate(&status, priority)?;
        let due = req.due_date.unwrap_or(old.due_date);
        let linked = req.linked_memo_id.unwrap_or(old.linked_memo_id);
        self.require_linked_memo(workspace_id, linked.as_deref())
            .await?;
        let rev = old.revision + 1;
        let etag = format_etag(rev as u64, id);
        let now = now_rfc3339();
        let completed = if status == "completed" {
            old.completed_at.or(Some(now.clone()))
        } else {
            None
        };
        let changed=sqlx::query("UPDATE todos SET title=?,note=?,status=?,due_date=?,priority=?,linked_memo_id=?,revision=?,etag=?,updated_at=?,completed_at=? WHERE workspace_id=? AND id=? AND is_deleted=0 AND revision=? AND etag=?").bind(title.trim()).bind(note).bind(status).bind(due).bind(priority).bind(linked).bind(rev).bind(etag).bind(now).bind(completed).bind(workspace_id).bind(id).bind(old.revision).bind(&old.etag).execute(self.store.db.pool()).await.map_err(|e| PandaError::internal(e.to_string()))?;
        if changed.rows_affected() != 1 {
            return Err(PandaError::new(
                domain::ErrorCode::Conflict,
                "todo revision conflict",
            ));
        }
        let todo = self.get(workspace_id, id).await?;
        self.append_unlocked(workspace_id, &todo, "upsert").await?;
        Ok(todo)
    }
    pub async fn delete(
        &self,
        workspace_id: &str,
        id: &str,
        base_revision: Option<i64>,
        if_match_etag: Option<&str>,
        permanent: bool,
    ) -> PandaResult<()> {
        let _w = self.store.db.write().await;
        let old = if permanent {
            self.get_including_deleted(workspace_id, id).await?
        } else {
            self.get(workspace_id, id).await?
        };
        if let Some(rev) = base_revision {
            if rev != old.revision {
                return Err(PandaError::new(
                    domain::ErrorCode::Conflict,
                    "todo revision conflict",
                ));
            }
        }
        if let Some(etag) = if_match_etag {
            if etag != old.etag {
                return Err(PandaError::new(
                    domain::ErrorCode::Conflict,
                    "todo etag conflict",
                ));
            }
        }
        if permanent {
            if !old.is_deleted {
                return Err(PandaError::invalid(
                    "todo must be in trash before permanent deletion",
                ));
            }
            let changed = sqlx::query(
                "DELETE FROM todos WHERE workspace_id=? AND id=? AND is_deleted=1 AND revision=? AND etag=?",
            )
            .bind(workspace_id)
            .bind(id)
            .bind(old.revision)
            .bind(&old.etag)
            .execute(self.store.db.pool())
            .await
            .map_err(|error| PandaError::internal(error.to_string()))?;
            if changed.rows_affected() != 1 {
                return Err(PandaError::new(
                    domain::ErrorCode::Conflict,
                    "todo revision conflict",
                ));
            }
            return self.append_unlocked(workspace_id, &old, "delete").await;
        }
        let now = now_rfc3339();
        let revision = old.revision + 1;
        let etag = format_etag(revision as u64, id);
        let changed=sqlx::query("UPDATE todos SET is_deleted=1,deleted_at=?,updated_at=?,revision=?,etag=? WHERE workspace_id=? AND id=? AND is_deleted=0 AND revision=? AND etag=?").bind(&now).bind(&now).bind(revision).bind(etag).bind(workspace_id).bind(id).bind(old.revision).bind(&old.etag).execute(self.store.db.pool()).await.map_err(|e| PandaError::internal(e.to_string()))?;
        if changed.rows_affected() != 1 {
            return Err(PandaError::new(
                domain::ErrorCode::Conflict,
                "todo revision conflict",
            ));
        }
        let tombstone = self.get_including_deleted(workspace_id, id).await?;
        self.append_unlocked(workspace_id, &tombstone, "delete")
            .await
    }

    pub async fn restore(
        &self,
        workspace_id: &str,
        id: &str,
        base_revision: Option<i64>,
        if_match_etag: Option<&str>,
    ) -> PandaResult<Todo> {
        let _write_guard = self.store.db.write().await;
        let old = self.get_including_deleted(workspace_id, id).await?;
        if !old.is_deleted {
            return Err(PandaError::invalid("todo is not deleted"));
        }
        if let Some(revision) = base_revision {
            if revision != old.revision {
                return Err(PandaError::new(
                    domain::ErrorCode::Conflict,
                    "todo revision conflict",
                ));
            }
        }
        if let Some(etag) = if_match_etag {
            if etag != old.etag {
                return Err(PandaError::new(
                    domain::ErrorCode::Conflict,
                    "todo etag conflict",
                ));
            }
        }
        let revision = old.revision + 1;
        let etag = format_etag(revision as u64, id);
        let now = now_rfc3339();
        let changed = sqlx::query("UPDATE todos SET is_deleted=0,deleted_at=NULL,updated_at=?,revision=?,etag=? WHERE workspace_id=? AND id=? AND is_deleted=1 AND revision=? AND etag=?")
            .bind(&now)
            .bind(revision)
            .bind(etag)
            .bind(workspace_id)
            .bind(id)
            .bind(old.revision)
            .bind(&old.etag)
            .execute(self.store.db.pool())
            .await
            .map_err(|error| PandaError::internal(error.to_string()))?;
        if changed.rows_affected() != 1 {
            return Err(PandaError::new(
                domain::ErrorCode::Conflict,
                "todo revision conflict",
            ));
        }
        let todo = self.get(workspace_id, id).await?;
        self.append_unlocked(workspace_id, &todo, "upsert").await?;
        Ok(todo)
    }
}
