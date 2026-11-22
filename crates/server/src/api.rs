//! REST and websocket surface. Every response is scoped to the caller: a client user only ever
//! receives rows for their own client id.

use crate::auth::AuthUser;
use crate::state::{AppState, LegQuery};
use axum::extract::rejection::JsonRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use sl_core::{RiskBook, RiskMode};
use std::collections::HashSet;
use std::sync::Arc;

type Shared = Arc<AppState>;

const MAX_LOGIN_FAILS: u32 = 5;
const LOGIN_WINDOW_SECS: i64 = 60;

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/api/login", post(login))
        .route("/api/state", get(get_state))
        .route("/api/legs", get(get_legs))
        .route("/api/accounts/:id/book", get(get_book))
        .route("/api/incidents/:id/ack", post(ack))
        .route("/api/risk", get(get_risk).put(put_risk))
        .route("/api/admin/accounts", get(crate::accounts::list).post(crate::accounts::add))
        .route("/api/admin/accounts/:id", axum::routing::delete(crate::accounts::remove))
        .route("/api/admin/accounts/:id/session", put(crate::accounts::set_session))
        .route("/ws", get(ws))
        .with_state(state)
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "error": msg }))).into_response()
}

#[derive(Deserialize)]
struct LoginReq {
    username: String,
    password: String,
}

/// True when this username has used up its failed attempts for the current window.
fn throttled(s: &AppState, user: &str) -> bool {
    let now = Utc::now();
    let mut map = s.login_fails.lock().expect("login_fails lock");
    match map.get(user) {
        Some((n, since)) if now - *since < Duration::seconds(LOGIN_WINDOW_SECS) => *n >= MAX_LOGIN_FAILS,
        Some(_) => {
            map.remove(user);
            false
        }
        None => false,
    }
}

fn record_failure(s: &AppState, user: &str) {
    let now = Utc::now();
    let mut map = s.login_fails.lock().expect("login_fails lock");
    // Bound the table so random usernames cannot grow it without limit.
    if map.len() > 10_000 {
        map.retain(|_, (_, since)| now - *since < Duration::seconds(LOGIN_WINDOW_SECS));
    }
    let e = map.entry(user.to_string()).or_insert((0, now));
    if now - e.1 >= Duration::seconds(LOGIN_WINDOW_SECS) {
        *e = (0, now);
    }
    e.0 += 1;
}

async fn login(State(s): State<Shared>, body: Result<Json<LoginReq>, JsonRejection>) -> Response {
    let Ok(Json(req)) = body else { return err(StatusCode::BAD_REQUEST, "Send a username and password") };
    let key = req.username.trim().to_lowercase();
    if throttled(&s, &key) {
        return err(StatusCode::TOO_MANY_REQUESTS, "Too many attempts. Wait a minute and try again.");
    }
    // Hashing is slow on purpose. Run it off the async workers, a few at a time, so a flood of
    // logins cannot stall the broker polling tasks.
    let Ok(_permit) = s.login_gate.acquire().await else { return err(StatusCode::SERVICE_UNAVAILABLE, "Try again") };
    let st = s.clone();
    let (u, p) = (req.username.clone(), req.password.clone());
    let result = tokio::task::spawn_blocking(move || st.auth.login(&u, &p)).await.ok().flatten();
    match result {
        Some((token, user)) => {
            s.login_fails.lock().expect("login_fails lock").remove(&key);
            (StatusCode::OK, Json(json!({ "token": token, "user": user }))).into_response()
        }
        None => {
            record_failure(&s, &key);
            err(StatusCode::UNAUTHORIZED, "Wrong username or password")
        }
    }
}

async fn get_legs(State(s): State<Shared>, user: AuthUser, Query(q): Query<LegQuery>) -> impl IntoResponse {
    let inner = s.inner.read().await;
    Json(inner.legs_page(user.role, user.client_id.as_deref(), &q))
}

async fn get_book(State(s): State<Shared>, user: AuthUser, Path(id): Path<String>) -> Response {
    let inner = s.inner.read().await;
    match inner.account_book(user.role, user.client_id.as_deref(), &id) {
        Some(b) => Json(b).into_response(),
        // Same answer for "missing" and "not yours", so ids cannot be probed.
        None => err(StatusCode::NOT_FOUND, "Account not found"),
    }
}

async fn get_state(State(s): State<Shared>, user: AuthUser) -> impl IntoResponse {
    let inner = s.inner.read().await;
    Json(inner.view(user.role, user.client_id.as_deref(), Utc::now()))
}

async fn ack(State(s): State<Shared>, user: AuthUser, Path(id): Path<u64>) -> StatusCode {
    let mut inner = s.inner.write().await;
    // A client may only acknowledge incidents on their own legs.
    let allowed = inner
        .incidents
        .incidents
        .iter()
        .find(|i| i.id == id)
        .map(|i| user.is_admin() || user.client_id.as_deref() == Some(i.client_id.as_str()))
        .unwrap_or(false);
    if !allowed {
        return StatusCode::NOT_FOUND;
    }
    let ok = inner.incidents.acknowledge(id, &user.username, Utc::now());
    drop(inner);
    let _ = s.notify.send(());
    if ok { StatusCode::NO_CONTENT } else { StatusCode::CONFLICT }
}

async fn get_risk(State(s): State<Shared>, user: AuthUser) -> Response {
    if !user.is_admin() {
        return StatusCode::FORBIDDEN.into_response();
    }
    Json(s.inner.read().await.risk.clone()).into_response()
}

pub fn validate(book: &RiskBook) -> Result<(), String> {
    if !book.rules.iter().any(|r| r.client_id.is_none() && r.segment.is_none()) {
        return Err("Keep one default rule for all clients and all segments".into());
    }
    let mut seen = HashSet::new();
    for r in &book.rules {
        if !seen.insert((r.client_id.clone(), r.segment)) {
            return Err("Two rules have the same client and segment".into());
        }
        let s = &r.setting;
        match s.mode {
            RiskMode::Pct if !(s.max_pct.is_finite() && s.max_pct > 0.0 && s.max_pct <= 1000.0) => {
                return Err("Maximum loss % must be above 0 and at most 1000".into());
            }
            RiskMode::Rupee if !(s.max_rupee.is_finite() && s.max_rupee >= 0.0) => {
                return Err("Maximum loss in rupees must be zero or more".into());
            }
            _ => {}
        }
        if !(0..=3600).contains(&s.grace_sec) {
            return Err("Grace period must be between 0 and 3600 seconds".into());
        }
    }
    Ok(())
}

async fn put_risk(State(s): State<Shared>, user: AuthUser, body: Result<Json<RiskBook>, JsonRejection>) -> Response {
    if !user.is_admin() {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(Json(book)) = body else { return err(StatusCode::BAD_REQUEST, "Some risk values are missing or not numbers") };
    if let Err(msg) = validate(&book) {
        return err(StatusCode::BAD_REQUEST, &msg);
    }
    let mut inner = s.inner.write().await;
    inner.risk = book;
    inner.reevaluate_all(Utc::now());
    drop(inner);
    tracing::info!(by = %user.username, "risk settings updated");
    let _ = s.notify.send(());
    StatusCode::NO_CONTENT.into_response()
}

async fn ws(State(s): State<Shared>, user: AuthUser, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| push_loop(socket, s, user))
}

async fn push_loop(mut socket: WebSocket, s: Shared, user: AuthUser) {
    let mut rx = s.notify.subscribe();
    loop {
        // The token was only checked at connect time, so end the socket when it expires.
        let remaining = user.exp as i64 - Utc::now().timestamp();
        if remaining <= 0 {
            let _ = socket.close().await;
            break;
        }
        let payload = {
            let inner = s.inner.read().await;
            serde_json::to_string(&inner.view(user.role, user.client_id.as_deref(), Utc::now()))
        };
        let Ok(text) = payload else { break };
        if socket.send(Message::Text(text)).await.is_err() {
            break;
        }
        tokio::select! {
            r = rx.recv() => {
                // Lagging is fine: the next send carries the full current state anyway.
                if let Err(tokio::sync::broadcast::error::RecvError::Closed) = r { break }
                // Coalesce bursts into one push.
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                while rx.try_recv().is_ok() {}
            }
            m = socket.recv() => match m {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
            // Heartbeat so time-driven fields (unprotected seconds) keep moving, and so the
            // expiry check above runs even when nothing changes.
            _ = tokio::time::sleep(std::time::Duration::from_secs(5.min(remaining as u64))) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sl_core::{RiskRule, RiskSetting, Segment};

    fn rule(client: Option<&str>, seg: Option<Segment>, setting: RiskSetting) -> RiskRule {
        RiskRule { client_id: client.map(str::to_string), segment: seg, setting }
    }

    #[test]
    fn requires_a_default_rule() {
        let b = RiskBook { rules: vec![rule(Some("c1"), None, RiskSetting::default())] };
        assert!(validate(&b).is_err());
        let ok = RiskBook { rules: vec![rule(None, None, RiskSetting::default())] };
        assert!(validate(&ok).is_ok());
    }

    #[test]
    fn rejects_duplicates() {
        let b = RiskBook {
            rules: vec![
                rule(None, None, RiskSetting::default()),
                rule(Some("c1"), Some(Segment::Opt), RiskSetting::default()),
                rule(Some("c1"), Some(Segment::Opt), RiskSetting::default()),
            ],
        };
        assert!(validate(&b).is_err());
    }

    #[test]
    fn only_the_active_mode_field_is_checked() {
        // Rupee mode may carry a junk percent value, and the other way round.
        let rupee = RiskSetting { mode: RiskMode::Rupee, max_pct: f64::NAN, max_rupee: 5000.0, grace_sec: 15 };
        assert!(validate(&RiskBook { rules: vec![rule(None, None, rupee)] }).is_ok());
        let pct = RiskSetting { mode: RiskMode::Pct, max_pct: 40.0, max_rupee: f64::NAN, grace_sec: 15 };
        assert!(validate(&RiskBook { rules: vec![rule(None, None, pct)] }).is_ok());
        let bad = RiskSetting { mode: RiskMode::Pct, max_pct: 0.0, max_rupee: 1.0, grace_sec: 15 };
        assert!(validate(&RiskBook { rules: vec![rule(None, None, bad)] }).is_err());
    }

    #[test]
    fn grace_bounds() {
        let bad = RiskSetting { grace_sec: 4000, ..RiskSetting::default() };
        assert!(validate(&RiskBook { rules: vec![rule(None, None, bad)] }).is_err());
    }
}
