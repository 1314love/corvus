//! Local authentication adapter: Argon2id passwords, HS256 access tokens and
//! opaque rotating refresh tokens. Auth is off by default on loopback binds.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use argon2::password_hash::rand_core::OsRng as SaltRng;
use argon2::password_hash::{PasswordHash, SaltString};
use argon2::{Argon2, PasswordHasher as _, PasswordVerifier};
use async_trait::async_trait;
use axum::http::{header, HeaderMap, StatusCode};
use chrono::Utc;
use corvus_application::dto::{LoginRequest, RefreshRequest, TokenPair, UserAccount, UserView};
use corvus_application::ports::{
    AccessClaims, AuthRepository, PasswordHasher, RefreshTokenRecord, TokenService,
};
pub use corvus_domain::AuthMode;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::rngs::OsRng;
use rand::RngCore;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

/// Authentication configuration chosen by the daemon.
#[derive(Clone, Debug)]
pub struct AuthConfig {
    /// Off on loopback, Local for authenticated deployments.
    pub mode: AuthMode,
    /// JWT issuer claim.
    pub issuer: String,
    /// JWT audience claim.
    pub audience: String,
    /// Access-token lifetime in seconds.
    pub access_ttl_secs: u64,
    /// Refresh-token lifetime in seconds.
    pub refresh_ttl_secs: i64,
    /// HS256 signing secret; never logged.
    pub jwt_secret: SecretString,
}

impl AuthConfig {
    /// Auth disabled. Only valid on loopback binds.
    pub fn disabled() -> Self {
        Self {
            mode: AuthMode::Off,
            issuer: "corvus".to_string(),
            audience: "corvus-api".to_string(),
            access_ttl_secs: 900,
            refresh_ttl_secs: 30 * 24 * 60 * 60,
            jwt_secret: SecretString::new(String::new().into_boxed_str()),
        }
    }

    /// Auth enabled with a caller-supplied secret.
    pub fn local(secret: impl Into<String>) -> Self {
        let secret: String = secret.into();
        Self {
            mode: AuthMode::Local,
            jwt_secret: SecretString::new(secret.into_boxed_str()),
            ..Self::disabled()
        }
    }

    /// Whether requests must carry a bearer token.
    pub fn enabled(&self) -> bool {
        self.mode == AuthMode::Local
    }

    /// Whether the signing secret is long enough for HS256.
    pub fn secret_is_strong(&self) -> bool {
        self.jwt_secret.expose_secret().len() >= 32
    }
}

/// Typed authentication failure.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// Auth is disabled but an endpoint needs it.
    #[error("authentication is disabled")]
    Disabled,
    /// Username or password did not match.
    #[error("invalid username or password")]
    InvalidCredentials,
    /// The account exists but is disabled.
    #[error("account is disabled")]
    AccountDisabled,
    /// No bearer header was supplied.
    #[error("missing bearer token")]
    MissingToken,
    /// The token failed signature, audience or format validation.
    #[error("invalid bearer token")]
    InvalidToken,
    /// The token is well formed but expired.
    #[error("authentication token is expired")]
    Expired,
    /// The auth store failed.
    #[error("authentication storage failed: {0}")]
    Storage(String),
    /// An internal hashing or signing failure.
    #[error("internal authentication failure")]
    Internal,
}

impl AuthError {
    /// HTTP status used by the REST adapter.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Disabled => StatusCode::SERVICE_UNAVAILABLE,
            Self::InvalidCredentials | Self::MissingToken | Self::InvalidToken | Self::Expired => {
                StatusCode::UNAUTHORIZED
            }
            Self::AccountDisabled => StatusCode::FORBIDDEN,
            Self::Storage(_) | Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Disabled => "AUTH_DISABLED",
            Self::InvalidCredentials => "INVALID_CREDENTIALS",
            Self::AccountDisabled => "ACCOUNT_DISABLED",
            Self::MissingToken => "MISSING_TOKEN",
            Self::InvalidToken => "INVALID_TOKEN",
            Self::Expired => "TOKEN_EXPIRED",
            Self::Storage(_) => "AUTH_STORAGE",
            Self::Internal => "AUTH_INTERNAL",
        }
    }
}

/// Argon2id password hasher.
#[derive(Debug, Default, Clone, Copy)]
pub struct Argon2PasswordHasher;

impl PasswordHasher for Argon2PasswordHasher {
    fn hash(&self, password: &str) -> Result<String, String> {
        let salt = SaltString::generate(&mut SaltRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|error| error.to_string())
    }

    fn verify(&self, password: &str, hash: &str) -> Result<bool, String> {
        let parsed = PasswordHash::new(hash).map_err(|error| error.to_string())?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct JwtClaims {
    sub: String,
    username: String,
    role: String,
    iss: String,
    aud: String,
    exp: usize,
    iat: usize,
    jti: String,
}

/// HS256 access-token implementation.
pub struct JwtTokenService {
    encoding: EncodingKey,
    decoding: DecodingKey,
    issuer: String,
    audience: String,
    ttl_secs: u64,
}

impl JwtTokenService {
    /// Build a token service from raw secret bytes.
    pub fn new(
        secret: &[u8],
        issuer: impl Into<String>,
        audience: impl Into<String>,
        ttl_secs: u64,
    ) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
            issuer: issuer.into(),
            audience: audience.into(),
            ttl_secs,
        }
    }
}

impl TokenService for JwtTokenService {
    fn issue_access(&self, user: &UserView) -> Result<String, String> {
        let now = Utc::now().timestamp() as usize;
        let claims = JwtClaims {
            sub: user.id.clone(),
            username: user.username.clone(),
            role: user.role.clone(),
            iss: self.issuer.clone(),
            aud: self.audience.clone(),
            exp: now.saturating_add(self.ttl_secs as usize),
            iat: now,
            jti: random_hex(16),
        };
        encode(&Header::new(Algorithm::HS256), &claims, &self.encoding)
            .map_err(|error| error.to_string())
    }

    fn verify_access(&self, token: &str) -> Result<AccessClaims, String> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.set_audience(&[self.audience.as_str()]);
        let data = decode::<JwtClaims>(token, &self.decoding, &validation)
            .map_err(|error| error.to_string())?;
        Ok(AccessClaims {
            subject: data.claims.sub,
            username: data.claims.username,
            role: data.claims.role,
            expires_at: data.claims.exp,
        })
    }
}

/// Authentication use cases shared by REST middleware and handlers.
pub struct AuthService {
    config: AuthConfig,
    repository: Arc<dyn AuthRepository>,
    hasher: Arc<dyn PasswordHasher>,
    tokens: Arc<dyn TokenService>,
}

impl AuthService {
    /// Build the default service with Argon2id and HS256.
    pub fn new(config: AuthConfig, repository: Arc<dyn AuthRepository>) -> Self {
        let tokens = JwtTokenService::new(
            config.jwt_secret.expose_secret().as_bytes(),
            config.issuer.clone(),
            config.audience.clone(),
            config.access_ttl_secs,
        );
        Self {
            config,
            repository,
            hasher: Arc::new(Argon2PasswordHasher),
            tokens: Arc::new(tokens),
        }
    }

    /// Build with alternate hashing and token adapters, for tests.
    pub fn with_adapters(
        config: AuthConfig,
        repository: Arc<dyn AuthRepository>,
        hasher: Arc<dyn PasswordHasher>,
        tokens: Arc<dyn TokenService>,
    ) -> Self {
        Self {
            config,
            repository,
            hasher,
            tokens,
        }
    }

    /// Current auth mode.
    pub fn mode(&self) -> AuthMode {
        self.config.mode
    }

    /// Validate a request header map.
    pub fn authorize(&self, headers: &HeaderMap) -> Result<AccessClaims, AuthError> {
        if !self.config.enabled() {
            return Err(AuthError::Disabled);
        }
        let token = bearer_token(headers).ok_or(AuthError::MissingToken)?;
        self.tokens
            .verify_access(token)
            .map_err(|_| AuthError::InvalidToken)
    }

    /// Exchange username and password for an access and refresh token pair.
    pub async fn login(&self, request: &LoginRequest) -> Result<TokenPair, AuthError> {
        if !self.config.enabled() {
            return Err(AuthError::Disabled);
        }
        let account = self
            .repository
            .find_user(&request.username)
            .await
            .map_err(AuthError::Storage)?
            .ok_or(AuthError::InvalidCredentials)?;
        if account.disabled {
            return Err(AuthError::AccountDisabled);
        }
        let valid = self
            .hasher
            .verify(&request.password, &account.password_hash)
            .map_err(|_| AuthError::Internal)?;
        if !valid {
            return Err(AuthError::InvalidCredentials);
        }
        let view = UserView {
            id: account.id.clone(),
            username: account.username.clone(),
            role: account.role.clone(),
        };
        self.issue_pair(&view).await
    }

    /// Rotate a refresh token and issue a fresh pair.
    pub async fn refresh(&self, request: &RefreshRequest) -> Result<TokenPair, AuthError> {
        if !self.config.enabled() {
            return Err(AuthError::Disabled);
        }
        let (id, secret) = split_refresh(&request.refresh_token)?;
        let record = self
            .repository
            .find_refresh(id)
            .await
            .map_err(AuthError::Storage)?
            .ok_or(AuthError::InvalidToken)?;
        if record.revoked_at.is_some() {
            return Err(AuthError::InvalidToken);
        }
        let now = now_ms();
        if record.expires_at <= now {
            return Err(AuthError::Expired);
        }
        let valid = self
            .hasher
            .verify(secret, &record.token_hash)
            .map_err(|_| AuthError::Internal)?;
        if !valid {
            return Err(AuthError::InvalidToken);
        }
        let view = UserView {
            id: record.user_id.clone(),
            username: record.username.clone(),
            role: record.role.clone(),
        };
        let pair = self.issue_pair(&view).await?;
        let replacement = pair
            .refresh_token
            .split_once('.')
            .map(|(id, _)| id.to_string())
            .unwrap_or_default();
        self.repository
            .revoke_refresh(&record.id, now, Some(&replacement))
            .await
            .map_err(AuthError::Storage)?;
        Ok(pair)
    }

    /// Revoke a refresh token.
    pub async fn logout(&self, request: &RefreshRequest) -> Result<(), AuthError> {
        if !self.config.enabled() {
            return Err(AuthError::Disabled);
        }
        let (id, _) = split_refresh(&request.refresh_token)?;
        if let Some(record) = self
            .repository
            .find_refresh(id)
            .await
            .map_err(AuthError::Storage)?
        {
            if record.revoked_at.is_none() {
                self.repository
                    .revoke_refresh(&record.id, now_ms(), None)
                    .await
                    .map_err(AuthError::Storage)?;
            }
        }
        Ok(())
    }

    async fn issue_pair(&self, user: &UserView) -> Result<TokenPair, AuthError> {
        let access_token = self
            .tokens
            .issue_access(user)
            .map_err(|_| AuthError::Internal)?;
        let id = random_hex(16);
        let secret = random_hex(32);
        let token_hash = self.hasher.hash(&secret).map_err(|_| AuthError::Internal)?;
        let now = now_ms();
        let expires_at = now.saturating_add(self.config.refresh_ttl_secs.saturating_mul(1000));
        self.repository
            .store_refresh(&id, &user.id, &token_hash, expires_at, now)
            .await
            .map_err(AuthError::Storage)?;
        Ok(TokenPair {
            access_token,
            refresh_token: format!("{id}.{secret}"),
            expires_in: self.config.access_ttl_secs,
            token_type: "Bearer".to_string(),
        })
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    (!token.is_empty()).then_some(token)
}

fn split_refresh(token: &str) -> Result<(&str, &str), AuthError> {
    let (id, secret) = token.split_once('.').ok_or(AuthError::InvalidToken)?;
    if id.is_empty() || secret.is_empty() {
        return Err(AuthError::InvalidToken);
    }
    Ok((id, secret))
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buffer);
    hex::encode(buffer)
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

/// In-memory auth repository used by tests and by auth-enabled smoke checks.
#[derive(Default)]
pub struct MemoryAuthRepository {
    users: Mutex<Vec<UserAccount>>,
    refresh: Mutex<HashMap<String, RefreshTokenRecord>>,
    next_user: AtomicU64,
}

impl MemoryAuthRepository {
    /// Empty repository.
    pub fn new() -> Self {
        Self::default()
    }

    /// Convenience helper that creates a user with an Argon2id hash.
    pub async fn add_user(
        &self,
        username: &str,
        password: &str,
        role: &str,
    ) -> Result<String, String> {
        let hash = Argon2PasswordHasher.hash(password)?;
        self.create_user(username, &hash, role).await
    }
}

#[async_trait]
impl AuthRepository for MemoryAuthRepository {
    async fn create_user(
        &self,
        username: &str,
        password_hash: &str,
        role: &str,
    ) -> Result<String, String> {
        if username.trim().is_empty() {
            return Err("username must not be empty".to_string());
        }
        let mut users = self
            .users
            .lock()
            .map_err(|_| "auth store poisoned".to_string())?;
        if users.iter().any(|user| user.username == username) {
            return Err("username already exists".to_string());
        }
        let id = format!(
            "user-{}",
            self.next_user.fetch_add(1, Ordering::Relaxed) + 1
        );
        users.push(UserAccount {
            id: id.clone(),
            username: username.to_string(),
            password_hash: password_hash.to_string(),
            role: role.to_string(),
            disabled: false,
        });
        Ok(id)
    }

    async fn find_user(&self, username: &str) -> Result<Option<UserAccount>, String> {
        let users = self
            .users
            .lock()
            .map_err(|_| "auth store poisoned".to_string())?;
        Ok(users.iter().find(|user| user.username == username).cloned())
    }

    async fn user_view(&self, id: &str) -> Result<Option<UserView>, String> {
        let users = self
            .users
            .lock()
            .map_err(|_| "auth store poisoned".to_string())?;
        Ok(users
            .iter()
            .find(|user| user.id == id)
            .map(|user| UserView {
                id: user.id.clone(),
                username: user.username.clone(),
                role: user.role.clone(),
            }))
    }

    async fn store_refresh(
        &self,
        id: &str,
        user_id: &str,
        token_hash: &str,
        expires_at: i64,
        _created_at: i64,
    ) -> Result<(), String> {
        let user = {
            let users = self
                .users
                .lock()
                .map_err(|_| "auth store poisoned".to_string())?;
            users.iter().find(|user| user.id == user_id).cloned()
        };
        let Some(user) = user else {
            return Err("user does not exist".to_string());
        };
        let mut refresh = self
            .refresh
            .lock()
            .map_err(|_| "auth store poisoned".to_string())?;
        refresh.insert(
            id.to_string(),
            RefreshTokenRecord {
                id: id.to_string(),
                user_id: user.id.clone(),
                token_hash: token_hash.to_string(),
                expires_at,
                revoked_at: None,
                replaced_by: None,
                username: user.username,
                role: user.role,
            },
        );
        Ok(())
    }

    async fn find_refresh(&self, id: &str) -> Result<Option<RefreshTokenRecord>, String> {
        let refresh = self
            .refresh
            .lock()
            .map_err(|_| "auth store poisoned".to_string())?;
        Ok(refresh.get(id).cloned())
    }

    async fn revoke_refresh(
        &self,
        id: &str,
        revoked_at: i64,
        replaced_by: Option<&str>,
    ) -> Result<(), String> {
        let mut refresh = self
            .refresh
            .lock()
            .map_err(|_| "auth store poisoned".to_string())?;
        if let Some(record) = refresh.get_mut(id) {
            if record.revoked_at.is_none() {
                record.revoked_at = Some(revoked_at);
                record.replaced_by = replaced_by.map(str::to_string);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AuthConfig {
        AuthConfig::local("0123456789abcdef0123456789abcdef")
    }

    #[test]
    fn short_secrets_are_not_accepted() {
        assert!(!AuthConfig::local("short").secret_is_strong());
        assert!(config().secret_is_strong());
        assert!(!AuthConfig::disabled().enabled());
    }

    #[tokio::test]
    async fn login_issues_bearer_tokens_and_rotation_invalidates_the_old_refresh() {
        let repository = Arc::new(MemoryAuthRepository::new());
        repository
            .add_user("admin", "correct horse battery staple", "admin")
            .await
            .expect("the bootstrap user is created");
        let service = AuthService::new(config(), repository);

        let pair = service
            .login(&LoginRequest {
                username: "admin".to_string(),
                password: "correct horse battery staple".to_string(),
            })
            .await
            .expect("valid credentials log in");
        assert_eq!(pair.token_type, "Bearer");
        assert_eq!(pair.expires_in, 900);
        assert!(pair.refresh_token.contains('.'));

        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", pair.access_token)
                .parse()
                .expect("a bearer header"),
        );
        let claims = service
            .authorize(&headers)
            .expect("the access token verifies");
        assert_eq!(claims.username, "admin");

        let rotated = service
            .refresh(&RefreshRequest {
                refresh_token: pair.refresh_token.clone(),
            })
            .await
            .expect("the refresh token rotates");
        assert_ne!(rotated.refresh_token, pair.refresh_token);
        assert!(matches!(
            service
                .refresh(&RefreshRequest {
                    refresh_token: pair.refresh_token,
                })
                .await,
            Err(AuthError::InvalidToken)
        ));

        service
            .logout(&RefreshRequest {
                refresh_token: rotated.refresh_token.clone(),
            })
            .await
            .expect("logout revokes the refresh token");
        assert!(matches!(
            service
                .refresh(&RefreshRequest {
                    refresh_token: rotated.refresh_token,
                })
                .await,
            Err(AuthError::InvalidToken)
        ));
    }

    #[tokio::test]
    async fn wrong_passwords_and_missing_headers_are_typed() {
        let repository = Arc::new(MemoryAuthRepository::new());
        repository
            .add_user("admin", "correct horse battery staple", "admin")
            .await
            .expect("the bootstrap user is created");
        let service = AuthService::new(config(), repository);
        assert!(matches!(
            service
                .login(&LoginRequest {
                    username: "admin".to_string(),
                    password: "wrong".to_string(),
                })
                .await,
            Err(AuthError::InvalidCredentials)
        ));
        assert!(matches!(
            service.authorize(&HeaderMap::new()),
            Err(AuthError::MissingToken)
        ));
    }
}
