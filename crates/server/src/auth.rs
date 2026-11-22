//! Login and role scoping. Passwords are argon2id hashes; sessions are short-lived HS256 JWTs.

use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::{request::Parts, StatusCode};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};

use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Role {
    Admin,
    Client,
}

#[derive(Debug, Clone)]
pub struct User {
    pub username: String,
    pub hash: String,
    pub role: Role,
    pub client_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    role: Role,
    client_id: Option<String>,
    exp: usize,
}

pub struct AuthState {
    users: Vec<User>,
    enc: EncodingKey,
    dec: DecodingKey,
}

pub fn hash_password(pw: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default().hash_password(pw.as_bytes(), &salt).expect("hashing").to_string()
}

pub fn random_secret() -> String {
    let mut b = [0u8; 48];
    OsRng.fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

impl AuthState {
    pub fn new(users: Vec<User>, secret: &str) -> Self {
        AuthState {
            users,
            enc: EncodingKey::from_secret(secret.as_bytes()),
            dec: DecodingKey::from_secret(secret.as_bytes()),
        }
    }

    pub fn login(&self, username: &str, password: &str) -> Option<(String, AuthUser)> {
        let user = self.users.iter().find(|u| u.username == username);
        // Verify against a dummy hash for unknown users so timing does not reveal valid names.
        let hash = user.map(|u| u.hash.clone()).unwrap_or_else(|| dummy_hash().to_string());
        let ok = PasswordHash::new(&hash)
            .ok()
            .map(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
            .unwrap_or(false);
        let user = user.filter(|_| ok)?;
        let exp = (chrono::Utc::now() + chrono::Duration::hours(12)).timestamp() as usize;
        let claims = Claims { sub: user.username.clone(), role: user.role, client_id: user.client_id.clone(), exp };
        let token = encode(&Header::default(), &claims, &self.enc).ok()?;
        Some((token, AuthUser { username: user.username.clone(), role: user.role, client_id: user.client_id.clone(), exp }))
    }

    fn verify(&self, token: &str) -> Option<AuthUser> {
        let data = decode::<Claims>(token, &self.dec, &Validation::default()).ok()?;
        let c = data.claims;
        Some(AuthUser { username: c.sub, role: c.role, client_id: c.client_id, exp: c.exp })
    }
}

fn dummy_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| hash_password("not-a-real-password"))
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthUser {
    pub username: String,
    pub role: Role,
    pub client_id: Option<String>,
    /// Token expiry (unix seconds). Long-lived sockets close when it passes.
    #[serde(skip)]
    pub exp: usize,
}

impl AuthUser {
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }
}

#[axum::async_trait]
impl FromRequestParts<Arc<AppState>> for AuthUser {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, StatusCode> {
        let from_header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::to_string);
        // The browser websocket API cannot set headers, so only /ws may pass ?token=.
        let from_query = if parts.uri.path() == "/ws" {
            parts.uri.query().and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("token=").map(str::to_string)))
        } else {
            None
        };
        let token = from_header.or(from_query).ok_or(StatusCode::UNAUTHORIZED)?;
        state.auth.verify(&token).ok_or(StatusCode::UNAUTHORIZED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth() -> AuthState {
        AuthState::new(
            vec![
                User { username: "admin".into(), hash: hash_password("pw1"), role: Role::Admin, client_id: None },
                User { username: "aarav".into(), hash: hash_password("pw2"), role: Role::Client, client_id: Some("c1".into()) },
            ],
            "test-secret",
        )
    }

    #[test]
    fn login_and_token_round_trip_keeps_scope() {
        let a = auth();
        let (token, user) = a.login("aarav", "pw2").expect("valid login");
        assert_eq!(user.client_id.as_deref(), Some("c1"));
        let back = a.verify(&token).expect("valid token");
        assert_eq!(back.role, Role::Client);
        assert_eq!(back.client_id.as_deref(), Some("c1"));
    }

    #[test]
    fn wrong_password_and_unknown_user_fail() {
        let a = auth();
        assert!(a.login("aarav", "nope").is_none());
        assert!(a.login("ghost", "pw1").is_none());
    }

    #[test]
    fn tampered_token_is_rejected() {
        let a = auth();
        let (token, _) = a.login("admin", "pw1").unwrap();
        assert!(a.verify(&format!("{token}x")).is_none());
        let other = AuthState::new(vec![], "different-secret");
        assert!(other.verify(&token).is_none());
    }
}
