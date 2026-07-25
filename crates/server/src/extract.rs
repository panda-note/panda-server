use crate::error::AppError;
use crate::state::AppState;
use auth::AuthContext;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use domain::PandaError;

pub struct AuthUser(pub AuthContext);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| PandaError::unauthenticated("missing Authorization"))?;
        let token = header
            .strip_prefix("Bearer ")
            .or_else(|| header.strip_prefix("bearer "))
            .ok_or_else(|| PandaError::unauthenticated("expected Bearer token"))?;
        let ctx = state.auth.authenticate_bearer(token).await?;
        Ok(AuthUser(ctx))
    }
}

pub struct OptionalAuth(pub Option<AuthContext>);

impl FromRequestParts<AppState> for OptionalAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let Some(header) = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
        else {
            return Ok(OptionalAuth(None));
        };
        let Some(token) = header
            .strip_prefix("Bearer ")
            .or_else(|| header.strip_prefix("bearer "))
        else {
            return Ok(OptionalAuth(None));
        };
        match state.auth.authenticate_bearer(token).await {
            Ok(ctx) => Ok(OptionalAuth(Some(ctx))),
            Err(_) => Ok(OptionalAuth(None)),
        }
    }
}
