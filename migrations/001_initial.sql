-- Panda v1 schema (SQLite). Designed for meta/content split, CAS, device sync.

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS workspaces (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY NOT NULL,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    is_owner INTEGER NOT NULL DEFAULT 0,
    is_disabled INTEGER NOT NULL DEFAULT 0,
    last_login_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workspace_members (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL DEFAULT 'owner',
    created_at TEXT NOT NULL,
    PRIMARY KEY (workspace_id, user_id)
);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    device_id TEXT,
    expires_at TEXT NOT NULL,
    revoked_at TEXT,
    last_seen_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_token_hash ON sessions(token_hash);
CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);

CREATE TABLE IF NOT EXISTS api_tokens (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    scopes_json TEXT NOT NULL,
    last_used_at TEXT,
    expires_at TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS notebooks (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    parent_id TEXT REFERENCES notebooks(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    slug TEXT,
    path TEXT NOT NULL,
    depth INTEGER NOT NULL DEFAULT 0,
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_deleted INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_notebooks_ws_path ON notebooks(workspace_id, path);
CREATE INDEX IF NOT EXISTS idx_notebooks_ws_parent ON notebooks(workspace_id, parent_id);

CREATE TABLE IF NOT EXISTS content_blobs (
    hash TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    byte_size INTEGER NOT NULL,
    storage_key TEXT NOT NULL,
    refcount INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_content_blobs_ws ON content_blobs(workspace_id);

CREATE TABLE IF NOT EXISTS memos (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    notebook_id TEXT NOT NULL REFERENCES notebooks(id),
    title TEXT,
    excerpt TEXT NOT NULL DEFAULT '',
    tags_json TEXT NOT NULL DEFAULT '[]',
    is_pinned INTEGER NOT NULL DEFAULT 0,
    is_archived INTEGER NOT NULL DEFAULT 0,
    is_deleted INTEGER NOT NULL DEFAULT 0,
    revision INTEGER NOT NULL DEFAULT 1,
    content_hash TEXT NOT NULL,
    etag TEXT NOT NULL,
    source_memo_ids_json TEXT NOT NULL DEFAULT '[]',
    merge_source_count INTEGER NOT NULL DEFAULT 0,
    merged_into_memo_id TEXT,
    created_by TEXT,
    updated_by TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_memos_ws_notebook ON memos(workspace_id, notebook_id, is_deleted, updated_at);
CREATE INDEX IF NOT EXISTS idx_memos_ws_updated ON memos(workspace_id, updated_at, id);
CREATE INDEX IF NOT EXISTS idx_memos_ws_etag ON memos(workspace_id, id, etag);

-- Independent tasks. A task can reference a memo, but is never derived from its Markdown.
CREATE TABLE IF NOT EXISTS todos (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'inbox' CHECK(status IN ('inbox', 'open', 'completed')),
    due_date TEXT,
    priority INTEGER NOT NULL DEFAULT 0 CHECK(priority BETWEEN 0 AND 3),
    linked_memo_id TEXT REFERENCES memos(id) ON DELETE SET NULL,
    is_deleted INTEGER NOT NULL DEFAULT 0,
    revision INTEGER NOT NULL DEFAULT 1,
    etag TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT,
    deleted_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_todos_ws_status_due ON todos(workspace_id, is_deleted, status, due_date);
CREATE INDEX IF NOT EXISTS idx_todos_ws_updated ON todos(workspace_id, updated_at, id);

CREATE TABLE IF NOT EXISTS memo_contents (
    memo_id TEXT PRIMARY KEY NOT NULL REFERENCES memos(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    inline_markdown TEXT,
    blob_ref TEXT REFERENCES content_blobs(hash),
    byte_size INTEGER NOT NULL,
    content_text TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS memo_revisions (
    id TEXT PRIMARY KEY NOT NULL,
    memo_id TEXT NOT NULL REFERENCES memos(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL,
    title TEXT,
    tags_json TEXT NOT NULL DEFAULT '[]',
    content_hash TEXT NOT NULL,
    created_by TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(memo_id, revision)
);

CREATE TABLE IF NOT EXISTS edit_leases (
    id TEXT PRIMARY KEY NOT NULL,
    memo_id TEXT NOT NULL REFERENCES memos(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    base_revision INTEGER NOT NULL,
    base_content_hash TEXT NOT NULL,
    mode TEXT NOT NULL DEFAULT 'soft',
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_leases_memo ON edit_leases(memo_id, actor_id);

CREATE TABLE IF NOT EXISTS idempotency_keys (
    workspace_id TEXT NOT NULL,
    key TEXT NOT NULL,
    memo_id TEXT,
    response_json TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (workspace_id, key)
);

CREATE TABLE IF NOT EXISTS resources (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    memo_id TEXT REFERENCES memos(id) ON DELETE SET NULL,
    content_hash TEXT NOT NULL REFERENCES content_blobs(hash),
    kind TEXT NOT NULL,
    mime_type TEXT,
    filename TEXT,
    byte_size INTEGER NOT NULL,
    width INTEGER,
    height INTEGER,
    is_deleted INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_resources_ws_hash ON resources(workspace_id, content_hash);
CREATE INDEX IF NOT EXISTS idx_resources_memo ON resources(memo_id);

CREATE TABLE IF NOT EXISTS sync_meta (
    workspace_id TEXT PRIMARY KEY NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    sync_epoch INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sync_changes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    payload_kind TEXT NOT NULL,
    payload_json TEXT,
    fold_key TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sync_changes_ws_id ON sync_changes(workspace_id, id);
CREATE INDEX IF NOT EXISTS idx_sync_changes_fold ON sync_changes(workspace_id, fold_key, id);

CREATE TABLE IF NOT EXISTS device_cursors (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    device_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    cursor INTEGER NOT NULL DEFAULT 0,
    sync_epoch INTEGER NOT NULL DEFAULT 1,
    last_seen_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (workspace_id, device_id)
);

CREATE TABLE IF NOT EXISTS search_dirty (
    memo_id TEXT PRIMARY KEY NOT NULL REFERENCES memos(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL,
    marked_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS audit_events (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT,
    actor_id TEXT,
    action TEXT NOT NULL,
    entity_type TEXT,
    entity_id TEXT,
    metadata_json TEXT,
    created_at TEXT NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS memos_fts USING fts5(
    memo_id UNINDEXED,
    workspace_id UNINDEXED,
    title,
    content_text,
    tags,
    tokenize = 'porter unicode61'
);
