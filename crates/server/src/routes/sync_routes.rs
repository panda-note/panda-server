use crate::error::{proto_or_json, read_body_proto_or_json, wants_protobuf, AppError};
use crate::extract::AuthUser;
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use domain::{new_id, PandaError};
use futures::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use proto::{
    SyncHello, SyncPullResponse, SyncPushItemResult, SyncPushRequest, SyncPushResponse, SyncWsAuth,
    PROTOCOL_VERSION,
};
use serde::Deserialize;
use store::{TodoCreate, TodoUpdate};
use sync::hint_after_change;

#[derive(Deserialize)]
pub struct InventoryQuery {
    pub after: Option<String>,
    pub limit: Option<i64>,
    pub device_id: Option<String>,
}

#[derive(Deserialize)]
pub struct PullQuery {
    pub cursor: Option<u64>,
    pub limit: Option<i64>,
    pub device_id: Option<String>,
}

pub async fn inventory(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
    Query(q): Query<InventoryQuery>,
) -> Result<impl IntoResponse, AppError> {
    let resp = state
        .store
        .sync()
        .inventory(
            &ctx.workspace_id,
            q.after.as_deref(),
            q.limit.unwrap_or(500),
        )
        .await?;
    if let Some(device) = q.device_id.as_deref().or(ctx.device_id.as_deref()) {
        state
            .store
            .sync()
            .upsert_device_cursor(&ctx.workspace_id, device, &ctx.user_id, resp.cursor)
            .await?;
    }
    Ok(proto_or_json(wants_protobuf(&headers), &resp))
}

pub async fn pull(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
    Query(q): Query<PullQuery>,
) -> Result<impl IntoResponse, AppError> {
    let after = q.cursor.unwrap_or(0);
    let (changes, cursor, has_more) = state
        .store
        .sync()
        .pull(&ctx.workspace_id, after, q.limit.unwrap_or(100))
        .await?;
    let sync_epoch = state.store.sync().epoch(&ctx.workspace_id).await?;
    if let Some(device) = q.device_id.as_deref().or(ctx.device_id.as_deref()) {
        state
            .store
            .sync()
            .upsert_device_cursor(&ctx.workspace_id, device, &ctx.user_id, cursor)
            .await?;
    }
    state.metrics.inc_pulls();
    Ok(proto_or_json(
        wants_protobuf(&headers),
        &SyncPullResponse {
            protocol_version: PROTOCOL_VERSION,
            sync_epoch,
            cursor,
            changes,
            has_more,
        },
    ))
}

pub async fn push(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let req: SyncPushRequest = read_body_proto_or_json(&headers, body).await?;
    if req.items.len() > state.cfg.limits.max_batch_size {
        return Err(AppError(PandaError::invalid("push batch too large")));
    }
    let mut results = Vec::new();
    for item in req.items {
        let result = match item.op.as_str() {
            "memo.create" => {
                let notebook = item.notebook_id.clone().unwrap_or_default();
                let notebook = if notebook.is_empty() {
                    state
                        .store
                        .notebooks()
                        .default_inbox_id(&ctx.workspace_id)
                        .await?
                } else {
                    notebook
                };
                let md = item.markdown.unwrap_or_default();
                let blobs = state.blobs.clone();
                match state
                    .store
                    .memos()
                    .create(
                        &ctx.workspace_id,
                        &ctx.user_id,
                        &notebook,
                        item.title,
                        &md,
                        &[],
                        move |bytes| {
                            let blobs = blobs.clone();
                            let data = bytes.to_vec();
                            Box::pin(async move { blobs.put(&data).await })
                        },
                    )
                    .await
                {
                    Ok(detail) => SyncPushItemResult {
                        client_op_id: item.client_op_id,
                        ok: true,
                        error_code: None,
                        save_ack: None,
                        memo_id: detail.summary.map(|s| s.id),
                        todo: None,
                    },
                    Err(e) => SyncPushItemResult {
                        client_op_id: item.client_op_id,
                        ok: false,
                        error_code: Some(e.code.as_str().to_string()),
                        save_ack: None,
                        memo_id: None,
                        todo: None,
                    },
                }
            }
            "memo.update" => {
                let Some(memo_id) = item.memo_id.clone() else {
                    results.push(SyncPushItemResult {
                        client_op_id: item.client_op_id,
                        ok: false,
                        error_code: Some("invalid_argument".into()),
                        save_ack: None,
                        memo_id: None,
                        todo: None,
                    });
                    continue;
                };
                let Some(save) = item.save else {
                    results.push(SyncPushItemResult {
                        client_op_id: item.client_op_id,
                        ok: false,
                        error_code: Some("invalid_argument".into()),
                        save_ack: None,
                        memo_id: Some(memo_id),
                        todo: None,
                    });
                    continue;
                };
                let blobs = state.blobs.clone();
                let blobs2 = state.blobs.clone();
                let tags = if save.tags.is_empty() {
                    None
                } else {
                    Some(save.tags.clone())
                };
                match state
                    .store
                    .memos()
                    .save(
                        &ctx.workspace_id,
                        &ctx.user_id,
                        &memo_id,
                        save.if_match_etag.as_deref(),
                        save.base_revision,
                        save.base_content_hash.as_deref(),
                        save.lease_id.as_deref(),
                        &save.idempotency_key,
                        save.body,
                        save.title,
                        tags,
                        save.is_pinned,
                        save.is_archived,
                        save.notebook_id,
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
                    .await
                {
                    Ok(ack) => SyncPushItemResult {
                        client_op_id: item.client_op_id,
                        ok: true,
                        error_code: None,
                        save_ack: Some(ack),
                        memo_id: Some(memo_id),
                        todo: None,
                    },
                    Err(e) => SyncPushItemResult {
                        client_op_id: item.client_op_id,
                        ok: false,
                        error_code: Some(e.code.as_str().to_string()),
                        save_ack: None,
                        memo_id: Some(memo_id),
                        todo: None,
                    },
                }
            }
            "memo.delete" => {
                let Some(memo_id) = item.memo_id else {
                    results.push(SyncPushItemResult {
                        client_op_id: item.client_op_id,
                        ok: false,
                        error_code: Some("invalid_argument".into()),
                        save_ack: None,
                        memo_id: None,
                        todo: None,
                    });
                    continue;
                };
                match state
                    .store
                    .memos()
                    .soft_delete(&ctx.workspace_id, &memo_id, false)
                    .await
                {
                    Ok(()) => SyncPushItemResult {
                        client_op_id: item.client_op_id,
                        ok: true,
                        error_code: None,
                        save_ack: None,
                        memo_id: Some(memo_id),
                        todo: None,
                    },
                    Err(e) => SyncPushItemResult {
                        client_op_id: item.client_op_id,
                        ok: false,
                        error_code: Some(e.code.as_str().to_string()),
                        save_ack: None,
                        memo_id: Some(memo_id),
                        todo: None,
                    },
                }
            }
            "todo.create" => {
                let Some(push) = item.todo else {
                    return Err(AppError(PandaError::invalid("todo payload required")));
                };
                let Some(todo) = push.todo else {
                    return Err(AppError(PandaError::invalid("todo payload required")));
                };
                match state
                    .store
                    .todos()
                    .create(
                        &ctx.workspace_id,
                        TodoCreate {
                            title: todo.title,
                            note: todo.note,
                            status: Some(todo.status),
                            due_date: todo.due_date,
                            priority: Some(todo.priority as i64),
                            linked_memo_id: todo.linked_memo_id,
                        },
                    )
                    .await
                {
                    Ok(todo) => SyncPushItemResult {
                        client_op_id: item.client_op_id,
                        ok: true,
                        error_code: None,
                        save_ack: None,
                        memo_id: None,
                        todo: Some(todo.to_proto()),
                    },
                    Err(e) => SyncPushItemResult {
                        client_op_id: item.client_op_id,
                        ok: false,
                        error_code: Some(e.code.as_str().to_string()),
                        save_ack: None,
                        memo_id: None,
                        todo: None,
                    },
                }
            }
            "todo.update" | "todo.complete" => {
                let Some(push) = item.todo else {
                    return Err(AppError(PandaError::invalid("todo payload required")));
                };
                let Some(todo) = push.todo else {
                    return Err(AppError(PandaError::invalid("todo payload required")));
                };
                match state
                    .store
                    .todos()
                    .update(
                        &ctx.workspace_id,
                        &todo.id,
                        TodoUpdate {
                            title: Some(todo.title),
                            note: Some(todo.note),
                            status: Some(todo.status),
                            due_date: Some(todo.due_date),
                            priority: Some(todo.priority as i64),
                            linked_memo_id: Some(todo.linked_memo_id),
                            base_revision: push.base_revision.map(|v| v as i64),
                            if_match_etag: push.if_match_etag,
                        },
                    )
                    .await
                {
                    Ok(todo) => SyncPushItemResult {
                        client_op_id: item.client_op_id,
                        ok: true,
                        error_code: None,
                        save_ack: None,
                        memo_id: None,
                        todo: Some(todo.to_proto()),
                    },
                    Err(e) => SyncPushItemResult {
                        client_op_id: item.client_op_id,
                        ok: false,
                        error_code: Some(e.code.as_str().to_string()),
                        save_ack: None,
                        memo_id: None,
                        todo: None,
                    },
                }
            }
            "todo.delete" => {
                let Some(push) = item.todo else {
                    return Err(AppError(PandaError::invalid("todo payload required")));
                };
                let Some(todo) = push.todo else {
                    return Err(AppError(PandaError::invalid("todo payload required")));
                };
                match state
                    .store
                    .todos()
                    .delete(
                        &ctx.workspace_id,
                        &todo.id,
                        push.base_revision.map(|v| v as i64),
                        push.if_match_etag.as_deref(),
                    )
                    .await
                {
                    Ok(()) => SyncPushItemResult {
                        client_op_id: item.client_op_id,
                        ok: true,
                        error_code: None,
                        save_ack: None,
                        memo_id: None,
                        todo: Some(todo),
                    },
                    Err(e) => SyncPushItemResult {
                        client_op_id: item.client_op_id,
                        ok: false,
                        error_code: Some(e.code.as_str().to_string()),
                        save_ack: None,
                        memo_id: None,
                        todo: None,
                    },
                }
            }
            _ => SyncPushItemResult {
                client_op_id: item.client_op_id,
                ok: false,
                error_code: Some("invalid_argument".into()),
                save_ack: None,
                memo_id: None,
                todo: None,
            },
        };
        results.push(result);
    }
    hint_after_change(
        state.bus.as_ref(),
        &state.store,
        &ctx.workspace_id,
        vec!["push".into()],
    )
    .await;
    let _ = new_id;
    Ok(proto_or_json(
        wants_protobuf(&headers),
        &SyncPushResponse { results },
    ))
}

pub async fn ws_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(state, socket))
}

async fn handle_ws(state: AppState, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    // First message must be SyncWsAuth (protobuf or JSON)
    let Some(Ok(first)) = stream.next().await else {
        return;
    };
    let auth = match first {
        Message::Binary(b) => SyncWsAuth::decode(b.as_ref()).ok(),
        Message::Text(t) => serde_json::from_str(&t).ok(),
        _ => None,
    };
    let Some(auth) = auth else {
        let _ = sink.send(Message::Text("auth required".into())).await;
        return;
    };
    let ctx = match state.auth.authenticate_bearer(&auth.bearer).await {
        Ok(c) => c,
        Err(_) => {
            let _ = sink.send(Message::Text("unauthorized".into())).await;
            return;
        }
    };
    let epoch = state
        .store
        .sync()
        .epoch(&ctx.workspace_id)
        .await
        .unwrap_or(1);
    let cursor = state
        .store
        .sync()
        .max_cursor(&ctx.workspace_id)
        .await
        .unwrap_or(auth.cursor);
    let hello = SyncHello {
        protocol_version: PROTOCOL_VERSION,
        sync_epoch: epoch,
        cursor,
    };
    let mut buf = Vec::new();
    if hello.encode(&mut buf).is_ok() {
        let _ = sink.send(Message::Binary(buf.into())).await;
    }
    if !auth.device_id.is_empty() {
        let _ = state
            .store
            .sync()
            .upsert_device_cursor(&ctx.workspace_id, &auth.device_id, &ctx.user_id, cursor)
            .await;
    }

    let mut rx = state.bus.subscribe(&ctx.workspace_id).await;
    loop {
        tokio::select! {
            msg = stream.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(p))) => { let _ = sink.send(Message::Pong(p)).await; }
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            hint = rx.recv() => {
                match hint {
                    Ok(h) => {
                        let mut buf = Vec::new();
                        if h.encode(&mut buf).is_ok() {
                            if sink.send(Message::Binary(buf.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        }
    }
}

pub async fn admin_compact(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
) -> Result<impl IntoResponse, AppError> {
    if !ctx.is_owner {
        return Err(AppError(PandaError::new(
            domain::ErrorCode::PermissionDenied,
            "owner only",
        )));
    }
    let n = state
        .store
        .sync()
        .compact(&ctx.workspace_id, state.cfg.sync.device_cursor_ttl_days)
        .await?;
    Ok(axum::Json(serde_json::json!({"deleted": n})))
}
