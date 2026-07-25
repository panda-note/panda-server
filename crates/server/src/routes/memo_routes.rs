use crate::error::{proto_or_json, read_body_proto_or_json, wants_protobuf, AppError};
use crate::extract::AuthUser;
use crate::state::AppState;
use auth::AuthService;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use domain::{new_id, PandaError};
use proto::{
    BatchDeleteRequest, BatchGetRequest, BatchGetResponse, BatchMoveRequest, CreateMemoRequest,
    MemoListResponse, MemoOpenRequest, MemoOpenResponse, MergeMemosRequest, RevisionListResponse,
    SaveBody, PROTOCOL_VERSION,
};
use serde::Deserialize;
use store::MemoListQuery;
use sync::hint_after_change;

#[derive(Deserialize)]
pub struct ListQuery {
    pub notebook_id: Option<String>,
    pub trash: Option<bool>,
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

pub async fn list_memos(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Result<impl IntoResponse, AppError> {
    AuthService::require_scope(&ctx, "memos:read")?;
    let (items, next) = state
        .store
        .memos()
        .list(
            &ctx.workspace_id,
            MemoListQuery {
                notebook_id: q.notebook_id,
                trash: q.trash.unwrap_or(false),
                q: q.q,
                limit: q.limit.unwrap_or(50),
                cursor: q.cursor,
            },
        )
        .await?;
    Ok(proto_or_json(
        wants_protobuf(&headers),
        &MemoListResponse {
            items,
            next_cursor: next,
        },
    ))
}

pub async fn create_memo(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    AuthService::require_scope(&ctx, "memos:write")?;
    let req: CreateMemoRequest = read_body_proto_or_json(&headers, body).await?;
    let blobs = state.blobs.clone();
    let detail = state
        .store
        .memos()
        .create(
            &ctx.workspace_id,
            &ctx.user_id,
            &req.notebook_id,
            req.title,
            &req.markdown,
            &req.tags,
            move |bytes| {
                let blobs = blobs.clone();
                let data = bytes.to_vec();
                Box::pin(async move { blobs.put(&data).await })
            },
        )
        .await?;
    hint_after_change(
        state.bus.as_ref(),
        &state.store,
        &ctx.workspace_id,
        vec!["memo.content".into()],
    )
    .await;
    Ok(proto_or_json(wants_protobuf(&headers), &detail))
}

pub async fn open_memo(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    AuthService::require_scope(&ctx, "memos:read")?;
    let req: MemoOpenRequest = if body.is_empty() {
        MemoOpenRequest {
            protocol_version: PROTOCOL_VERSION,
            lease_mode: "soft".into(),
        }
    } else {
        read_body_proto_or_json(&headers, body).await?
    };
    if req.protocol_version != 0 && req.protocol_version != PROTOCOL_VERSION {
        return Err(AppError(PandaError::invalid(format!(
            "unsupported protocol_version {}",
            req.protocol_version
        ))));
    }
    let mode = if req.lease_mode.is_empty() {
        "soft"
    } else {
        req.lease_mode.as_str()
    };
    let (memo, etag, lease) = state
        .store
        .memos()
        .open(&ctx.workspace_id, &id, &ctx.user_id, mode)
        .await?;
    let resp = MemoOpenResponse {
        protocol_version: PROTOCOL_VERSION,
        memo: Some(memo),
        etag,
        lease,
    };
    Ok(proto_or_json(wants_protobuf(&headers), &resp))
}

pub async fn save_memo(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    AuthService::require_scope(&ctx, "memos:write")?;
    let req: proto::MemoSaveRequest = read_body_proto_or_json(&headers, body).await?;
    let blobs = state.blobs.clone();
    let blobs2 = state.blobs.clone();
    let tags = if req.tags.is_empty() {
        None
    } else {
        Some(req.tags.clone())
    };
    let ack = state
        .store
        .memos()
        .save(
            &ctx.workspace_id,
            &ctx.user_id,
            &id,
            req.if_match_etag.as_deref(),
            req.base_revision,
            req.base_content_hash.as_deref(),
            req.lease_id.as_deref(),
            &req.idempotency_key,
            req.body,
            req.title,
            tags,
            req.is_pinned,
            req.is_archived,
            req.notebook_id,
            move |bytes| {
                let blobs = blobs.clone();
                let data = bytes.to_vec();
                Box::pin(async move { blobs.put(&data).await })
            },
            move |h| {
                let blobs = blobs2.clone();
                Box::pin(async move { blobs.get(&h).await })
            },
        )
        .await?;
    hint_after_change(
        state.bus.as_ref(),
        &state.store,
        &ctx.workspace_id,
        vec!["memo".into()],
    )
    .await;
    state.metrics.inc_saves();
    Ok(proto_or_json(wants_protobuf(&headers), &ack))
}

pub async fn delete_memo(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    Path(id): Path<String>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    AuthService::require_scope(&ctx, "memos:write")?;
    let permanent = q.get("permanent").is_some_and(|v| v == "1" || v == "true");
    state
        .store
        .memos()
        .soft_delete(&ctx.workspace_id, &id, permanent)
        .await?;
    hint_after_change(
        state.bus.as_ref(),
        &state.store,
        &ctx.workspace_id,
        vec!["memo.meta".into()],
    )
    .await;
    Ok(axum::Json(serde_json::json!({"ok": true})))
}

pub async fn restore_memo(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    AuthService::require_scope(&ctx, "memos:write")?;
    state.store.memos().restore(&ctx.workspace_id, &id).await?;
    Ok(axum::Json(serde_json::json!({"ok": true})))
}

pub async fn batch_get(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    AuthService::require_scope(&ctx, "memos:read")?;
    let req: BatchGetRequest = read_body_proto_or_json(&headers, body).await?;
    if req.ids.len() > state.cfg.limits.max_batch_size {
        return Err(AppError(PandaError::invalid("batch too large")));
    }
    let mut items = Vec::new();
    for id in req.ids {
        if let Ok((memo, _, _)) = state
            .store
            .memos()
            .open(&ctx.workspace_id, &id, &ctx.user_id, "none")
            .await
        {
            if !req.include_content {
                items.push(proto::MemoDetail {
                    summary: memo.summary,
                    content: None,
                });
            } else {
                items.push(memo);
            }
        }
    }
    Ok(proto_or_json(
        wants_protobuf(&headers),
        &BatchGetResponse { items },
    ))
}

pub async fn get_content(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    Path(hash): Path<String>,
) -> Result<Response, AppError> {
    AuthService::require_scope(&ctx, "memos:read")?;
    // A global CAS hash is readable only when this workspace has a live reference to it.
    if !state
        .store
        .memos()
        .can_access_blob(&ctx.workspace_id, &hash)
        .await?
    {
        return Err(AppError(PandaError::not_found("content hash not found")));
    }
    let data = state.blobs.get(&hash).await?;
    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/markdown; charset=utf-8"),
            ),
            (
                header::ETAG,
                HeaderValue::from_str(&format!("\"{hash}\""))
                    .unwrap_or(HeaderValue::from_static("\"\"")),
            ),
        ],
        data,
    )
        .into_response())
}

pub async fn list_revisions(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let items = state
        .store
        .memos()
        .list_revisions(&ctx.workspace_id, &id, 50)
        .await?;
    Ok(proto_or_json(
        wants_protobuf(&headers),
        &RevisionListResponse { items },
    ))
}

pub async fn restore_revision(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
    Path((id, revision_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppError> {
    let blobs = state.blobs.clone();
    let blobs2 = state.blobs.clone();
    let ack = state
        .store
        .memos()
        .restore_revision(
            &ctx.workspace_id,
            &ctx.user_id,
            &id,
            &revision_id,
            move |bytes| {
                let blobs = blobs.clone();
                let data = bytes.to_vec();
                Box::pin(async move { blobs.put(&data).await })
            },
            move |h| {
                let blobs = blobs2.clone();
                Box::pin(async move { blobs.get(&h).await })
            },
        )
        .await?;
    Ok(proto_or_json(wants_protobuf(&headers), &ack))
}

pub async fn batch_move(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let req: BatchMoveRequest = read_body_proto_or_json(&headers, body).await?;
    if req.memo_ids.len() > state.cfg.limits.max_batch_size {
        return Err(AppError(PandaError::invalid("batch too large")));
    }
    let n = state
        .store
        .memos()
        .batch_move(&ctx.workspace_id, &req.memo_ids, &req.notebook_id)
        .await?;
    Ok(axum::Json(serde_json::json!({"moved": n})))
}

pub async fn batch_delete(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let req: BatchDeleteRequest = read_body_proto_or_json(&headers, body).await?;
    for id in &req.memo_ids {
        state
            .store
            .memos()
            .soft_delete(&ctx.workspace_id, id, req.permanent)
            .await?;
    }
    Ok(axum::Json(serde_json::json!({"ok": true})))
}

pub async fn merge_memos(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let req: MergeMemosRequest = read_body_proto_or_json(&headers, body).await?;
    let blobs = state.blobs.clone();
    let blobs2 = state.blobs.clone();
    let detail = state
        .store
        .memos()
        .merge(
            &ctx.workspace_id,
            &ctx.user_id,
            &req.memo_ids,
            &req.notebook_id,
            req.title,
            {
                let blobs = blobs.clone();
                move |bytes| {
                    let blobs = blobs.clone();
                    let data = bytes.to_vec();
                    Box::pin(async move { blobs.put(&data).await })
                }
            },
            move |h| {
                let blobs = blobs2.clone();
                Box::pin(async move { blobs.get(&h).await })
            },
        )
        .await?;
    let _ = new_id;
    let _ = SaveBody::MarkdownFull;
    Ok(proto_or_json(wants_protobuf(&headers), &detail))
}
