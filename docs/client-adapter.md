# Panda Desktop ↔ Server Adapter Notes

For `panda-desktop` `panda_*` crates. Treat the server implementation as
authoritative.

Canonical types: `crates/proto/src/lib.rs`  
Route table: `crates/server/src/routes/mod.rs`  
Base URL example: `http://127.0.0.1:8787/api/v1`

Concept mapping, CAS content, etag/revision concurrency, inventory/pull/push
sync, and SyncHint wakeups heavily reference the protocol design of
[EdgeEver](https://github.com/tianma-if/edgeever). Server routes and types in
this repo remain authoritative for Panda clients.

---

## Concept mapping

| UI / client | Server | Notes |
|-------------|--------|-------|
| Note | `Memo` | Markdown body; blake3 `content_hash` |
| Folder | `Notebook` | Tree (`parent_id` / `path` / `depth`) |
| Favorite | `is_pinned` | No separate favorite entity |
| Archive | `is_archived` | |
| Trash | `is_deleted` | Soft-delete by default; `permanent=` hard-deletes |
| Tags | `tags: Vec<String>` | Also `GET /tags` |
| Version | `revision` + `etag` | etag = `{revision}:{hash_prefix16}` |

`panda_core` should align with `MemoSummary` / `MemoDetail` / `MemoSaveRequest`
and must not invent a parallel model.

---

## Auth

- Every request: `Authorization: Bearer <token>`
- Desktop preferred flow: after login, create an **API token** (`pnda_…`) and
  store `apiUrl` + token locally
- Password login: `POST /auth/login` → `session_token` (bootstrap / token
  management only; not the steady-state connection mode)
- Sessions skip scope checks; API tokens carry scopes (for example
  `memos:read` / `memos:write`)
- Default bootstrap: `admin` / `admin123`

Related: `POST /auth/login`, `GET /auth/session`, `GET|POST /api-tokens`,
`DELETE /api-tokens/{id}`

---

## Three-pane API mapping

### Left pane (navigation)

| Capability | API |
|------------|-----|
| Folder tree | `GET /notebooks` |
| Tags | `GET /tags` |
| Trash entry | Client filter; list with `GET /memos?trash=true` |
| Favorites | Client filter on `is_pinned`, or set on save |

### Middle pane (list)

`GET /memos?notebook_id&trash&q&limit&cursor` → `MemoListResponse`
(**summaries only**)

`MemoSummary` key fields: `id`, `notebook_id`, `title?`, `excerpt`, `tags`,
`is_pinned`, `is_archived`, `is_deleted`, `revision`, `content_hash`, `etag`,
`created_at`, `updated_at`, `deleted_at?`

### Right pane (editor)

1. `POST /memos/{id}/open` (optional edit lease) → `MemoOpenResponse`
2. Body: `MemoContent.body` = `markdown` or `content_ref` (then
   `GET /contents/{hash}`)
3. Edit in the Zed buffer; debounce, write locally, then
   `POST /memos/{id}/save`
4. Save carries `if_match_etag` / `base_revision` / `base_content_hash` /
   `idempotency_key`; body = `markdown_full` | `markdown_patch` |
   `content_hash_only`
5. Success → `MemoSaveAck` (new `revision` / `etag` / `content_hash`)
6. Conflict → **409** + `current_etag`; phase-one policy: **conflict copy**,
   never silent overwrite

Create: `POST /memos` (markdown)  
Delete: `DELETE /memos/{id}` (soft-delete by default)  
Restore: `POST /memos/{id}/restore`

---

## Sync (`panda_sync`)

```text
inventory → pull changelog → push ops
         ↗
    WS SyncHint (wake client to pull)
```

| Step | API |
|------|-----|
| Full / inventory | `GET /sync/inventory?after&limit&device_id` |
| Incremental | `GET /sync/pull?cursor&limit&device_id` |
| Offline queue upload | `POST /sync/push` (`client_op_id` idempotency; ops: `memo.create` / `memo.update` / `memo.delete`) |
| Live hints | `GET /sync/ws`: first frame `SyncWsAuth { bearer, device_id, cursor }` → `SyncHello` → `SyncHint` |

Interactive editing still uses open/save; sync covers the offline queue and
multi-device catch-up.

---

## Attachments

- Upload: `POST /memos/{id}/resources`
- List: `GET /resources`
- Download: `GET /resources/{id}/blob`

Phase one can skip rich attachment editing.

---

## Wire format

- JSON by default
- Optional `Content-Type` / `Accept: application/x-protobuf` (prost)

Desktop `panda_api` can use JSON for phase one.

---

## Client layering

```text
panda_ui → panda_core → panda_store → panda_sync → panda_api
```

Editor / Buffer must not depend on `panda_api` directly. Save order: Buffer →
debounce → SQLite (`LocalModified`) → sync queue → server → `Synced`.
