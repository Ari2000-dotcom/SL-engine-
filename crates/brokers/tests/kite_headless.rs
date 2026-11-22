//! The unattended Zerodha login, run against local stand-ins for kite.zerodha.com and api.kite.trade.

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use sha2::{Digest, Sha256};
use sl_brokers::kite_live::{KiteAuto, KiteBroker};
use sl_brokers::{BrokerAdapter, BrokerError};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

const SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
const API_SECRET: &str = "apisecret";

#[derive(Clone)]
struct Web {
    needs_authorize: bool,
    followed_foreign: Arc<AtomicBool>,
}

fn totp_ok(code: &str) -> bool {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    [now, now.saturating_sub(30), now + 30].iter().any(|t| sl_brokers::totp::code_at(SECRET, *t).as_deref() == Some(code))
}

fn json(body: serde_json::Value) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], body.to_string()).into_response()
}

async fn web_login_page() -> Response {
    ([(header::SET_COOKIE, "sess_id=seed123; Path=/")], "login page").into_response()
}

async fn web_login(Form(f): Form<HashMap<String, String>>) -> Response {
    if f.get("user_id").map(String::as_str) == Some("AB1234") && f.get("password").map(String::as_str) == Some("pw") {
        ([(header::SET_COOKIE, "kf_session=s1; Path=/")], json(serde_json::json!({"status":"success","data":{"request_id":"rid1","twofa_type":"totp"}}))).into_response()
    } else {
        (StatusCode::FORBIDDEN, json(serde_json::json!({"status":"error","message":"Invalid user_id or password"}))).into_response()
    }
}

async fn web_twofa(headers: HeaderMap, Form(f): Form<HashMap<String, String>>) -> Response {
    let cookie = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()).unwrap_or("");
    if !cookie.contains("kf_session=s1") || !cookie.contains("sess_id=seed123") {
        return (StatusCode::FORBIDDEN, json(serde_json::json!({"status":"error","message":"no session"}))).into_response();
    }
    if f.get("request_id").map(String::as_str) == Some("rid1") && totp_ok(f.get("twofa_value").map(String::as_str).unwrap_or("")) {
        ([(header::SET_COOKIE, "enctoken=zzz; Path=/")], json(serde_json::json!({"status":"success","data":{}}))).into_response()
    } else {
        (StatusCode::FORBIDDEN, json(serde_json::json!({"status":"error","message":"Invalid TOTP"}))).into_response()
    }
}

async fn web_connect(State(w): State<Web>, Query(q): Query<HashMap<String, String>>) -> Response {
    if q.get("skip_session").is_some() {
        if w.needs_authorize {
            return (StatusCode::FOUND, [(header::LOCATION, "/connect/authorize?x=1")]).into_response();
        }
        // The app's own redirect URL, on a host that nothing listens on. Following it would spend the token.
        return (StatusCode::FOUND, [(header::LOCATION, "http://127.0.0.1:9/cb?request_token=RT123&action=login")]).into_response();
    }
    web_login_page().await
}

async fn web_authorize() -> &'static str {
    "authorize page"
}

async fn web_foreign(State(w): State<Web>) -> &'static str {
    w.followed_foreign.store(true, Ordering::SeqCst);
    "spent"
}

#[derive(Clone)]
struct Api {
    key: &'static str,
    profile_calls: Arc<AtomicUsize>,
}

async fn api_session(Form(f): Form<HashMap<String, String>>) -> Response {
    let want = format!("{:x}", Sha256::digest(format!("keyabc{}{}", "RT123", API_SECRET).as_bytes()));
    if f.get("request_token").map(String::as_str) == Some("RT123") && f.get("checksum") == Some(&want) {
        json(serde_json::json!({"status":"success","data":{"access_token":"AT999","user_id":"AB1234"}}))
    } else {
        (StatusCode::FORBIDDEN, json(serde_json::json!({"status":"error","message":"bad checksum","error_type":"TokenException"}))).into_response()
    }
}

async fn api_profile(State(a): State<Api>, headers: HeaderMap) -> Response {
    a.profile_calls.fetch_add(1, Ordering::SeqCst);
    let auth = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).unwrap_or("");
    if auth == format!("token {}:AT999", a.key) {
        json(serde_json::json!({"status":"success","data":{"user_id":"AB1234"}}))
    } else {
        (StatusCode::FORBIDDEN, json(serde_json::json!({"status":"error","message":"Incorrect api_key or access_token.","error_type":"TokenException"}))).into_response()
    }
}

async fn serve(app: Router) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    format!("http://{addr}")
}

async fn stand_ins(needs_authorize: bool) -> (String, String, Web, Api) {
    let web = Web { needs_authorize, followed_foreign: Arc::new(AtomicBool::new(false)) };
    let api = Api { key: "keyabc", profile_calls: Arc::new(AtomicUsize::new(0)) };
    let web_url = serve(
        Router::new()
            .route("/connect/login", get(web_connect))
            .route("/connect/authorize", get(web_authorize))
            .route("/cb", get(web_foreign))
            .route("/api/login", post(web_login))
            .route("/api/twofa", post(web_twofa))
            .with_state(web.clone()),
    )
    .await;
    let api_url = serve(Router::new().route("/session/token", post(api_session)).route("/user/profile", get(api_profile)).with_state(api.clone())).await;
    (web_url, api_url, web, api)
}

fn auto(totp: &str, password: &str) -> KiteAuto {
    KiteAuto { api_secret: API_SECRET.into(), user_id: "AB1234".into(), password: password.into(), totp_secret: totp.into() }
}

#[tokio::test]
async fn logs_in_by_itself_without_spending_the_request_token() {
    let (web_url, api_url, web, _api) = stand_ins(false).await;
    let k = KiteBroker::new("keyabc", "").with_auto(auto(SECRET, "pw")).with_hosts(&api_url, &web_url);
    k.login().await.expect("unattended login");
    assert!(k.has_token());
    assert!(!web.followed_foreign.load(Ordering::SeqCst), "the redirect to the app URL must not be followed");
}

#[tokio::test]
async fn an_expired_stored_token_is_replaced_by_a_fresh_login() {
    let (web_url, api_url, _web, api) = stand_ins(false).await;
    let k = KiteBroker::new("keyabc", "OLD-EXPIRED").with_auto(auto(SECRET, "pw")).with_hosts(&api_url, &web_url);
    k.login().await.expect("re-login");
    assert!(api.profile_calls.load(Ordering::SeqCst) >= 2, "old token tried, then the new one checked");
}

#[tokio::test]
async fn a_good_stored_token_skips_the_web_login() {
    let (web_url, api_url, web, _api) = stand_ins(false).await;
    // AT999 is what the stand-in accepts, and the password here is wrong: it must never be used.
    let k = KiteBroker::new("keyabc", "AT999").with_auto(auto(SECRET, "wrong")).with_hosts(&api_url, &web_url);
    k.login().await.expect("stored token still valid");
    assert!(!web.followed_foreign.load(Ordering::SeqCst));
}

#[tokio::test]
async fn a_wrong_password_says_so() {
    let (web_url, api_url, _w, _a) = stand_ins(false).await;
    let k = KiteBroker::new("keyabc", "").with_auto(auto(SECRET, "nope")).with_hosts(&api_url, &web_url);
    match k.login().await {
        Err(BrokerError::Auth(m)) => assert!(m.contains("Invalid user_id or password"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_wrong_totp_secret_gets_the_hint() {
    let (web_url, api_url, _w, _a) = stand_ins(false).await;
    let k = KiteBroker::new("keyabc", "").with_auto(auto("MFRGGZDFMZTWQ2LKNNWG23TPOBYXE43U", "pw")).with_hosts(&api_url, &web_url);
    match k.login().await {
        Err(BrokerError::Auth(m)) => assert!(m.contains("TOTP"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn the_first_connection_asks_for_the_one_time_authorize_click() {
    let (web_url, api_url, _w, _a) = stand_ins(true).await;
    let k = KiteBroker::new("keyabc", "").with_auto(auto(SECRET, "pw")).with_hosts(&api_url, &web_url);
    match k.login().await {
        Err(BrokerError::Auth(m)) => assert!(m.contains("one-time") && m.contains("/connect/login"), "{m}"),
        other => panic!("{other:?}"),
    }
}
