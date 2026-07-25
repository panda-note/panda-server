mod auth_routes;
mod memo_routes;
mod misc_routes;
mod sync_routes;
mod todo_routes;

use crate::state::AppState;
use axum::routing::{delete, get, patch, post};
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

pub fn router(state: AppState) -> Router {
    let limit = state.cfg.limits.max_request_body_bytes;
    Router::new()
        .route("/api/v1/health", get(misc_routes::health))
        .route("/api/v1/openapi.yaml", get(misc_routes::openapi))
        .route("/api/health", get(misc_routes::health))
        .route("/api/v1/auth/login", post(auth_routes::login))
        .route("/api/v1/auth/register", post(auth_routes::register))
        .route("/api/v1/auth/logout", post(auth_routes::logout))
        .route("/api/v1/auth/session", get(auth_routes::session))
        .route("/api/v1/workspaces", get(auth_routes::list_workspaces))
        .route(
            "/api/v1/auth/change-password",
            post(auth_routes::change_password),
        )
        .route(
            "/api/v1/notebooks",
            get(misc_routes::list_notebooks).post(misc_routes::create_notebook),
        )
        .route(
            "/api/v1/notebooks/{id}",
            patch(misc_routes::rename_notebook).delete(misc_routes::delete_notebook),
        )
        .route(
            "/api/v1/memos",
            get(memo_routes::list_memos).post(memo_routes::create_memo),
        )
        .route(
            "/api/v1/todos",
            get(todo_routes::list_todos).post(todo_routes::create_todo),
        )
        .route(
            "/api/v1/todos/{id}",
            get(todo_routes::get_todo)
                .patch(todo_routes::update_todo)
                .delete(todo_routes::delete_todo),
        )
        .route(
            "/api/v1/todos/{id}/complete",
            post(todo_routes::complete_todo),
        )
        .route(
            "/api/v1/todos/{id}/restore",
            post(todo_routes::restore_todo),
        )
        .route("/api/v1/memos/{id}/open", post(memo_routes::open_memo))
        .route("/api/v1/memos/{id}/save", post(memo_routes::save_memo))
        .route("/api/v1/memos/{id}", delete(memo_routes::delete_memo))
        .route(
            "/api/v1/memos/{id}/restore",
            post(memo_routes::restore_memo),
        )
        .route(
            "/api/v1/memos/{id}/revisions",
            get(memo_routes::list_revisions),
        )
        .route(
            "/api/v1/memos/{id}/revisions/{revision_id}/restore",
            post(memo_routes::restore_revision),
        )
        .route("/api/v1/memos/batch-get", post(memo_routes::batch_get))
        .route("/api/v1/memos/batch/move", post(memo_routes::batch_move))
        .route(
            "/api/v1/memos/batch/delete",
            post(memo_routes::batch_delete),
        )
        .route("/api/v1/memos/merge", post(memo_routes::merge_memos))
        .route("/api/v1/contents/{hash}", get(memo_routes::get_content))
        .route("/api/v1/tags", get(misc_routes::list_tags))
        .route("/api/v1/sync/inventory", get(sync_routes::inventory))
        .route("/api/v1/sync/pull", get(sync_routes::pull))
        .route("/api/v1/sync/push", post(sync_routes::push))
        .route("/api/v1/sync/ws", get(sync_routes::ws_handler))
        .route("/api/v1/resources", get(misc_routes::list_resources))
        .route(
            "/api/v1/memos/{id}/resources",
            post(misc_routes::upload_resource),
        )
        .route(
            "/api/v1/resources/{id}/blob",
            get(misc_routes::download_resource),
        )
        .route(
            "/api/v1/api-tokens",
            get(auth_routes::list_tokens).post(auth_routes::create_token),
        )
        .route("/api/v1/api-tokens/{id}", delete(auth_routes::delete_token))
        .route("/mcp", post(misc_routes::mcp_rpc))
        .route("/api/v1/mcp", post(misc_routes::mcp_rpc))
        .route("/api/v1/mcp/info", get(misc_routes::mcp_info))
        .route(
            "/api/v1/exports/markdown.zip",
            get(misc_routes::export_markdown_zip),
        )
        .route("/api/v1/metrics", get(misc_routes::metrics))
        .route("/api/v1/admin/compact", post(sync_routes::admin_compact))
        .layer(RequestBodyLimitLayer::new(limit))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
