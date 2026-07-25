//! Authentication: Argon2id passwords, Bearer sessions, API tokens.

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2, Params, Version,
};
use domain::{PandaError, PandaResult};
use rand::RngCore;
use store::{SessionRow, Store, UserRow};

#[derive(Clone)]
pub struct AuthService {
    store: Store,
    session_ttl_days: i64,
    argon2_memory_kib: u32,
    argon2_iterations: u32,
    argon2_parallelism: u32,
}

#[derive(Clone, Debug)]
pub struct AuthContext {
    pub user_id: String,
    pub username: String,
    pub workspace_id: String,
    pub is_owner: bool,
    pub device_id: Option<String>,
    pub scopes: Option<Vec<String>>,
    pub session_id: Option<String>,
}

impl AuthService {
    pub fn new(
        store: Store,
        session_ttl_days: i64,
        argon2_memory_kib: u32,
        argon2_iterations: u32,
        argon2_parallelism: u32,
    ) -> Self {
        Self {
            store,
            session_ttl_days,
            argon2_memory_kib,
            argon2_iterations,
            argon2_parallelism,
        }
    }

    fn argon2(&self) -> PandaResult<Argon2<'static>> {
        let params = Params::new(
            self.argon2_memory_kib,
            self.argon2_iterations,
            self.argon2_parallelism,
            None,
        )
        .map_err(|e| PandaError::internal(format!("argon2 params: {e}")))?;
        Ok(Argon2::new(
            argon2::Algorithm::Argon2id,
            Version::V0x13,
            params,
        ))
    }

    pub fn hash_password(&self, password: &str) -> PandaResult<String> {
        let salt = SaltString::generate(&mut rand::thread_rng());
        let hash = self
            .argon2()?
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| PandaError::internal(format!("hash: {e}")))?
            .to_string();
        Ok(hash)
    }

    pub fn verify_password(&self, password: &str, hash: &str) -> PandaResult<bool> {
        let parsed =
            PasswordHash::new(hash).map_err(|e| PandaError::internal(format!("bad hash: {e}")))?;
        Ok(self
            .argon2()?
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    }

    pub fn hash_token(token: &str) -> String {
        blake3::hash(token.as_bytes()).to_hex().to_string()
    }

    pub fn mint_token(prefix: &str) -> String {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        format!("{prefix}{}", hex::encode(bytes))
    }

    pub async fn ensure_bootstrap(&self, username: &str, password: &str) -> PandaResult<()> {
        if self.store.users().count_users().await? == 0 {
            let hash = self.hash_password(password)?;
            self.store.users().bootstrap_owner(username, &hash).await?;
            tracing::info!(username, "bootstrapped owner account");
        }
        Ok(())
    }

    pub async fn register(
        &self,
        username: &str,
        password: &str,
        device_id: Option<&str>,
    ) -> PandaResult<(UserRow, String, String, String)> {
        let username = username.trim();
        if username.is_empty() {
            return Err(PandaError::invalid("username required"));
        }
        if password.len() < 8 {
            return Err(PandaError::invalid("password too short"));
        }
        if self
            .store
            .users()
            .find_by_username(username)
            .await?
            .is_some()
        {
            return Err(PandaError::conflict("username already taken"));
        }
        let hash = self.hash_password(password)?;
        let (user_id, ws) = self
            .store
            .users()
            .create_personal_user(username, &hash)
            .await?;
        let mut user = self
            .store
            .users()
            .find_by_id(&user_id)
            .await?
            .ok_or_else(|| PandaError::internal("registered user missing"))?;
        user.is_owner = 1;
        let token = Self::mint_token("pnd_");
        let token_hash = Self::hash_token(&token);
        let expires = (chrono::Utc::now() + chrono::Duration::days(self.session_ttl_days))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        self.store
            .users()
            .create_session(&user.id, &ws, &token_hash, device_id, &expires)
            .await?;
        self.store.users().mark_login(&user.id).await?;
        Ok((user, token, ws, expires))
    }

    pub async fn login(
        &self,
        username: &str,
        password: &str,
        device_id: Option<&str>,
        workspace_id: Option<&str>,
    ) -> PandaResult<(UserRow, String, String, String)> {
        let mut user = self
            .store
            .users()
            .find_by_username(username)
            .await?
            .ok_or_else(|| PandaError::unauthenticated("invalid credentials"))?;
        if user.is_disabled != 0 {
            return Err(PandaError::unauthenticated("account disabled"));
        }
        if !self.verify_password(password, &user.password_hash)? {
            return Err(PandaError::unauthenticated("invalid credentials"));
        }
        let (ws, role) = self
            .store
            .users()
            .resolve_workspace(&user.id, workspace_id)
            .await?;
        user.is_owner = i64::from(role == "owner");
        let token = Self::mint_token("pnd_");
        let token_hash = Self::hash_token(&token);
        let expires = (chrono::Utc::now() + chrono::Duration::days(self.session_ttl_days))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        self.store
            .users()
            .create_session(&user.id, &ws, &token_hash, device_id, &expires)
            .await?;
        self.store.users().mark_login(&user.id).await?;
        Ok((user, token, ws, expires))
    }

    pub async fn authenticate_bearer(&self, token: &str) -> PandaResult<AuthContext> {
        if let Some(ctx) = self.try_session(token).await? {
            return Ok(ctx);
        }
        if let Some(ctx) = self.try_api_token(token).await? {
            return Ok(ctx);
        }
        Err(PandaError::unauthenticated("invalid token"))
    }

    async fn try_session(&self, token: &str) -> PandaResult<Option<AuthContext>> {
        let hash = Self::hash_token(token);
        let Some(s): Option<SessionRow> =
            self.store.users().find_session_by_token_hash(&hash).await?
        else {
            return Ok(None);
        };
        self.store.users().touch_session(&s.id).await.ok();
        Ok(Some(AuthContext {
            user_id: s.user_id,
            username: s.username,
            workspace_id: s.workspace_id,
            is_owner: s.is_owner != 0,
            device_id: s.device_id,
            scopes: None,
            session_id: Some(s.id),
        }))
    }

    async fn try_api_token(&self, token: &str) -> PandaResult<Option<AuthContext>> {
        if !token.starts_with("pnda_") && !token.starts_with("eev") {
            // still try hash lookup
        }
        let hash = Self::hash_token(token);
        let Some((ws, user_id, scopes_json, _id)) =
            self.store.users().find_api_token(&hash).await?
        else {
            return Ok(None);
        };
        let user = self
            .store
            .users()
            .find_by_id(&user_id)
            .await?
            .ok_or_else(|| PandaError::unauthenticated("token user missing"))?;
        let Some(role) = self.store.users().membership_role(&ws, &user_id).await? else {
            return Err(PandaError::unauthenticated(
                "token workspace membership no longer exists",
            ));
        };
        let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();
        Ok(Some(AuthContext {
            user_id,
            username: user.username,
            workspace_id: ws,
            is_owner: role == "owner",
            device_id: None,
            scopes: Some(scopes),
            session_id: None,
        }))
    }

    pub async fn logout(&self, token: &str) -> PandaResult<()> {
        let hash = Self::hash_token(token);
        self.store.users().revoke_session_by_token_hash(&hash).await
    }

    pub async fn change_password(
        &self,
        user_id: &str,
        current: &str,
        new_password: &str,
    ) -> PandaResult<()> {
        let user = self
            .store
            .users()
            .find_by_id(user_id)
            .await?
            .ok_or_else(|| PandaError::not_found("user"))?;
        if !self.verify_password(current, &user.password_hash)? {
            return Err(PandaError::unauthenticated("current password incorrect"));
        }
        if new_password.len() < 8 {
            return Err(PandaError::invalid("password too short"));
        }
        let hash = self.hash_password(new_password)?;
        self.store.users().update_password(user_id, &hash).await
    }

    pub fn require_scope(ctx: &AuthContext, scope: &str) -> PandaResult<()> {
        if ctx.scopes.is_none() {
            return Ok(()); // interactive session
        }
        let scopes = ctx.scopes.as_ref().unwrap();
        if scopes.iter().any(|s| s == scope || s == "admin") {
            Ok(())
        } else {
            Err(PandaError::new(
                domain::ErrorCode::PermissionDenied,
                format!("missing scope {scope}"),
            ))
        }
    }
}

// hex is used in mint_token
use hex;
