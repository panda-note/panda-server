use crate::{now_rfc3339, Store};
use domain::{
    apply_markdown_patch, content_hash, derive_excerpt, derive_plain_text, etag_matches,
    format_etag, new_id, normalize_markdown, ErrorCode, PandaError, PandaResult,
};
use proto::{
    EditLease, MemoContent, MemoContentBody, MemoDetail, MemoInventoryItem, MemoSaveAck,
    MemoSummary, PROTOCOL_VERSION,
};
use serde_json::json;
use sqlx::FromRow;

#[derive(Debug, Clone)]
pub struct MemoListQuery {
    pub notebook_id: Option<String>,
    pub trash: bool,
    pub q: Option<String>,
    pub limit: i64,
    pub cursor: Option<String>,
}

#[derive(Debug, FromRow)]
struct MemoRow {
    id: String,
    notebook_id: String,
    title: Option<String>,
    excerpt: String,
    tags_json: String,
    is_pinned: i64,
    is_archived: i64,
    is_deleted: i64,
    revision: i64,
    content_hash: String,
    etag: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
}

#[derive(Debug, FromRow)]
pub(crate) struct ContentRow {
    #[allow(dead_code)]
    revision: i64,
    content_hash: String,
    inline_markdown: Option<String>,
    blob_ref: Option<String>,
    byte_size: i64,
}

pub struct MemoRepo<'a> {
    pub store: &'a Store,
}

impl MemoRepo<'_> {
    fn summary_from_row(r: MemoRow) -> MemoSummary {
        let tags: Vec<String> = serde_json::from_str(&r.tags_json).unwrap_or_default();
        MemoSummary {
            id: r.id,
            notebook_id: r.notebook_id,
            title: r.title,
            excerpt: r.excerpt,
            tags,
            is_pinned: r.is_pinned != 0,
            is_archived: r.is_archived != 0,
            is_deleted: r.is_deleted != 0,
            revision: r.revision as u64,
            content_hash: r.content_hash,
            etag: r.etag,
            created_at: r.created_at,
            updated_at: r.updated_at,
            deleted_at: r.deleted_at,
        }
    }

    pub async fn list(
        &self,
        workspace_id: &str,
        q: MemoListQuery,
    ) -> PandaResult<(Vec<MemoSummary>, Option<String>)> {
        let limit = q.limit.clamp(1, 200);
        let trash = if q.trash { 1 } else { 0 };
        let needle =
            q.q.as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());

        // Search path: filter first (LIKE + FTS), then paginate — never FTS-after-limit.
        if let Some(ref needle) = needle {
            let fts_ids = self.search_fts_ids(workspace_id, needle).await;
            let like = format!("%{needle}%");

            let mut sql = String::from(
                "SELECT id, notebook_id, title, excerpt, tags_json, is_pinned, is_archived, is_deleted,
                        revision, content_hash, etag, created_at, updated_at, deleted_at
                 FROM memos WHERE workspace_id = ? AND is_deleted = ?
                   AND (
                     IFNULL(title, '') LIKE ? COLLATE NOCASE
                     OR excerpt LIKE ? COLLATE NOCASE
                     OR tags_json LIKE ? COLLATE NOCASE",
            );
            if !fts_ids.is_empty() {
                sql.push_str(" OR id IN (");
                sql.push_str(&vec!["?"; fts_ids.len()].join(","));
                sql.push(')');
            }
            sql.push(')');
            if q.notebook_id.is_some() {
                sql.push_str(" AND notebook_id = ?");
            }
            if q.cursor.is_some() {
                sql.push_str(" AND (updated_at, id) < (?, ?)");
            }
            sql.push_str(" ORDER BY updated_at DESC, id DESC LIMIT ?");

            let mut query = sqlx::query_as::<_, MemoRow>(&sql)
                .bind(workspace_id)
                .bind(trash)
                .bind(&like)
                .bind(&like)
                .bind(&like);
            for id in &fts_ids {
                query = query.bind(id);
            }
            if let Some(ref nb) = q.notebook_id {
                query = query.bind(nb);
            }
            if let Some(ref c) = q.cursor {
                let (ts, id) = c.split_once('|').unwrap_or((c.as_str(), ""));
                query = query.bind(ts).bind(id);
            }
            query = query.bind(limit + 1);

            let mut rows = query
                .fetch_all(self.store.db.pool())
                .await
                .map_err(|e| PandaError::internal(e.to_string()))?;

            let mut next = None;
            if rows.len() as i64 > limit {
                rows.pop();
                if let Some(last) = rows.last() {
                    next = Some(format!("{}|{}", last.updated_at, last.id));
                }
            }

            return Ok((rows.into_iter().map(Self::summary_from_row).collect(), next));
        }

        let mut sql = String::from(
            "SELECT id, notebook_id, title, excerpt, tags_json, is_pinned, is_archived, is_deleted,
                    revision, content_hash, etag, created_at, updated_at, deleted_at
             FROM memos WHERE workspace_id = ? AND is_deleted = ?",
        );
        if q.notebook_id.is_some() {
            sql.push_str(" AND notebook_id = ?");
        }
        if q.cursor.is_some() {
            sql.push_str(" AND (updated_at, id) < (?, ?)");
        }
        sql.push_str(" ORDER BY updated_at DESC, id DESC LIMIT ?");

        let mut query = sqlx::query_as::<_, MemoRow>(&sql)
            .bind(workspace_id)
            .bind(trash);
        if let Some(ref nb) = q.notebook_id {
            query = query.bind(nb);
        }
        if let Some(ref c) = q.cursor {
            let (ts, id) = c.split_once('|').unwrap_or((c.as_str(), ""));
            query = query.bind(ts).bind(id);
        }
        query = query.bind(limit + 1);

        let mut rows = query
            .fetch_all(self.store.db.pool())
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;

        let mut next = None;
        if rows.len() as i64 > limit {
            rows.pop();
            if let Some(last) = rows.last() {
                next = Some(format!("{}|{}", last.updated_at, last.id));
            }
        }

        Ok((rows.into_iter().map(Self::summary_from_row).collect(), next))
    }

    /// FTS memo ids for content search. Failures / empty index return [].
    async fn search_fts_ids(&self, workspace_id: &str, needle: &str) -> Vec<String> {
        let sanitized = Self::sanitize_fts_query(needle);
        if sanitized.is_empty() {
            return Vec::new();
        }
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT memo_id FROM memos_fts WHERE workspace_id = ? AND memos_fts MATCH ? LIMIT 500",
        )
        .bind(workspace_id)
        .bind(&sanitized)
        .fetch_all(self.store.db.pool())
        .await
        .unwrap_or_default();
        rows.into_iter().map(|r| r.0).collect()
    }

    fn sanitize_fts_query(raw: &str) -> String {
        // Strip FTS operators that cause MATCH syntax errors; wrap tokens for prefix-friendly search.
        let cleaned: String = raw
            .chars()
            .map(|c| match c {
                '"' | '\'' | '*' | '(' | ')' | ':' | '^' => ' ',
                other => other,
            })
            .collect();
        cleaned
            .split_whitespace()
            .map(|token| format!("\"{token}\"*"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub async fn get_summary(
        &self,
        workspace_id: &str,
        id: &str,
    ) -> PandaResult<Option<MemoSummary>> {
        let row: Option<MemoRow> = sqlx::query_as(
            "SELECT id, notebook_id, title, excerpt, tags_json, is_pinned, is_archived, is_deleted,
                    revision, content_hash, etag, created_at, updated_at, deleted_at
             FROM memos WHERE workspace_id = ? AND id = ?",
        )
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(row.map(Self::summary_from_row))
    }

    async fn load_content_row(&self, workspace_id: &str, memo_id: &str) -> PandaResult<ContentRow> {
        sqlx::query_as::<_, ContentRow>(
            "SELECT mc.revision, mc.content_hash, mc.inline_markdown, mc.blob_ref, mc.byte_size
             FROM memo_contents mc
             JOIN memos m ON m.id = mc.memo_id
             WHERE m.workspace_id = ? AND mc.memo_id = ?",
        )
        .bind(workspace_id)
        .bind(memo_id)
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?
        .ok_or_else(|| PandaError::not_found("memo content missing"))
    }

    pub async fn read_markdown(
        &self,
        workspace_id: &str,
        memo_id: &str,
        load_blob: impl Fn(
            String,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = PandaResult<Vec<u8>>> + Send>,
        >,
    ) -> PandaResult<String> {
        let row = self.load_content_row(workspace_id, memo_id).await?;
        if let Some(md) = row.inline_markdown {
            return Ok(md);
        }
        if let Some(href) = row.blob_ref {
            let bytes = load_blob(href).await?;
            String::from_utf8(bytes).map_err(|e| PandaError::internal(format!("utf8 content: {e}")))
        } else {
            Err(PandaError::internal("content has neither inline nor blob"))
        }
    }

    pub async fn can_access_blob(&self, workspace_id: &str, hash: &str) -> PandaResult<bool> {
        let row: (i64,) = sqlx::query_as(
            "SELECT EXISTS(
                 SELECT 1
                 FROM memo_contents mc
                 JOIN memos m ON m.id = mc.memo_id
                 WHERE m.workspace_id = ? AND mc.blob_ref = ?
                 UNION ALL
                 SELECT 1 FROM resources r
                 WHERE r.workspace_id = ? AND r.content_hash = ? AND r.is_deleted = 0
             )",
        )
        .bind(workspace_id)
        .bind(hash)
        .bind(workspace_id)
        .bind(hash)
        .fetch_one(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(row.0 != 0)
    }

    async fn require_notebook(&self, workspace_id: &str, notebook_id: &str) -> PandaResult<()> {
        let notebook = self
            .store
            .notebooks()
            .get(workspace_id, notebook_id)
            .await?;
        if notebook.is_none_or(|notebook| notebook.is_deleted != 0) {
            return Err(PandaError::not_found("notebook not found in workspace"));
        }
        Ok(())
    }

    pub(crate) fn content_proto(row: &ContentRow, inline_threshold: u64) -> MemoContent {
        let body = if let Some(ref md) = row.inline_markdown {
            if row.byte_size as u64 <= inline_threshold || row.blob_ref.is_none() {
                Some(MemoContentBody::Markdown(md.clone()))
            } else if let Some(ref href) = row.blob_ref {
                Some(MemoContentBody::ContentRef(href.clone()))
            } else {
                Some(MemoContentBody::Markdown(md.clone()))
            }
        } else if let Some(ref href) = row.blob_ref {
            Some(MemoContentBody::ContentRef(href.clone()))
        } else {
            None
        };
        MemoContent {
            body,
            content_hash: row.content_hash.clone(),
            byte_size: row.byte_size as u64,
        }
    }

    pub async fn open(
        &self,
        workspace_id: &str,
        memo_id: &str,
        actor_id: &str,
        lease_mode: &str,
    ) -> PandaResult<(MemoDetail, String, Option<EditLease>)> {
        let summary = self
            .get_summary(workspace_id, memo_id)
            .await?
            .ok_or_else(|| PandaError::not_found("memo not found"))?;
        let crow = self.load_content_row(workspace_id, memo_id).await?;
        let content = Self::content_proto(&crow, self.store.inline_threshold);
        let etag = summary.etag.clone();

        let lease = if lease_mode == "none" {
            None
        } else {
            Some(
                self.create_lease(workspace_id, memo_id, actor_id, &summary, lease_mode)
                    .await?,
            )
        };

        Ok((
            MemoDetail {
                summary: Some(summary),
                content: Some(content),
            },
            etag,
            lease,
        ))
    }

    async fn create_lease(
        &self,
        workspace_id: &str,
        memo_id: &str,
        actor_id: &str,
        summary: &MemoSummary,
        mode: &str,
    ) -> PandaResult<EditLease> {
        let _w = self.store.db.write().await;
        let id = new_id();
        let now = chrono::Utc::now();
        let expires = now + chrono::Duration::days(30);
        let now_s = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let exp_s = expires.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let mode = if mode == "exclusive" {
            "exclusive"
        } else {
            "soft"
        };

        sqlx::query(
            "INSERT INTO edit_leases (id, memo_id, workspace_id, actor_id, base_revision, base_content_hash, mode, expires_at, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(memo_id)
        .bind(workspace_id)
        .bind(actor_id)
        .bind(summary.revision as i64)
        .bind(&summary.content_hash)
        .bind(mode)
        .bind(&exp_s)
        .bind(&now_s)
        .bind(&now_s)
        .execute(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        Ok(EditLease {
            id,
            memo_id: memo_id.to_string(),
            base_revision: summary.revision,
            base_content_hash: summary.content_hash.clone(),
            mode: mode.to_string(),
            expires_at: exp_s,
        })
    }

    pub async fn create(
        &self,
        workspace_id: &str,
        actor_id: &str,
        notebook_id: &str,
        title: Option<String>,
        markdown: &str,
        tags: &[String],
        put_blob: impl FnOnce(
            &[u8],
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = PandaResult<(String, u64)>> + Send>,
        >,
    ) -> PandaResult<MemoDetail> {
        self.require_notebook(workspace_id, notebook_id).await?;
        let _w = self.store.db.write().await;
        let normalized = normalize_markdown(markdown);
        let hash = content_hash(&normalized);
        let bytes = normalized.as_bytes();
        let byte_size = bytes.len() as u64;
        let excerpt = derive_excerpt(&normalized, 240);
        let text = derive_plain_text(&normalized);
        let now = now_rfc3339();
        let id = new_id();
        let revision = 1u64;
        let etag = format_etag(revision, &hash);
        let tags_json = serde_json::to_string(tags).unwrap_or_else(|_| "[]".into());

        let (inline, blob_ref) = if byte_size <= self.store.inline_threshold {
            (Some(normalized.clone()), None)
        } else {
            let (h, _) = put_blob(bytes).await?;
            self.upsert_blob_meta(workspace_id, &h, byte_size, &h)
                .await?;
            (None, Some(h))
        };

        let mut tx = self
            .store
            .db
            .pool()
            .begin()
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;

        if let Some(ref href) = blob_ref {
            sqlx::query("UPDATE content_blobs SET refcount = refcount + 1 WHERE hash = ?")
                .bind(href)
                .execute(&mut *tx)
                .await
                .ok();
        }

        sqlx::query(
            "INSERT INTO memos (id, workspace_id, notebook_id, title, excerpt, tags_json, is_pinned, is_archived, is_deleted,
             revision, content_hash, etag, created_by, updated_by, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, 0, 0, 0, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(workspace_id)
        .bind(notebook_id)
        .bind(&title)
        .bind(&excerpt)
        .bind(&tags_json)
        .bind(revision as i64)
        .bind(&hash)
        .bind(&etag)
        .bind(actor_id)
        .bind(actor_id)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        sqlx::query(
            "INSERT INTO memo_contents (memo_id, revision, content_hash, inline_markdown, blob_ref, byte_size, content_text, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(revision as i64)
        .bind(&hash)
        .bind(&inline)
        .bind(&blob_ref)
        .bind(byte_size as i64)
        .bind(&text)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        sqlx::query(
            "INSERT INTO search_dirty (memo_id, workspace_id, marked_at) VALUES (?, ?, ?)
             ON CONFLICT(memo_id) DO UPDATE SET marked_at = excluded.marked_at",
        )
        .bind(&id)
        .bind(workspace_id)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .ok();

        tx.commit()
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;

        // sync log outside tx (still under write lock)
        self.store
            .sync()
            .append_unlocked(
                workspace_id,
                "memo",
                &id,
                "upsert",
                "content",
                Some(&json!({"id": id, "etag": etag}).to_string()),
                &format!("memo:{id}:content"),
            )
            .await?;

        let summary = self.get_summary(workspace_id, &id).await?.unwrap();
        let crow = self.load_content_row(workspace_id, &id).await?;
        Ok(MemoDetail {
            summary: Some(summary),
            content: Some(Self::content_proto(&crow, self.store.inline_threshold)),
        })
    }

    async fn upsert_blob_meta(
        &self,
        workspace_id: &str,
        hash: &str,
        byte_size: u64,
        storage_key: &str,
    ) -> PandaResult<()> {
        let now = now_rfc3339();
        sqlx::query(
            "INSERT INTO content_blobs (hash, workspace_id, byte_size, storage_key, refcount, created_at)
             VALUES (?, ?, ?, ?, 0, ?)
             ON CONFLICT(hash) DO NOTHING",
        )
        .bind(hash)
        .bind(workspace_id)
        .bind(byte_size as i64)
        .bind(storage_key)
        .bind(&now)
        .execute(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(())
    }

    pub async fn save(
        &self,
        workspace_id: &str,
        actor_id: &str,
        memo_id: &str,
        if_match_etag: Option<&str>,
        base_revision: Option<u64>,
        base_content_hash: Option<&str>,
        lease_id: Option<&str>,
        idempotency_key: &str,
        body: Option<proto::SaveBody>,
        title: Option<String>,
        tags: Option<Vec<String>>,
        is_pinned: Option<bool>,
        is_archived: Option<bool>,
        notebook_id: Option<String>,
        put_blob: impl FnOnce(
            &[u8],
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = PandaResult<(String, u64)>> + Send>,
        >,
        load_blob: impl Fn(
            String,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = PandaResult<Vec<u8>>> + Send>,
        >,
    ) -> PandaResult<MemoSaveAck> {
        if idempotency_key.is_empty() {
            return Err(PandaError::invalid("idempotency_key required"));
        }

        // Idempotency hit?
        if let Some(ack) = self.get_idempotency(workspace_id, idempotency_key).await? {
            return Ok(ack);
        }

        let _w = self.store.db.write().await;
        // re-check under lock
        if let Some(ack) = self.get_idempotency(workspace_id, idempotency_key).await? {
            return Ok(ack);
        }

        let summary = self
            .get_summary(workspace_id, memo_id)
            .await?
            .ok_or_else(|| PandaError::not_found("memo not found"))?;

        // Precondition
        if let Some(etag) = if_match_etag {
            if !etag_matches(etag, summary.revision, &summary.content_hash) {
                return Err(
                    PandaError::new(ErrorCode::RevisionConflict, "etag mismatch")
                        .with_etag(summary.etag.clone()),
                );
            }
        } else if let (Some(rev), Some(hash)) = (base_revision, base_content_hash) {
            if rev != summary.revision || hash != summary.content_hash {
                return Err(PandaError::new(
                    ErrorCode::ContentConflict,
                    "base revision/hash mismatch",
                )
                .with_etag(summary.etag.clone()));
            }
        } else if body.is_some() {
            return Err(PandaError::new(
                ErrorCode::PreconditionFailed,
                "if_match_etag or base_revision+base_content_hash required for content save",
            ));
        }

        if let Some(lid) = lease_id {
            let ok: Option<(i64,)> = sqlx::query_as(
                "SELECT 1 FROM edit_leases
                 WHERE workspace_id = ? AND id = ? AND memo_id = ? AND actor_id = ? AND expires_at > ?",
            )
            .bind(workspace_id)
            .bind(lid)
            .bind(memo_id)
            .bind(actor_id)
            .bind(now_rfc3339())
            .fetch_optional(self.store.db.pool())
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;
            if ok.is_none() {
                return Err(
                    PandaError::new(ErrorCode::LeaseConflict, "invalid or expired lease")
                        .with_etag(summary.etag.clone()),
                );
            }
        }

        if let Some(ref requested_notebook_id) = notebook_id {
            self.require_notebook(workspace_id, requested_notebook_id)
                .await?;
        }
        let crow = self.load_content_row(workspace_id, memo_id).await?;
        let mut new_markdown: Option<String> = None;
        let mut content_changed = false;

        match body {
            Some(proto::SaveBody::MarkdownFull(md)) => {
                new_markdown = Some(normalize_markdown(&md));
                content_changed = true;
            }
            Some(proto::SaveBody::MarkdownPatch(patch)) => {
                let base = if let Some(md) = crow.inline_markdown.clone() {
                    md
                } else if let Some(href) = crow.blob_ref.clone() {
                    let b = load_blob(href).await?;
                    String::from_utf8(b).map_err(|e| PandaError::internal(e.to_string()))?
                } else {
                    String::new()
                };
                new_markdown = Some(normalize_markdown(&apply_markdown_patch(&base, &patch)?));
                content_changed = true;
            }
            Some(proto::SaveBody::ContentHashOnly(hash)) => {
                // CAS reuse
                if !self.can_access_blob(workspace_id, &hash).await? {
                    return Err(PandaError::not_found("content hash not found in CAS"));
                }
                // treat as content change pointing at hash; load bytes for excerpt
                let bytes = load_blob(hash.clone()).await?;
                let md =
                    String::from_utf8(bytes).map_err(|e| PandaError::internal(e.to_string()))?;
                new_markdown = Some(normalize_markdown(&md));
                content_changed = true;
            }
            None => {}
        }

        let meta_only = !content_changed
            && (title.is_some()
                || tags.is_some()
                || is_pinned.is_some()
                || is_archived.is_some()
                || notebook_id.is_some());

        if !content_changed && !meta_only {
            return Err(PandaError::invalid("nothing to save"));
        }

        let now = now_rfc3339();
        let next_revision = if content_changed {
            summary.revision + 1
        } else {
            summary.revision
        };

        let (hash, byte_size, inline, blob_ref, excerpt) = if let Some(ref md) = new_markdown {
            let hash = content_hash(md);
            let byte_size = md.len() as u64;
            let excerpt = derive_excerpt(md, 240);
            let (inline, blob_ref) = if byte_size <= self.store.inline_threshold {
                (Some(md.clone()), None::<String>)
            } else {
                let (h, _) = put_blob(md.as_bytes()).await?;
                self.upsert_blob_meta(workspace_id, &h, byte_size, &h)
                    .await?;
                (None, Some(h))
            };
            (hash, byte_size, inline, blob_ref, excerpt)
        } else {
            (
                summary.content_hash.clone(),
                crow.byte_size as u64,
                crow.inline_markdown.clone(),
                crow.blob_ref.clone(),
                summary.excerpt.clone(),
            )
        };

        let etag = format_etag(next_revision, &hash);
        let new_title = title.or(summary.title.clone());
        let new_tags = tags.unwrap_or(summary.tags.clone());
        let tags_json = serde_json::to_string(&new_tags).unwrap_or_else(|_| "[]".into());
        let new_pinned = is_pinned.unwrap_or(summary.is_pinned);
        let new_archived = is_archived.unwrap_or(summary.is_archived);
        let new_notebook = notebook_id.unwrap_or(summary.notebook_id.clone());

        // Maybe snapshot previous
        if content_changed {
            self.maybe_snapshot(memo_id, &summary, actor_id, &now)
                .await?;
        }

        let mut tx = self
            .store
            .db
            .pool()
            .begin()
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;

        sqlx::query(
            "UPDATE memos SET notebook_id = ?, title = ?, excerpt = ?, tags_json = ?, is_pinned = ?, is_archived = ?,
             revision = ?, content_hash = ?, etag = ?, updated_by = ?, updated_at = ?
             WHERE id = ? AND workspace_id = ?",
        )
        .bind(&new_notebook)
        .bind(&new_title)
        .bind(&excerpt)
        .bind(&tags_json)
        .bind(new_pinned as i64)
        .bind(new_archived as i64)
        .bind(next_revision as i64)
        .bind(&hash)
        .bind(&etag)
        .bind(actor_id)
        .bind(&now)
        .bind(memo_id)
        .bind(workspace_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        if content_changed {
            let text = new_markdown
                .as_ref()
                .map(|m| derive_plain_text(m))
                .unwrap_or_default();
            sqlx::query(
                "UPDATE memo_contents SET revision = ?, content_hash = ?, inline_markdown = ?, blob_ref = ?,
                 byte_size = ?, content_text = ?, updated_at = ? WHERE memo_id = ?",
            )
            .bind(next_revision as i64)
            .bind(&hash)
            .bind(&inline)
            .bind(&blob_ref)
            .bind(byte_size as i64)
            .bind(&text)
            .bind(&now)
            .bind(memo_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;

            if let Some(ref href) = blob_ref {
                sqlx::query("UPDATE content_blobs SET refcount = refcount + 1 WHERE hash = ?")
                    .bind(href)
                    .execute(&mut *tx)
                    .await
                    .ok();
            }

            sqlx::query(
                "INSERT INTO search_dirty (memo_id, workspace_id, marked_at) VALUES (?, ?, ?)
                 ON CONFLICT(memo_id) DO UPDATE SET marked_at = excluded.marked_at",
            )
            .bind(memo_id)
            .bind(workspace_id)
            .bind(&now)
            .execute(&mut *tx)
            .await
            .ok();
        }

        if let Some(lid) = lease_id {
            sqlx::query(
                "UPDATE edit_leases SET base_revision = ?, base_content_hash = ?, updated_at = ? WHERE id = ?",
            )
            .bind(next_revision as i64)
            .bind(&hash)
            .bind(&now)
            .bind(lid)
            .execute(&mut *tx)
            .await
            .ok();
        }

        let ack = MemoSaveAck {
            protocol_version: PROTOCOL_VERSION,
            revision: next_revision,
            content_hash: hash.clone(),
            etag: etag.clone(),
            lease_id: lease_id.map(|s| s.to_string()),
            saved_at: now.clone(),
        };
        let ack_json = serde_json::to_string(&ack).unwrap_or_default();
        let exp = (chrono::Utc::now() + chrono::Duration::hours(24))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        sqlx::query(
            "INSERT INTO idempotency_keys (workspace_id, key, memo_id, response_json, expires_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(workspace_id)
        .bind(idempotency_key)
        .bind(memo_id)
        .bind(&ack_json)
        .bind(&exp)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .ok();

        tx.commit()
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;

        let kind = if content_changed { "content" } else { "meta" };
        self.store
            .sync()
            .append_unlocked(
                workspace_id,
                "memo",
                memo_id,
                "upsert",
                kind,
                Some(&json!({"id": memo_id, "etag": etag, "kind": kind}).to_string()),
                &format!("memo:{memo_id}:{kind}"),
            )
            .await?;

        Ok(ack)
    }

    async fn get_idempotency(
        &self,
        workspace_id: &str,
        key: &str,
    ) -> PandaResult<Option<MemoSaveAck>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT response_json FROM idempotency_keys WHERE workspace_id = ? AND key = ? AND expires_at > ?",
        )
        .bind(workspace_id)
        .bind(key)
        .bind(now_rfc3339())
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(row.and_then(|(j,)| serde_json::from_str(&j).ok()))
    }

    async fn maybe_snapshot(
        &self,
        memo_id: &str,
        summary: &MemoSummary,
        actor_id: &str,
        now: &str,
    ) -> PandaResult<()> {
        let last: Option<(String,)> = sqlx::query_as(
            "SELECT created_at FROM memo_revisions WHERE memo_id = ? ORDER BY revision DESC LIMIT 1",
        )
        .bind(memo_id)
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;

        let should = match last {
            None => true,
            Some((ts,)) => {
                if let Ok(t) = chrono::DateTime::parse_from_rfc3339(&ts) {
                    let age =
                        chrono::Utc::now().signed_duration_since(t.with_timezone(&chrono::Utc));
                    age.num_seconds() >= 300
                } else {
                    true
                }
            }
        };
        if !should {
            return Ok(());
        }
        let id = new_id();
        let tags_json = serde_json::to_string(&summary.tags).unwrap_or_else(|_| "[]".into());
        sqlx::query(
            "INSERT INTO memo_revisions (id, memo_id, revision, title, tags_json, content_hash, created_by, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(memo_id)
        .bind(summary.revision as i64)
        .bind(&summary.title)
        .bind(&tags_json)
        .bind(&summary.content_hash)
        .bind(actor_id)
        .bind(now)
        .execute(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(())
    }

    pub async fn soft_delete(
        &self,
        workspace_id: &str,
        memo_id: &str,
        permanent: bool,
    ) -> PandaResult<()> {
        let _w = self.store.db.write().await;
        let now = now_rfc3339();
        if permanent {
            sqlx::query("DELETE FROM memos WHERE workspace_id = ? AND id = ?")
                .bind(workspace_id)
                .bind(memo_id)
                .execute(self.store.db.pool())
                .await
                .map_err(|e| PandaError::internal(e.to_string()))?;
            self.store
                .sync()
                .append_unlocked(
                    workspace_id,
                    "memo",
                    memo_id,
                    "delete",
                    "delete",
                    None,
                    &format!("memo:{memo_id}:delete"),
                )
                .await?;
        } else {
            sqlx::query(
                "UPDATE memos SET is_deleted = 1, deleted_at = ?, updated_at = ? WHERE workspace_id = ? AND id = ?",
            )
            .bind(&now)
            .bind(&now)
            .bind(workspace_id)
            .bind(memo_id)
            .execute(self.store.db.pool())
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;
            self.store
                .sync()
                .append_unlocked(
                    workspace_id,
                    "memo",
                    memo_id,
                    "upsert",
                    "meta",
                    Some(&json!({"id": memo_id, "is_deleted": true}).to_string()),
                    &format!("memo:{memo_id}:meta"),
                )
                .await?;
        }
        Ok(())
    }

    pub async fn restore(&self, workspace_id: &str, memo_id: &str) -> PandaResult<()> {
        let _w = self.store.db.write().await;
        let now = now_rfc3339();
        sqlx::query(
            "UPDATE memos SET is_deleted = 0, deleted_at = NULL, updated_at = ? WHERE workspace_id = ? AND id = ?",
        )
        .bind(&now)
        .bind(workspace_id)
        .bind(memo_id)
        .execute(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        self.store
            .sync()
            .append_unlocked(
                workspace_id,
                "memo",
                memo_id,
                "upsert",
                "meta",
                Some(&json!({"id": memo_id, "is_deleted": false}).to_string()),
                &format!("memo:{memo_id}:meta"),
            )
            .await?;
        Ok(())
    }

    pub async fn batch_move(
        &self,
        workspace_id: &str,
        memo_ids: &[String],
        notebook_id: &str,
    ) -> PandaResult<u64> {
        self.require_notebook(workspace_id, notebook_id).await?;
        let _w = self.store.db.write().await;
        let now = now_rfc3339();
        let mut n = 0u64;
        for id in memo_ids {
            let r = sqlx::query(
                "UPDATE memos SET notebook_id = ?, updated_at = ? WHERE workspace_id = ? AND id = ?",
            )
            .bind(notebook_id)
            .bind(&now)
            .bind(workspace_id)
            .bind(id)
            .execute(self.store.db.pool())
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;
            if r.rows_affected() > 0 {
                n += 1;
                self.store
                    .sync()
                    .append_unlocked(
                        workspace_id,
                        "memo",
                        id,
                        "upsert",
                        "meta",
                        Some(&json!({"id": id, "notebook_id": notebook_id}).to_string()),
                        &format!("memo:{id}:meta"),
                    )
                    .await?;
            }
        }
        Ok(n)
    }

    pub async fn inventory_page(
        &self,
        workspace_id: &str,
        after: Option<&str>,
        limit: i64,
    ) -> PandaResult<(Vec<MemoInventoryItem>, bool, Option<String>)> {
        let limit = limit.clamp(1, 1000);
        let mut sql = String::from(
            "SELECT id, notebook_id, etag, is_pinned, is_archived, is_deleted, updated_at
             FROM memos WHERE workspace_id = ?",
        );
        if after.is_some() {
            sql.push_str(" AND id > ?");
        }
        sql.push_str(" ORDER BY id ASC LIMIT ?");

        let mut q = sqlx::query_as::<_, InventoryRow>(&sql).bind(workspace_id);
        if let Some(a) = after {
            q = q.bind(a);
        }
        q = q.bind(limit + 1);
        let mut rows = q
            .fetch_all(self.store.db.pool())
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;
        let mut has_more = false;
        let mut next = None;
        if rows.len() as i64 > limit {
            has_more = true;
            rows.pop();
            next = rows.last().map(|r| r.id.clone());
        }
        Ok((
            rows.into_iter()
                .map(|r| MemoInventoryItem {
                    id: r.id,
                    notebook_id: r.notebook_id,
                    etag: r.etag,
                    is_pinned: r.is_pinned != 0,
                    is_archived: r.is_archived != 0,
                    is_deleted: r.is_deleted != 0,
                    updated_at: r.updated_at,
                })
                .collect(),
            has_more,
            next,
        ))
    }

    pub async fn list_revisions(
        &self,
        workspace_id: &str,
        memo_id: &str,
        limit: i64,
    ) -> PandaResult<Vec<proto::MemoRevision>> {
        let _ = self
            .get_summary(workspace_id, memo_id)
            .await?
            .ok_or_else(|| PandaError::not_found("memo not found"))?;
        let rows: Vec<RevRow> = sqlx::query_as(
            "SELECT id, memo_id, revision, title, tags_json, content_hash, created_at
             FROM memo_revisions WHERE memo_id = ? ORDER BY revision DESC LIMIT ?",
        )
        .bind(memo_id)
        .bind(limit.clamp(1, 100))
        .fetch_all(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|r| proto::MemoRevision {
                id: r.id,
                memo_id: r.memo_id,
                revision: r.revision as u64,
                title: r.title,
                tags: serde_json::from_str(&r.tags_json).unwrap_or_default(),
                content_hash: r.content_hash,
                created_at: r.created_at,
            })
            .collect())
    }

    pub async fn restore_revision(
        &self,
        workspace_id: &str,
        actor_id: &str,
        memo_id: &str,
        revision_id: &str,
        put_blob: impl FnOnce(
            &[u8],
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = PandaResult<(String, u64)>> + Send>,
        >,
        load_blob: impl Fn(
            String,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = PandaResult<Vec<u8>>> + Send>,
        >,
    ) -> PandaResult<MemoSaveAck> {
        let summary = self
            .get_summary(workspace_id, memo_id)
            .await?
            .ok_or_else(|| PandaError::not_found("memo not found"))?;
        let row: Option<(i64, Option<String>, String, String)> = sqlx::query_as(
            "SELECT revision, title, tags_json, content_hash FROM memo_revisions WHERE id = ? AND memo_id = ?",
        )
        .bind(revision_id)
        .bind(memo_id)
        .fetch_optional(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        let (_rev, title, tags_json, hash) =
            row.ok_or_else(|| PandaError::not_found("revision not found"))?;
        let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
        let md_bytes = match load_blob(hash.clone()).await {
            Ok(b) => b,
            Err(_) => {
                let crow = self.load_content_row(workspace_id, memo_id).await?;
                if crow.content_hash == hash {
                    if let Some(md) = crow.inline_markdown {
                        md.into_bytes()
                    } else {
                        return Err(PandaError::not_found(
                            "revision content blob missing; ensure CAS retained hash",
                        ));
                    }
                } else {
                    return Err(PandaError::not_found(
                        "revision content blob missing; ensure CAS retained hash",
                    ));
                }
            }
        };
        let md = String::from_utf8(md_bytes).map_err(|e| PandaError::internal(e.to_string()))?;
        self.save(
            workspace_id,
            actor_id,
            memo_id,
            Some(&summary.etag),
            None,
            None,
            None,
            &new_id(),
            Some(proto::SaveBody::MarkdownFull(md)),
            title,
            Some(tags),
            None,
            None,
            None,
            put_blob,
            load_blob,
        )
        .await
    }

    pub async fn merge(
        &self,
        workspace_id: &str,
        actor_id: &str,
        memo_ids: &[String],
        notebook_id: &str,
        title: Option<String>,
        put_blob: impl FnOnce(
                &[u8],
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = PandaResult<(String, u64)>> + Send>,
            > + Clone,
        load_blob: impl Fn(
                String,
            )
                -> std::pin::Pin<Box<dyn std::future::Future<Output = PandaResult<Vec<u8>>> + Send>>
            + Clone,
    ) -> PandaResult<MemoDetail> {
        if memo_ids.len() < 2 {
            return Err(PandaError::invalid("merge requires >= 2 memos"));
        }
        let mut parts = Vec::new();
        let mut all_tags = Vec::new();
        for id in memo_ids {
            let s = self
                .get_summary(workspace_id, id)
                .await?
                .ok_or_else(|| PandaError::not_found(format!("memo {id}")))?;
            all_tags.extend(s.tags);
            let md = self
                .read_markdown(workspace_id, id, load_blob.clone())
                .await?;
            let heading = s.title.unwrap_or_else(|| id.clone());
            parts.push(format!("# {heading}\n\n{md}"));
        }
        all_tags.sort();
        all_tags.dedup();
        let merged = parts.join("\n\n---\n\n");
        let detail = self
            .create(
                workspace_id,
                actor_id,
                notebook_id,
                title.or_else(|| Some("Merged".into())),
                &merged,
                &all_tags,
                put_blob,
            )
            .await?;
        let new_id = detail.summary.as_ref().unwrap().id.clone();
        for id in memo_ids {
            let _w = self.store.db.write().await;
            let now = now_rfc3339();
            sqlx::query(
                "UPDATE memos SET is_deleted = 1, deleted_at = ?, merged_into_memo_id = ?, updated_at = ?
                 WHERE workspace_id = ? AND id = ?",
            )
            .bind(&now)
            .bind(&new_id)
            .bind(&now)
            .bind(workspace_id)
            .bind(id)
            .execute(self.store.db.pool())
            .await
            .ok();
        }
        Ok(detail)
    }

    pub async fn list_tags(&self, workspace_id: &str) -> PandaResult<Vec<proto::TagSummary>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT tags_json FROM memos WHERE workspace_id = ? AND is_deleted = 0")
                .bind(workspace_id)
                .fetch_all(self.store.db.pool())
                .await
                .map_err(|e| PandaError::internal(e.to_string()))?;
        let mut map = std::collections::HashMap::<String, i64>::new();
        for (tj,) in rows {
            let tags: Vec<String> = serde_json::from_str(&tj).unwrap_or_default();
            for t in tags {
                *map.entry(t).or_default() += 1;
            }
        }
        let mut items: Vec<_> = map
            .into_iter()
            .map(|(tag, memo_count)| proto::TagSummary { tag, memo_count })
            .collect();
        items.sort_by(|a, b| a.tag.cmp(&b.tag));
        Ok(items)
    }

    pub async fn process_search_dirty(&self, limit: i64) -> PandaResult<u64> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT memo_id, workspace_id FROM search_dirty ORDER BY marked_at LIMIT ?",
        )
        .bind(limit)
        .fetch_all(self.store.db.pool())
        .await
        .map_err(|e| PandaError::internal(e.to_string()))?;
        let mut n = 0u64;
        for (memo_id, ws) in rows {
            let row: Option<(Option<String>, String, String)> = sqlx::query_as(
                "SELECT m.title, m.tags_json, COALESCE(c.content_text, '') FROM memos m
                 JOIN memo_contents c ON c.memo_id = m.id
                 WHERE m.workspace_id = ? AND m.id = ?",
            )
            .bind(&ws)
            .bind(&memo_id)
            .fetch_optional(self.store.db.pool())
            .await
            .map_err(|e| PandaError::internal(e.to_string()))?;
            if let Some((title, tags, text)) = row {
                sqlx::query("DELETE FROM memos_fts WHERE memo_id = ?")
                    .bind(&memo_id)
                    .execute(self.store.db.pool())
                    .await
                    .ok();
                sqlx::query(
                    "INSERT INTO memos_fts (memo_id, workspace_id, title, content_text, tags) VALUES (?, ?, ?, ?, ?)",
                )
                .bind(&memo_id)
                .bind(&ws)
                .bind(title.unwrap_or_default())
                .bind(text)
                .bind(tags)
                .execute(self.store.db.pool())
                .await
                .ok();
            }
            sqlx::query("DELETE FROM search_dirty WHERE memo_id = ?")
                .bind(&memo_id)
                .execute(self.store.db.pool())
                .await
                .ok();
            n += 1;
        }
        Ok(n)
    }
}

#[derive(FromRow)]
struct InventoryRow {
    id: String,
    notebook_id: String,
    etag: String,
    is_pinned: i64,
    is_archived: i64,
    is_deleted: i64,
    updated_at: String,
}

#[derive(FromRow)]
struct RevRow {
    id: String,
    memo_id: String,
    revision: i64,
    title: Option<String>,
    tags_json: String,
    content_hash: String,
    created_at: String,
}
