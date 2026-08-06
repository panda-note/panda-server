//! Panda Protocol v1 wire types (prost). `.proto` sources live in `/proto` for cross-language clients.

use prost::{Message, Oneof};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct ApiError {
    #[prost(string, tag = "1")]
    pub code: String,
    #[prost(string, tag = "2")]
    pub message: String,
    #[prost(bool, tag = "3")]
    pub retryable: bool,
    #[prost(string, optional, tag = "4")]
    pub current_etag: Option<String>,
    #[prost(string, optional, tag = "5")]
    pub details_json: Option<String>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct Notebook {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, optional, tag = "2")]
    pub parent_id: Option<String>,
    #[prost(string, tag = "3")]
    pub name: String,
    #[prost(string, optional, tag = "4")]
    pub slug: Option<String>,
    #[prost(string, tag = "5")]
    pub path: String,
    #[prost(int32, tag = "6")]
    pub depth: i32,
    #[prost(int32, tag = "7")]
    pub sort_order: i32,
    #[prost(int64, tag = "8")]
    pub memo_count: i64,
    #[prost(bool, tag = "9")]
    pub is_deleted: bool,
    #[prost(string, tag = "10")]
    pub created_at: String,
    #[prost(string, tag = "11")]
    pub updated_at: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct MemoSummary {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub notebook_id: String,
    #[prost(string, optional, tag = "3")]
    pub title: Option<String>,
    #[prost(string, tag = "4")]
    pub excerpt: String,
    #[prost(string, repeated, tag = "5")]
    pub tags: Vec<String>,
    #[prost(bool, tag = "6")]
    pub is_pinned: bool,
    #[prost(bool, tag = "7")]
    pub is_archived: bool,
    #[prost(bool, tag = "8")]
    pub is_deleted: bool,
    #[prost(uint64, tag = "9")]
    pub revision: u64,
    #[prost(string, tag = "10")]
    pub content_hash: String,
    #[prost(string, tag = "11")]
    pub etag: String,
    #[prost(string, tag = "12")]
    pub created_at: String,
    #[prost(string, tag = "13")]
    pub updated_at: String,
    #[prost(string, optional, tag = "14")]
    pub deleted_at: Option<String>,
}

#[derive(Clone, PartialEq, Oneof, Serialize, Deserialize)]
pub enum MemoContentBody {
    #[prost(string, tag = "1")]
    Markdown(String),
    #[prost(string, tag = "2")]
    ContentRef(String),
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct MemoContent {
    #[prost(oneof = "MemoContentBody", tags = "1, 2")]
    pub body: Option<MemoContentBody>,
    #[prost(string, tag = "3")]
    pub content_hash: String,
    #[prost(uint64, tag = "4")]
    pub byte_size: u64,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct MemoDetail {
    #[prost(message, optional, tag = "1")]
    pub summary: Option<MemoSummary>,
    #[prost(message, optional, tag = "2")]
    pub content: Option<MemoContent>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct EditLease {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub memo_id: String,
    #[prost(uint64, tag = "3")]
    pub base_revision: u64,
    #[prost(string, tag = "4")]
    pub base_content_hash: String,
    #[prost(string, tag = "5")]
    pub mode: String,
    #[prost(string, tag = "6")]
    pub expires_at: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct MemoOpenRequest {
    #[prost(uint32, tag = "1")]
    pub protocol_version: u32,
    #[prost(string, tag = "2")]
    pub lease_mode: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct MemoOpenResponse {
    #[prost(uint32, tag = "1")]
    pub protocol_version: u32,
    #[prost(message, optional, tag = "2")]
    pub memo: Option<MemoDetail>,
    #[prost(string, tag = "3")]
    pub etag: String,
    #[prost(message, optional, tag = "4")]
    pub lease: Option<EditLease>,
}

#[derive(Clone, PartialEq, Oneof, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveBody {
    #[prost(string, tag = "10")]
    MarkdownFull(String),
    #[prost(string, tag = "11")]
    MarkdownPatch(String),
    #[prost(string, tag = "12")]
    ContentHashOnly(String),
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct MemoSaveRequest {
    #[prost(uint32, tag = "1")]
    #[serde(default)]
    pub protocol_version: u32,
    #[prost(string, optional, tag = "2")]
    #[serde(default)]
    pub if_match_etag: Option<String>,
    #[prost(uint64, optional, tag = "3")]
    #[serde(default)]
    pub base_revision: Option<u64>,
    #[prost(string, optional, tag = "4")]
    #[serde(default)]
    pub base_content_hash: Option<String>,
    #[prost(string, optional, tag = "5")]
    #[serde(default)]
    pub lease_id: Option<String>,
    #[prost(string, tag = "6")]
    #[serde(default)]
    pub idempotency_key: String,
    #[prost(oneof = "SaveBody", tags = "10, 11, 12")]
    #[serde(default, flatten)]
    pub body: Option<SaveBody>,
    #[prost(string, optional, tag = "20")]
    #[serde(default)]
    pub title: Option<String>,
    #[prost(string, repeated, tag = "21")]
    #[serde(default)]
    pub tags: Vec<String>,
    #[prost(bool, optional, tag = "22")]
    #[serde(default)]
    pub is_pinned: Option<bool>,
    #[prost(bool, optional, tag = "23")]
    #[serde(default)]
    pub is_archived: Option<bool>,
    #[prost(string, optional, tag = "24")]
    #[serde(default)]
    pub notebook_id: Option<String>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct MemoSaveAck {
    #[prost(uint32, tag = "1")]
    pub protocol_version: u32,
    #[prost(uint64, tag = "2")]
    pub revision: u64,
    #[prost(string, tag = "3")]
    pub content_hash: String,
    #[prost(string, tag = "4")]
    pub etag: String,
    #[prost(string, optional, tag = "5")]
    pub lease_id: Option<String>,
    #[prost(string, tag = "6")]
    pub saved_at: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct MemoListResponse {
    #[prost(message, repeated, tag = "1")]
    pub items: Vec<MemoSummary>,
    #[prost(string, optional, tag = "2")]
    pub next_cursor: Option<String>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct BatchGetRequest {
    #[prost(uint32, tag = "1")]
    pub protocol_version: u32,
    #[prost(string, repeated, tag = "2")]
    pub ids: Vec<String>,
    #[prost(bool, tag = "3")]
    pub include_content: bool,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct BatchGetResponse {
    #[prost(message, repeated, tag = "1")]
    pub items: Vec<MemoDetail>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct MemoInventoryItem {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub notebook_id: String,
    #[prost(string, tag = "3")]
    pub etag: String,
    #[prost(bool, tag = "4")]
    pub is_pinned: bool,
    #[prost(bool, tag = "5")]
    pub is_archived: bool,
    #[prost(bool, tag = "6")]
    pub is_deleted: bool,
    #[prost(string, tag = "7")]
    pub updated_at: String,
}

/// A standalone task. This is deliberately separate from Markdown checkboxes.
#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct Todo {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub title: String,
    #[prost(string, tag = "3")]
    pub note: String,
    #[prost(string, tag = "4")]
    pub status: String,
    #[prost(string, optional, tag = "5")]
    pub due_date: Option<String>,
    #[prost(int32, tag = "6")]
    pub priority: i32,
    #[prost(string, optional, tag = "7")]
    pub linked_memo_id: Option<String>,
    #[prost(bool, tag = "8")]
    pub is_deleted: bool,
    #[prost(uint64, tag = "9")]
    pub revision: u64,
    #[prost(string, tag = "10")]
    pub etag: String,
    #[prost(string, tag = "11")]
    pub created_at: String,
    #[prost(string, tag = "12")]
    pub updated_at: String,
    #[prost(string, optional, tag = "13")]
    pub completed_at: Option<String>,
    #[prost(string, optional, tag = "14")]
    pub deleted_at: Option<String>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct TodoPush {
    #[prost(message, optional, tag = "1")]
    pub todo: Option<Todo>,
    #[prost(uint64, optional, tag = "2")]
    pub base_revision: Option<u64>,
    #[prost(string, optional, tag = "3")]
    pub if_match_etag: Option<String>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct SyncInventoryResponse {
    #[prost(uint32, tag = "1")]
    pub protocol_version: u32,
    #[prost(uint64, tag = "2")]
    pub sync_epoch: u64,
    #[prost(uint64, tag = "3")]
    pub cursor: u64,
    #[prost(message, repeated, tag = "4")]
    pub notebooks: Vec<Notebook>,
    #[prost(message, repeated, tag = "5")]
    pub memos: Vec<MemoInventoryItem>,
    #[prost(bool, tag = "6")]
    pub has_more: bool,
    #[prost(string, optional, tag = "7")]
    pub next_memo_cursor: Option<String>,
    #[prost(message, repeated, tag = "8")]
    pub todos: Vec<Todo>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct SyncChange {
    #[prost(uint64, tag = "1")]
    pub id: u64,
    #[prost(string, tag = "2")]
    pub entity_type: String,
    #[prost(string, tag = "3")]
    pub entity_id: String,
    #[prost(string, tag = "4")]
    pub operation: String,
    #[prost(string, tag = "5")]
    pub payload_kind: String,
    #[prost(string, optional, tag = "6")]
    pub payload_json: Option<String>,
    #[prost(string, tag = "7")]
    pub created_at: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct SyncPullResponse {
    #[prost(uint32, tag = "1")]
    pub protocol_version: u32,
    #[prost(uint64, tag = "2")]
    pub sync_epoch: u64,
    #[prost(uint64, tag = "3")]
    pub cursor: u64,
    #[prost(message, repeated, tag = "4")]
    pub changes: Vec<SyncChange>,
    #[prost(bool, tag = "5")]
    pub has_more: bool,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct SyncPushItem {
    #[prost(string, tag = "1")]
    pub client_op_id: String,
    #[prost(string, tag = "2")]
    pub op: String,
    #[prost(string, optional, tag = "3")]
    pub memo_id: Option<String>,
    #[prost(message, optional, tag = "4")]
    pub save: Option<MemoSaveRequest>,
    #[prost(string, optional, tag = "5")]
    pub notebook_id: Option<String>,
    #[prost(string, optional, tag = "6")]
    pub title: Option<String>,
    #[prost(string, optional, tag = "7")]
    pub markdown: Option<String>,
    #[prost(message, optional, tag = "8")]
    pub todo: Option<TodoPush>,
    /// Previous operation in the same entity's local journal. The server does
    /// not need to interpret this value; clients use it to preserve ordering.
    #[prost(string, optional, tag = "9")]
    pub depends_on_client_op_id: Option<String>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct SyncPushRequest {
    #[prost(uint32, tag = "1")]
    pub protocol_version: u32,
    #[prost(string, tag = "2")]
    pub device_id: String,
    #[prost(message, repeated, tag = "3")]
    pub items: Vec<SyncPushItem>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct SyncPushItemResult {
    #[prost(string, tag = "1")]
    pub client_op_id: String,
    #[prost(bool, tag = "2")]
    pub ok: bool,
    #[prost(string, optional, tag = "3")]
    pub error_code: Option<String>,
    #[prost(message, optional, tag = "4")]
    pub save_ack: Option<MemoSaveAck>,
    #[prost(string, optional, tag = "5")]
    pub memo_id: Option<String>,
    #[prost(message, optional, tag = "6")]
    pub todo: Option<Todo>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct SyncPushResponse {
    #[prost(message, repeated, tag = "1")]
    pub results: Vec<SyncPushItemResult>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct SyncWsAuth {
    #[prost(string, tag = "1")]
    pub bearer: String,
    #[prost(string, tag = "2")]
    pub device_id: String,
    #[prost(uint64, tag = "3")]
    pub cursor: u64,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct SyncHello {
    #[prost(uint32, tag = "1")]
    pub protocol_version: u32,
    #[prost(uint64, tag = "2")]
    pub sync_epoch: u64,
    #[prost(uint64, tag = "3")]
    pub cursor: u64,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct SyncHint {
    #[prost(uint64, tag = "1")]
    pub cursor: u64,
    #[prost(uint64, tag = "2")]
    pub sync_epoch: u64,
    #[prost(string, repeated, tag = "3")]
    pub kinds: Vec<String>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct LoginRequest {
    #[prost(string, tag = "1")]
    pub username: String,
    #[prost(string, tag = "2")]
    pub password: String,
    #[prost(string, optional, tag = "3")]
    pub device_id: Option<String>,
    /// Optional workspace to enter. Omitted keeps the existing primary-workspace behavior.
    #[prost(string, optional, tag = "4")]
    pub workspace_id: Option<String>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct AuthUser {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub username: String,
    #[prost(bool, tag = "3")]
    pub is_owner: bool,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct LoginResponse {
    #[prost(message, optional, tag = "1")]
    pub user: Option<AuthUser>,
    #[prost(string, tag = "2")]
    pub session_token: String,
    #[prost(string, tag = "3")]
    pub workspace_id: String,
    #[prost(string, tag = "4")]
    pub expires_at: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct SessionResponse {
    #[prost(message, optional, tag = "1")]
    pub user: Option<AuthUser>,
    #[prost(string, optional, tag = "2")]
    pub workspace_id: Option<String>,
    #[prost(bool, tag = "3")]
    pub authenticated: bool,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct HealthResponse {
    #[prost(bool, tag = "1")]
    pub ok: bool,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(string, tag = "3")]
    pub version: String,
    #[prost(uint32, tag = "4")]
    pub protocol_version: u32,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct CreateMemoRequest {
    #[prost(uint32, tag = "1")]
    pub protocol_version: u32,
    #[prost(string, tag = "2")]
    pub notebook_id: String,
    #[prost(string, optional, tag = "3")]
    pub title: Option<String>,
    #[prost(string, tag = "4")]
    pub markdown: String,
    #[prost(string, repeated, tag = "5")]
    pub tags: Vec<String>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct CreateNotebookRequest {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, optional, tag = "2")]
    pub parent_id: Option<String>,
    #[prost(int32, optional, tag = "3")]
    pub sort_order: Option<i32>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct RenameNotebookRequest {
    #[prost(string, tag = "1")]
    pub name: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct ReorderNotebooksRequest {
    #[prost(string, optional, tag = "1")]
    pub parent_id: Option<String>,
    #[prost(string, repeated, tag = "2")]
    pub notebook_ids: Vec<String>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct NotebookListResponse {
    #[prost(message, repeated, tag = "1")]
    pub items: Vec<Notebook>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct Resource {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, optional, tag = "2")]
    pub memo_id: Option<String>,
    #[prost(string, tag = "3")]
    pub content_hash: String,
    #[prost(string, tag = "4")]
    pub kind: String,
    #[prost(string, optional, tag = "5")]
    pub mime_type: Option<String>,
    #[prost(string, optional, tag = "6")]
    pub filename: Option<String>,
    #[prost(uint64, tag = "7")]
    pub byte_size: u64,
    #[prost(string, tag = "8")]
    pub url: String,
    #[prost(string, tag = "9")]
    pub created_at: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct MemoRevision {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub memo_id: String,
    #[prost(uint64, tag = "3")]
    pub revision: u64,
    #[prost(string, optional, tag = "4")]
    pub title: Option<String>,
    #[prost(string, repeated, tag = "5")]
    pub tags: Vec<String>,
    #[prost(string, tag = "6")]
    pub content_hash: String,
    #[prost(string, tag = "7")]
    pub created_at: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct RevisionListResponse {
    #[prost(message, repeated, tag = "1")]
    pub items: Vec<MemoRevision>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct TagSummary {
    #[prost(string, tag = "1")]
    pub tag: String,
    #[prost(int64, tag = "2")]
    pub memo_count: i64,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct TagListResponse {
    #[prost(message, repeated, tag = "1")]
    pub items: Vec<TagSummary>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct BatchMoveRequest {
    #[prost(string, repeated, tag = "1")]
    pub memo_ids: Vec<String>,
    #[prost(string, tag = "2")]
    pub notebook_id: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct BatchDeleteRequest {
    #[prost(string, repeated, tag = "1")]
    pub memo_ids: Vec<String>,
    #[prost(bool, tag = "2")]
    pub permanent: bool,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct MergeMemosRequest {
    #[prost(string, repeated, tag = "1")]
    pub memo_ids: Vec<String>,
    #[prost(string, tag = "2")]
    pub notebook_id: String,
    #[prost(string, optional, tag = "3")]
    pub title: Option<String>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct ApiTokenInfo {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(string, repeated, tag = "3")]
    pub scopes: Vec<String>,
    #[prost(string, optional, tag = "4")]
    pub token: Option<String>,
    #[prost(string, optional, tag = "5")]
    pub expires_at: Option<String>,
    #[prost(string, tag = "6")]
    pub created_at: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct CreateApiTokenRequest {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, repeated, tag = "2")]
    pub scopes: Vec<String>,
    #[prost(string, optional, tag = "3")]
    pub expires_at: Option<String>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct ApiTokenListResponse {
    #[prost(message, repeated, tag = "1")]
    pub items: Vec<ApiTokenInfo>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct ChangePasswordRequest {
    #[prost(string, tag = "1")]
    pub current_password: String,
    #[prost(string, tag = "2")]
    pub new_password: String,
}
