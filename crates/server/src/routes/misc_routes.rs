use crate::error::{proto_or_json, read_body_proto_or_json, wants_protobuf, AppError};
use crate::extract::{AuthUser, OptionalAuth};
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{Multipart, Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use domain::PandaError;
use proto::{
    CreateNotebookRequest, HealthResponse, NotebookListResponse, RenameNotebookRequest,
    TagListResponse, PROTOCOL_VERSION,
};

pub async fn health(headers: HeaderMap) -> impl IntoResponse {
    let resp = HealthResponse {
        ok: true,
        name: "panda".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        protocol_version: PROTOCOL_VERSION,
    };
    proto_or_json(wants_protobuf(&headers), &resp)
}

/// Serves the same checked-in contract used by SDKs and desktop clients.
pub async fn openapi() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/yaml; charset=utf-8"),
        )],
        include_str!("../../../../openapi.yaml"),
    )
}

pub async fn list_notebooks(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let items = state.store.notebooks().list(&ctx.workspace_id).await?;
    Ok(proto_or_json(
        wants_protobuf(&headers),
        &NotebookListResponse { items },
    ))
}

pub async fn create_notebook(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let req: CreateNotebookRequest = read_body_proto_or_json(&headers, body).await?;
    let nb = state
        .store
        .notebooks()
        .create(
            &ctx.workspace_id,
            &req.name,
            req.parent_id.as_deref(),
            req.sort_order.unwrap_or(0),
        )
        .await?;
    Ok(proto_or_json(wants_protobuf(&headers), &nb))
}

pub async fn rename_notebook(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let req: RenameNotebookRequest = read_body_proto_or_json(&headers, body).await?;
    let nb = state
        .store
        .notebooks()
        .rename(&ctx.workspace_id, &id, &req.name)
        .await?;
    Ok(proto_or_json(wants_protobuf(&headers), &nb))
}

pub async fn delete_notebook(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    state
        .store
        .notebooks()
        .soft_delete(&ctx.workspace_id, &id)
        .await?;
    Ok(axum::Json(serde_json::json!({"ok": true})))
}

pub async fn list_tags(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let items = state.store.memos().list_tags(&ctx.workspace_id).await?;
    Ok(proto_or_json(
        wants_protobuf(&headers),
        &TagListResponse { items },
    ))
}

pub async fn list_resources(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
) -> Result<impl IntoResponse, AppError> {
    let items = state.store.resources().list(&ctx.workspace_id, 200).await?;
    Ok(axum::Json(serde_json::json!({ "items": items })))
}

pub async fn upload_resource(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    Path(memo_id): Path<String>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    if state
        .store
        .memos()
        .get_summary(&ctx.workspace_id, &memo_id)
        .await?
        .is_none()
    {
        return Err(PandaError::not_found("memo not found in workspace").into());
    }
    let mut file_bytes = None;
    let mut filename = None;
    let mut mime = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| PandaError::invalid(e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" || name.is_empty() {
            filename = field.file_name().map(|s| s.to_string());
            mime = field.content_type().map(|s| s.to_string());
            file_bytes = Some(
                field
                    .bytes()
                    .await
                    .map_err(|e| PandaError::invalid(e.to_string()))?
                    .to_vec(),
            );
        }
    }
    let data = file_bytes.ok_or_else(|| PandaError::invalid("file required"))?;
    let (hash, size) = state.blobs.put(&data).await?;
    let kind = if mime.as_deref().is_some_and(|m| m.starts_with("image/")) {
        "image"
    } else {
        "attachment"
    };
    let res = state
        .store
        .resources()
        .create(
            &ctx.workspace_id,
            Some(&memo_id),
            &hash,
            kind,
            mime.as_deref(),
            filename.as_deref(),
            size,
        )
        .await?;
    Ok(axum::Json(res))
}

pub async fn download_resource(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let res = state
        .store
        .resources()
        .get(&ctx.workspace_id, &id)
        .await?
        .ok_or_else(|| PandaError::not_found("resource"))?;
    let etag = format!("\"{}\"", res.content_hash);
    if let Some(inm) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    {
        if inm == etag || inm == res.content_hash {
            return Ok(StatusCode::NOT_MODIFIED.into_response());
        }
    }
    let data = state.blobs.get(&res.content_hash).await?;
    let ctype = res
        .mime_type
        .unwrap_or_else(|| "application/octet-stream".into());
    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_str(&ctype)
                    .unwrap_or(HeaderValue::from_static("application/octet-stream")),
            ),
            (
                header::ETAG,
                HeaderValue::from_str(&etag).unwrap_or(HeaderValue::from_static("\"\"")),
            ),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, max-age=31536000, immutable"),
            ),
        ],
        data,
    )
        .into_response())
}

pub async fn export_markdown_zip(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
) -> Result<Response, AppError> {
    let bytes =
        crate::export::export_workspace_markdown_zip(&state.store, &state.blobs, &ctx.workspace_id)
            .await?;
    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/zip"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=\"panda-export.zip\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

pub async fn mcp_info() -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "name": "panda",
        "transport": "streamable-http",
        "protocol_versions": mcp::SUPPORTED_PROTOCOL_VERSIONS,
        "endpoints": ["/mcp", "/api/v1/mcp"],
        "auth": "Bearer API token or session",
        "recommended_scopes": ["memos:read", "memos:write", "notebooks:read", "notebooks:write"]
    }))
}

pub async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; version=0.0.4"),
        )],
        state.metrics.render_prometheus(),
    )
}

pub async fn mcp_rpc(
    State(state): State<AppState>,
    OptionalAuth(auth): OptionalAuth,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let ctx =
        auth.ok_or_else(|| AppError(PandaError::unauthenticated("MCP requires Bearer auth")))?;
    let value: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => {
            return Ok(axum::Json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": { "code": -32700, "message": format!("parse error: {error}") }
            }))
            .into_response());
        }
    };
    let resp = state.mcp.handle(&ctx, value).await;
    if resp.is_null() {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    Ok(axum::Json(resp).into_response())
}

#[cfg(test)]
mod tests {
    use super::{mcp_info, openapi};
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn openapi_contract_is_served_as_yaml_and_describes_token_auth() {
        let response = openapi().await.into_response();
        assert_eq!(
            response.headers()["content-type"],
            "application/yaml; charset=utf-8"
        );
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = std::str::from_utf8(&bytes).unwrap();
        assert!(body.contains("openapi: 3.1.0"));
        assert!(body.contains("bearerAuth"));
        assert!(body.contains("/memos/{id}/save"));
        assert!(body.contains("/api/v1/mcp"));
    }

    #[tokio::test]
    async fn mcp_metadata_advertises_streamable_http_endpoints() {
        let response = mcp_info().await.into_response();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = std::str::from_utf8(&bytes).unwrap();
        assert!(body.contains("streamable-http"));
        assert!(body.contains("/mcp"));
        assert!(body.contains(mcp::LATEST_PROTOCOL_VERSION));
    }
}
