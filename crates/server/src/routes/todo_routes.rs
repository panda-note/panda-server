use crate::error::AppError;
use crate::extract::AuthUser;
use crate::state::AppState;
use auth::AuthService;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use store::{TodoCreate, TodoListQuery, TodoUpdate};
use sync::hint_after_change;

#[derive(Deserialize)]
pub struct ListQuery {
    pub filter: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}
#[derive(Deserialize)]
pub struct CompleteRequest {
    pub completed: Option<bool>,
    pub base_revision: Option<i64>,
    pub if_match_etag: Option<String>,
}
#[derive(Deserialize)]
pub struct DeleteQuery {
    pub base_revision: Option<i64>,
    pub if_match_etag: Option<String>,
    #[serde(default)]
    pub permanent: bool,
}
#[derive(Deserialize)]
pub struct RestoreRequest {
    pub base_revision: Option<i64>,
    pub if_match_etag: Option<String>,
}

pub async fn list_todos(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    Query(q): Query<ListQuery>,
) -> Result<impl IntoResponse, AppError> {
    AuthService::require_scope(&ctx, "memos:read")?;
    let (items, next_cursor) = state
        .store
        .todos()
        .list(
            &ctx.workspace_id,
            TodoListQuery {
                filter: q.filter,
                limit: q.limit.unwrap_or(100),
                cursor: q.cursor,
            },
        )
        .await?;
    Ok(Json(
        serde_json::json!({"items":items,"next_cursor":next_cursor}),
    ))
}
pub async fn get_todo(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    AuthService::require_scope(&ctx, "memos:read")?;
    Ok(Json(state.store.todos().get(&ctx.workspace_id, &id).await?))
}
pub async fn create_todo(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    Json(req): Json<TodoCreate>,
) -> Result<impl IntoResponse, AppError> {
    AuthService::require_scope(&ctx, "memos:write")?;
    let todo = state.store.todos().create(&ctx.workspace_id, req).await?;
    hint_after_change(
        state.bus.as_ref(),
        &state.store,
        &ctx.workspace_id,
        vec!["todo".into()],
    )
    .await;
    Ok(Json(todo))
}
pub async fn update_todo(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    Path(id): Path<String>,
    Json(req): Json<TodoUpdate>,
) -> Result<impl IntoResponse, AppError> {
    AuthService::require_scope(&ctx, "memos:write")?;
    let todo = state
        .store
        .todos()
        .update(&ctx.workspace_id, &id, req)
        .await?;
    hint_after_change(
        state.bus.as_ref(),
        &state.store,
        &ctx.workspace_id,
        vec!["todo".into()],
    )
    .await;
    Ok(Json(todo))
}
pub async fn complete_todo(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    Path(id): Path<String>,
    Json(req): Json<CompleteRequest>,
) -> Result<impl IntoResponse, AppError> {
    AuthService::require_scope(&ctx, "memos:write")?;
    let todo = state
        .store
        .todos()
        .update(
            &ctx.workspace_id,
            &id,
            TodoUpdate {
                title: None,
                note: None,
                status: Some(if req.completed.unwrap_or(true) {
                    "completed".into()
                } else {
                    "open".into()
                }),
                due_date: None,
                priority: None,
                linked_memo_id: None,
                base_revision: req.base_revision,
                if_match_etag: req.if_match_etag,
            },
        )
        .await?;
    hint_after_change(
        state.bus.as_ref(),
        &state.store,
        &ctx.workspace_id,
        vec!["todo".into()],
    )
    .await;
    Ok(Json(todo))
}
pub async fn delete_todo(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    Path(id): Path<String>,
    Query(q): Query<DeleteQuery>,
) -> Result<impl IntoResponse, AppError> {
    AuthService::require_scope(&ctx, "memos:write")?;
    state
        .store
        .todos()
        .delete(
            &ctx.workspace_id,
            &id,
            q.base_revision,
            q.if_match_etag.as_deref(),
            q.permanent,
        )
        .await?;
    hint_after_change(
        state.bus.as_ref(),
        &state.store,
        &ctx.workspace_id,
        vec!["todo".into()],
    )
    .await;
    Ok(Json(serde_json::json!({"ok":true})))
}
pub async fn restore_todo(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    Path(id): Path<String>,
    Json(req): Json<RestoreRequest>,
) -> Result<impl IntoResponse, AppError> {
    AuthService::require_scope(&ctx, "memos:write")?;
    let todo = state
        .store
        .todos()
        .restore(
            &ctx.workspace_id,
            &id,
            req.base_revision,
            req.if_match_etag.as_deref(),
        )
        .await?;
    hint_after_change(
        state.bus.as_ref(),
        &state.store,
        &ctx.workspace_id,
        vec!["todo".into()],
    )
    .await;
    Ok(Json(todo))
}
