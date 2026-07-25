use crate::{now_rfc3339, Store};
use domain::{new_id, PandaError, PandaResult};
use sqlx::FromRow;

#[derive(Debug, Clone, serde::Serialize, FromRow)]
pub struct WorkspaceMembership {
    pub id: String,
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct UserRow {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub is_owner: i64,
    pub is_disabled: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct SessionRow {
    pub id: String,
    pub user_id: String,
    pub workspace_id: String,
    pub token_hash: String,
    pub device_id: Option<String>,
    pub expires_at: String,
    pub revoked_at: Option<String>,
    pub username: String,
    pub is_owner: i64,
}

pub struct UserRepo<'a> {
    pub store: &'a Store,
}

impl UserRepo<'_> {
    pub async fn count_users(&self) -> PandaResult<i64> {
        let n: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(self.store.db.pool())
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(n.0)
    }

    pub async fn find_by_username(&self, username: &str) -> PandaResult<Option<UserRow>> {
        sqlx::query_as::<_, UserRow>(
            "SELECT id, username, password_hash, is_owner, is_disabled FROM users WHERE username = ?",
        )
        .bind(username)
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))
    }

    pub async fn find_by_id(&self, id: &str) -> PandaResult<Option<UserRow>> {
        sqlx::query_as::<_, UserRow>(
            "SELECT id, username, password_hash, is_owner, is_disabled FROM users WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))
    }

    pub async fn bootstrap_owner(
        &self,
        username: &str,
        password_hash: &str,
    ) -> PandaResult<(String, String)> {
        let _w = self.store.db.write().await;
        let now = now_rfc3339();
        let user_id = new_id();
        let ws_id = new_id();
        let mut tx = self
            .store
            .db
            .pool()
            .begin()
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;

        sqlx::query(
            "INSERT INTO workspaces (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&ws_id)
        .bind("Personal")
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        sqlx::query(
            "INSERT INTO users (id, username, password_hash, is_owner, is_disabled, created_at, updated_at)
             VALUES (?, ?, ?, 1, 0, ?, ?)",
        )
        .bind(&user_id)
        .bind(username)
        .bind(password_hash)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role, created_at) VALUES (?, ?, 'owner', ?)",
        )
        .bind(&ws_id)
        .bind(&user_id)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        sqlx::query(
            "INSERT INTO sync_meta (workspace_id, sync_epoch, updated_at) VALUES (?, 1, ?)",
        )
        .bind(&ws_id)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        // Default inbox notebook
        let nb_id = new_id();
        let path = format!("/{nb_id}");
        sqlx::query(
            "INSERT INTO notebooks (id, workspace_id, parent_id, name, slug, path, depth, sort_order, is_deleted, created_at, updated_at)
             VALUES (?, ?, NULL, 'Inbox', 'inbox', ?, 0, 0, 0, ?, ?)",
        )
        .bind(&nb_id)
        .bind(&ws_id)
        .bind(&path)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        // Demo notebook tree: Work / Personal with child notebooks.
        let work_id = new_id();
        let work_path = format!("/{work_id}");
        sqlx::query(
            "INSERT INTO notebooks (id, workspace_id, parent_id, name, slug, path, depth, sort_order, is_deleted, created_at, updated_at)
             VALUES (?, ?, NULL, 'Work', 'work', ?, 0, 1, 0, ?, ?)",
        )
        .bind(&work_id)
        .bind(&ws_id)
        .bind(&work_path)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        for (name, slug, sort_order) in [("Projects", "projects", 0), ("Meetings", "meetings", 1)] {
            let child_id = new_id();
            let child_path = format!("{work_path}/{child_id}");
            sqlx::query(
                "INSERT INTO notebooks (id, workspace_id, parent_id, name, slug, path, depth, sort_order, is_deleted, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, 1, ?, 0, ?, ?)",
            )
            .bind(&child_id)
            .bind(&ws_id)
            .bind(&work_id)
            .bind(name)
            .bind(slug)
            .bind(&child_path)
            .bind(sort_order)
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;
        }

        let personal_id = new_id();
        let personal_path = format!("/{personal_id}");
        sqlx::query(
            "INSERT INTO notebooks (id, workspace_id, parent_id, name, slug, path, depth, sort_order, is_deleted, created_at, updated_at)
             VALUES (?, ?, NULL, 'Personal', 'personal', ?, 0, 2, 0, ?, ?)",
        )
        .bind(&personal_id)
        .bind(&ws_id)
        .bind(&personal_path)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        for (name, slug, sort_order) in [("Journal", "journal", 0), ("Reading", "reading", 1)] {
            let child_id = new_id();
            let child_path = format!("{personal_path}/{child_id}");
            sqlx::query(
                "INSERT INTO notebooks (id, workspace_id, parent_id, name, slug, path, depth, sort_order, is_deleted, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, 1, ?, 0, ?, ?)",
            )
            .bind(&child_id)
            .bind(&ws_id)
            .bind(&personal_id)
            .bind(name)
            .bind(slug)
            .bind(&child_path)
            .bind(sort_order)
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok((user_id, ws_id))
    }

    /// Register a new user with an isolated Personal workspace (Inbox only, no demo tree).
    pub async fn create_personal_user(
        &self,
        username: &str,
        password_hash: &str,
    ) -> PandaResult<(String, String)> {
        let _w = self.store.db.write().await;
        let now = now_rfc3339();
        let user_id = new_id();
        let ws_id = new_id();
        let mut tx = self
            .store
            .db
            .pool()
            .begin()
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;

        sqlx::query(
            "INSERT INTO workspaces (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&ws_id)
        .bind("Personal")
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        sqlx::query(
            "INSERT INTO users (id, username, password_hash, is_owner, is_disabled, created_at, updated_at)
             VALUES (?, ?, ?, 1, 0, ?, ?)",
        )
        .bind(&user_id)
        .bind(username)
        .bind(password_hash)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("UNIQUE") || msg.contains("unique") {
                PandaError::conflict("username already taken")
            } else {
                PandaError::internal(msg)
            }
        })?;

        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role, created_at) VALUES (?, ?, 'owner', ?)",
        )
        .bind(&ws_id)
        .bind(&user_id)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        sqlx::query(
            "INSERT INTO sync_meta (workspace_id, sync_epoch, updated_at) VALUES (?, 1, ?)",
        )
        .bind(&ws_id)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        let nb_id = new_id();
        let path = format!("/{nb_id}");
        sqlx::query(
            "INSERT INTO notebooks (id, workspace_id, parent_id, name, slug, path, depth, sort_order, is_deleted, created_at, updated_at)
             VALUES (?, ?, NULL, 'Inbox', 'inbox', ?, 0, 0, 0, ?, ?)",
        )
        .bind(&nb_id)
        .bind(&ws_id)
        .bind(&path)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok((user_id, ws_id))
    }

    pub async fn create_session(
        &self,
        user_id: &str,
        workspace_id: &str,
        token_hash: &str,
        device_id: Option<&str>,
        expires_at: &str,
    ) -> PandaResult<String> {
        let _w = self.store.db.write().await;
        let id = new_id();
        let now = now_rfc3339();
        sqlx::query(
            "INSERT INTO sessions (id, user_id, workspace_id, token_hash, device_id, expires_at, revoked_at, last_seen_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, NULL, ?, ?)",
        )
        .bind(&id)
        .bind(user_id)
        .bind(workspace_id)
        .bind(token_hash)
        .bind(device_id)
        .bind(expires_at)
        .bind(&now)
        .bind(&now)
        .execute(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(id)
    }

    pub async fn find_session_by_token_hash(
        &self,
        token_hash: &str,
    ) -> PandaResult<Option<SessionRow>> {
        let now = now_rfc3339();
        sqlx::query_as::<_, SessionRow>(
            "SELECT s.id, s.user_id, s.workspace_id, s.token_hash, s.device_id, s.expires_at, s.revoked_at,
                    u.username, CASE WHEN wm.role = 'owner' THEN 1 ELSE 0 END AS is_owner
             FROM sessions s
             JOIN users u ON u.id = s.user_id
             JOIN workspace_members wm ON wm.user_id = s.user_id AND wm.workspace_id = s.workspace_id
             WHERE s.token_hash = ? AND s.revoked_at IS NULL AND s.expires_at > ? AND u.is_disabled = 0",
        )
        .bind(token_hash)
        .bind(&now)
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))
    }

    pub async fn touch_session(&self, session_id: &str) -> PandaResult<()> {
        let now = now_rfc3339();
        sqlx::query("UPDATE sessions SET last_seen_at = ? WHERE id = ?")
            .bind(&now)
            .bind(session_id)
            .execute(self.store.db.pool())
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(())
    }

    pub async fn revoke_session_by_token_hash(&self, token_hash: &str) -> PandaResult<()> {
        let _w = self.store.db.write().await;
        let now = now_rfc3339();
        sqlx::query(
            "UPDATE sessions SET revoked_at = ? WHERE token_hash = ? AND revoked_at IS NULL",
        )
        .bind(&now)
        .bind(token_hash)
        .execute(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(())
    }

    pub async fn primary_workspace(&self, user_id: &str) -> PandaResult<String> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT workspace_id FROM workspace_members WHERE user_id = ? ORDER BY created_at LIMIT 1",
        )
        .bind(user_id)
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        row.map(|r| r.0)
            .ok_or_else(|| PandaError::not_found("workspace not found"))
    }

    pub async fn resolve_workspace(
        &self,
        user_id: &str,
        requested_workspace_id: Option<&str>,
    ) -> PandaResult<(String, String)> {
        let row: Option<(String, String)> = if let Some(workspace_id) = requested_workspace_id {
            sqlx::query_as(
                "SELECT workspace_id, role FROM workspace_members
                 WHERE user_id = ? AND workspace_id = ?",
            )
            .bind(user_id)
            .bind(workspace_id)
            .fetch_optional(self.store.db.pool())
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?
        } else {
            sqlx::query_as(
                "SELECT workspace_id, role FROM workspace_members
                 WHERE user_id = ? ORDER BY created_at LIMIT 1",
            )
            .bind(user_id)
            .fetch_optional(self.store.db.pool())
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?
        };
        row.ok_or_else(|| {
            PandaError::new(
                domain::ErrorCode::PermissionDenied,
                "workspace membership required",
            )
        })
    }

    pub async fn membership_role(
        &self,
        workspace_id: &str,
        user_id: &str,
    ) -> PandaResult<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT role FROM workspace_members WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id)
        .bind(user_id)
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(row.map(|(role,)| role))
    }

    pub async fn list_workspace_memberships(
        &self,
        user_id: &str,
    ) -> PandaResult<Vec<WorkspaceMembership>> {
        sqlx::query_as(
            "SELECT w.id, w.name, wm.role
             FROM workspace_members wm
             JOIN workspaces w ON w.id = wm.workspace_id
             WHERE wm.user_id = ?
             ORDER BY wm.created_at, w.id",
        )
        .bind(user_id)
        .fetch_all(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))
    }

    pub async fn update_password(&self, user_id: &str, password_hash: &str) -> PandaResult<()> {
        let _w = self.store.db.write().await;
        let now = now_rfc3339();
        sqlx::query("UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?")
            .bind(password_hash)
            .bind(&now)
            .bind(user_id)
            .execute(self.store.db.pool())
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(())
    }

    pub async fn find_api_token(
        &self,
        token_hash: &str,
    ) -> PandaResult<Option<(String, String, String, String)>> {
        // workspace_id, user_id, scopes_json, id
        let row: Option<(String, String, String, String)> = sqlx::query_as(
            "SELECT workspace_id, user_id, scopes_json, id FROM api_tokens
             WHERE token_hash = ? AND (expires_at IS NULL OR expires_at > ?)",
        )
        .bind(token_hash)
        .bind(now_rfc3339())
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(row)
    }

    pub async fn create_api_token(
        &self,
        workspace_id: &str,
        user_id: &str,
        name: &str,
        token_hash: &str,
        scopes_json: &str,
        expires_at: Option<&str>,
    ) -> PandaResult<String> {
        let _w = self.store.db.write().await;
        let id = new_id();
        let now = now_rfc3339();
        sqlx::query(
            "INSERT INTO api_tokens (id, workspace_id, user_id, name, token_hash, scopes_json, expires_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(workspace_id)
        .bind(user_id)
        .bind(name)
        .bind(token_hash)
        .bind(scopes_json)
        .bind(expires_at)
        .bind(&now)
        .execute(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(id)
    }

    pub async fn list_api_tokens(
        &self,
        workspace_id: &str,
    ) -> PandaResult<Vec<(String, String, String, Option<String>, String)>> {
        sqlx::query_as(
            "SELECT id, name, scopes_json, expires_at, created_at FROM api_tokens WHERE workspace_id = ? ORDER BY created_at DESC",
        )
        .bind(workspace_id)
        .fetch_all(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))
    }

    pub async fn delete_api_token(&self, workspace_id: &str, id: &str) -> PandaResult<()> {
        let _w = self.store.db.write().await;
        sqlx::query("DELETE FROM api_tokens WHERE workspace_id = ? AND id = ?")
            .bind(workspace_id)
            .bind(id)
            .execute(self.store.db.pool())
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(())
    }

    pub async fn mark_login(&self, user_id: &str) -> PandaResult<()> {
        let now = now_rfc3339();
        sqlx::query("UPDATE users SET last_login_at = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(user_id)
            .execute(self.store.db.pool())
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(())
    }
}
