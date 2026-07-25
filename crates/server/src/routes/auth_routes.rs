use crate::error::{proto_or_json, read_body_proto_or_json, wants_protobuf, AppError};
use crate::extract::{AuthUser, OptionalAuth};
use crate::state::AppState;
use auth::AuthService;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use proto::{
    ApiTokenInfo, ApiTokenListResponse, AuthUser as ProtoUser, ChangePasswordRequest,
    CreateApiTokenRequest, LoginRequest, LoginResponse, SessionResponse, PROTOCOL_VERSION,
};

fn require_token_admin(ctx: &auth::AuthContext) -> Result<(), AppError> {
    if !ctx.is_owner || ctx.session_id.is_none() {
        return Err(domain::PandaError::new(
            domain::ErrorCode::PermissionDenied,
            "API token management requires a workspace owner session",
        )
        .into());
    }
    Ok(())
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let req: LoginRequest = read_body_proto_or_json(&headers, body).await?;
    let (user, token, ws, expires) = state
        .auth
        .login(
            &req.username,
            &req.password,
            req.device_id.as_deref(),
            req.workspace_id.as_deref(),
        )
        .await?;
    let resp = LoginResponse {
        user: Some(ProtoUser {
            id: user.id,
            username: user.username,
            is_owner: user.is_owner != 0,
        }),
        session_token: token,
        workspace_id: ws,
        expires_at: expires,
    };
    Ok(proto_or_json(wants_protobuf(&headers), &resp))
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    if let Some(h) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(token) = h
            .strip_prefix("Bearer ")
            .or_else(|| h.strip_prefix("bearer "))
        {
            state.auth.logout(token).await?;
        }
    }
    Ok(axum::Json(serde_json::json!({"ok": true})))
}

pub async fn session(OptionalAuth(auth): OptionalAuth, headers: HeaderMap) -> impl IntoResponse {
    let resp = match auth {
        Some(ctx) => SessionResponse {
            user: Some(ProtoUser {
                id: ctx.user_id,
                username: ctx.username,
                is_owner: ctx.is_owner,
            }),
            workspace_id: Some(ctx.workspace_id),
            authenticated: true,
        },
        None => SessionResponse {
            user: None,
            workspace_id: None,
            authenticated: false,
        },
    };
    proto_or_json(wants_protobuf(&headers), &resp)
}

pub async fn list_workspaces(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
) -> Result<impl IntoResponse, AppError> {
    let mut items = state
        .store
        .users()
        .list_workspace_memberships(&ctx.user_id)
        .await?;
    if ctx.session_id.is_none() {
        items.retain(|workspace| workspace.id == ctx.workspace_id);
    }
    Ok(axum::Json(serde_json::json!({
        "current_workspace_id": ctx.workspace_id,
        "items": items
    })))
}

pub async fn change_password(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let req: ChangePasswordRequest = read_body_proto_or_json(&headers, body).await?;
    state
        .auth
        .change_password(&ctx.user_id, &req.current_password, &req.new_password)
        .await?;
    Ok(axum::Json(serde_json::json!({"ok": true})))
}

pub async fn list_tokens(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    require_token_admin(&ctx)?;
    let rows = state
        .store
        .users()
        .list_api_tokens(&ctx.workspace_id)
        .await?;
    let items = rows
        .into_iter()
        .map(
            |(id, name, scopes_json, expires_at, created_at)| ApiTokenInfo {
                id,
                name,
                scopes: serde_json::from_str(&scopes_json).unwrap_or_default(),
                token: None,
                expires_at,
                created_at,
            },
        )
        .collect();
    Ok(proto_or_json(
        wants_protobuf(&headers),
        &ApiTokenListResponse { items },
    ))
}

pub async fn create_token(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    require_token_admin(&ctx)?;
    let req: CreateApiTokenRequest = read_body_proto_or_json(&headers, body).await?;
    let raw = AuthService::mint_token("pnda_");
    let hash = AuthService::hash_token(&raw);
    let scopes = if req.scopes.is_empty() {
        vec![
            "memos:read".into(),
            "memos:write".into(),
            "notebooks:read".into(),
            "notebooks:write".into(),
            "resources:read".into(),
            "resources:write".into(),
        ]
    } else {
        req.scopes
    };
    let scopes_json = serde_json::to_string(&scopes).unwrap_or_else(|_| "[]".into());
    let id = state
        .store
        .users()
        .create_api_token(
            &ctx.workspace_id,
            &ctx.user_id,
            &req.name,
            &hash,
            &scopes_json,
            req.expires_at.as_deref(),
        )
        .await?;
    let info = ApiTokenInfo {
        id,
        name: req.name,
        scopes,
        token: Some(raw),
        expires_at: req.expires_at,
        created_at: store::now_rfc3339(),
    };
    let _ = PROTOCOL_VERSION;
    Ok(proto_or_json(wants_protobuf(&headers), &info))
}

pub async fn delete_token(
    State(state): State<AppState>,
    AuthUser(ctx): AuthUser,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<impl IntoResponse, AppError> {
    require_token_admin(&ctx)?;
    state
        .store
        .users()
        .delete_api_token(&ctx.workspace_id, &id)
        .await?;
    Ok(axum::Json(serde_json::json!({"ok": true})))
}

#[cfg(test)]
mod tests {
    use super::require_token_admin;
    use auth::AuthContext;

    fn context(is_owner: bool, session: bool) -> AuthContext {
        AuthContext {
            user_id: "user".into(),
            username: "user".into(),
            workspace_id: "workspace".into(),
            is_owner,
            device_id: None,
            scopes: None,
            session_id: session.then(|| "session".into()),
        }
    }

    #[test]
    fn token_management_requires_owner_session() {
        assert!(require_token_admin(&context(true, true)).is_ok());
        assert!(require_token_admin(&context(false, true)).is_err());
        assert!(require_token_admin(&context(true, false)).is_err());
    }
}
