//! Admin-only API for adding broker accounts at runtime. Keys and tokens go in once, are stored
//! encrypted, and are never sent back to the browser. Zerodha Kite, Angel One and Kotak Neo are connected.

use crate::auth::AuthUser;
use crate::monitor::{self, Control};
use crate::state::{AccountHealth, AccountInfo, AppState, ClientInfo, HealthStatus};
use crate::vault::{Persisted, StoredAccount, Vault};
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use sl_brokers::angel_live::AngelBroker;
use sl_brokers::kite_live::{exchange_request_token, KiteAuto, KiteBroker};
use sl_brokers::kotak_live::KotakBroker;
use sl_brokers::{totp, BrokerAdapter};
use sl_core::Broker;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

type Shared = Arc<AppState>;

pub struct Live {
    pub adapter: Arc<dyn BrokerAdapter>,
    /// Only Zerodha has a token the admin can replace by hand.
    pub kite: Option<Arc<KiteBroker>>,
    pub ctl: Arc<Control>,
}

fn extra<'a>(a: &'a StoredAccount, key: &str) -> &'a str {
    a.extra.get(key).map(String::as_str).unwrap_or("")
}

/// Whether this stored account can sign in without a person.
pub fn is_auto(a: &StoredAccount) -> bool {
    match a.info.broker {
        Broker::Zerodha => a.api_secret.is_some() && !extra(a, "user_id").is_empty() && !extra(a, "password").is_empty() && !extra(a, "totp_secret").is_empty(),
        Broker::Angel | Broker::Kotak => true,
    }
}

/// Build the poller's broker connection for a stored account.
pub fn build_live(a: &StoredAccount, ctl: Arc<Control>) -> Live {
    let auto = is_auto(a);
    match a.info.broker {
        Broker::Zerodha => {
            let mut k = KiteBroker::new(a.api_key.clone(), a.access_token.clone());
            if auto {
                k = k.with_auto(KiteAuto { api_secret: a.api_secret.clone().unwrap_or_default(), user_id: extra(a, "user_id").into(), password: extra(a, "password").into(), totp_secret: extra(a, "totp_secret").into() });
            }
            let kite = Arc::new(k);
            Live { adapter: kite.clone(), kite: Some(kite), ctl }
        }
        Broker::Angel => Live { adapter: Arc::new(AngelBroker::new(&a.api_key, extra(a, "client_code"), extra(a, "pin"), extra(a, "totp_secret"))), kite: None, ctl },
        Broker::Kotak => Live { adapter: Arc::new(KotakBroker::new(&a.api_key, extra(a, "mobile"), extra(a, "ucc"), extra(a, "mpin"), extra(a, "totp_secret"))), kite: None, ctl },
    }
}

fn broker_word(b: Broker) -> &'static str {
    match b {
        Broker::Zerodha => "ZERODHA",
        Broker::Angel => "ANGEL",
        Broker::Kotak => "KOTAK",
    }
}

#[derive(Default)]
pub struct Store {
    pub vault: Option<Vault>,
    pub data: Persisted,
    pub live: HashMap<String, Live>,
    pub poll: Duration,
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "error": msg }))).into_response()
}

/// Trimmed, single-line, bounded. Keys and tokens are short ASCII strings; anything else is a paste mistake.
fn clean(v: &Option<String>, max: usize) -> Option<String> {
    let t = v.as_deref()?.trim();
    if t.is_empty() || t.len() > max || t.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    Some(t.to_string())
}

fn present(v: &Option<String>) -> bool {
    v.as_deref().is_some_and(|s| !s.trim().is_empty())
}

#[derive(Deserialize)]
pub struct NewClient {
    name: String,
    group: String,
}

#[derive(Deserialize)]
pub struct AddReq {
    client_id: Option<String>,
    new_client: Option<NewClient>,
    broker: Broker,
    label: Option<String>,
    api_key: String,
    api_secret: Option<String>,
    access_token: Option<String>,
    request_token: Option<String>,
    /// Broker-specific fields: user_id, password, client_code, pin, mobile, ucc, mpin, totp_secret.
    extra: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
pub struct SessionReq {
    access_token: Option<String>,
    request_token: Option<String>,
}

/// An access token, from a pasted one or by exchanging a login request token. `None` means none was given.
async fn resolve_token(api_key: &str, secret: Option<&str>, access: &Option<String>, request: &Option<String>) -> Result<Option<String>, String> {
    if present(access) {
        return clean(access, 128).map(Some).ok_or_else(|| "The access token has spaces or is too long. Paste only the token.".to_string());
    }
    if present(request) {
        let rt = clean(request, 128).ok_or_else(|| "The request token has spaces or is too long. Paste only the token.".to_string())?;
        let secret = secret.ok_or_else(|| "An API secret is needed to turn a request token into an access token.".to_string())?;
        return exchange_request_token(None, api_key, secret, &rt).await.map(Some).map_err(|e| e.to_string());
    }
    Ok(None)
}

/// A password, PIN or MPIN: any printable text, but not empty and not absurdly long.
fn clean_free(v: &str, max: usize) -> Option<String> {
    let t = v.trim();
    if t.is_empty() || t.len() > max || t.chars().any(|c| c.is_control()) {
        return None;
    }
    Some(t.to_string())
}

/// The fields each broker needs, checked and tidied. A mistake here is told to the admin in plain words.
fn check_extra(broker: Broker, api_secret: &Option<String>, given: &HashMap<String, String>) -> Result<BTreeMap<String, String>, String> {
    let get = |k: &str| given.get(k).map(String::as_str).unwrap_or("");
    let mut out = BTreeMap::new();
    let ident = |k: &str, label: &str, out: &mut BTreeMap<String, String>| -> Result<(), String> {
        let v = clean(&Some(get(k).to_string()), 40).ok_or_else(|| format!("{label} is needed, without spaces."))?;
        out.insert(k.to_string(), v);
        Ok(())
    };
    let secret_field = |k: &str, label: &str, out: &mut BTreeMap<String, String>| -> Result<(), String> {
        let v = clean_free(get(k), 128).ok_or_else(|| format!("{label} is needed."))?;
        out.insert(k.to_string(), v);
        Ok(())
    };
    let totp_field = |out: &mut BTreeMap<String, String>| -> Result<(), String> {
        let v = totp::normalize_secret(get("totp_secret")).ok_or_else(|| "The TOTP secret is not valid. Use the text key shown under the QR code when TOTP was set up, not a 6-digit code.".to_string())?;
        out.insert("totp_secret".into(), v);
        Ok(())
    };
    match broker {
        Broker::Angel => {
            ident("client_code", "The client code", &mut out)?;
            secret_field("pin", "The MPIN", &mut out)?;
            totp_field(&mut out)?;
        }
        Broker::Kotak => {
            ident("mobile", "The registered mobile number", &mut out)?;
            ident("ucc", "The client code (UCC)", &mut out)?;
            secret_field("mpin", "The MPIN", &mut out)?;
            totp_field(&mut out)?;
        }
        Broker::Zerodha => {
            // Optional as a set: all of user id, password and TOTP secret (plus the API secret) for daily automatic login, or none.
            let any = ["user_id", "password", "totp_secret"].iter().any(|k| !get(k).trim().is_empty());
            if any {
                ident("user_id", "The Zerodha user id", &mut out)?;
                secret_field("password", "The Kite password", &mut out)?;
                totp_field(&mut out)?;
                if api_secret.is_none() {
                    return Err("Automatic login also needs the API secret.".into());
                }
            }
        }
    }
    Ok(out)
}

pub async fn list(State(s): State<Shared>, user: AuthUser) -> Response {
    if !user.is_admin() {
        return err(StatusCode::FORBIDDEN, "Only the administrator can manage accounts");
    }
    let store = s.store.lock().await;
    let rows: Vec<_> = store
        .data
        .accounts
        .iter()
        .map(|a| {
            json!({
                "account_id": a.info.account_id,
                "client_id": a.info.client_id,
                "broker": a.info.broker,
                "label": a.info.label,
                "has_secret": a.api_secret.is_some(),
                "has_token": !a.access_token.is_empty(),
                "auto_login": is_auto(a),
                "token_at": a.token_at,
            })
        })
        .collect();
    Json(json!({ "enabled": store.vault.is_some(), "accounts": rows })).into_response()
}

pub async fn add(State(s): State<Shared>, user: AuthUser, body: Result<Json<AddReq>, JsonRejection>) -> Response {
    if !user.is_admin() {
        return err(StatusCode::FORBIDDEN, "Only the administrator can manage accounts");
    }
    let Ok(Json(req)) = body else { return err(StatusCode::BAD_REQUEST, "The form was incomplete") };
    let Some(api_key) = clean(&Some(req.api_key.clone()), 64) else {
        return err(StatusCode::BAD_REQUEST, "Enter the API key, without spaces.");
    };
    let secret = if present(&req.api_secret) {
        match clean(&req.api_secret, 128) {
            Some(v) => Some(v),
            None => return err(StatusCode::BAD_REQUEST, "The API secret has spaces or is too long."),
        }
    } else {
        None
    };
    let extra_in = req.extra.clone().unwrap_or_default();
    let extra = match check_extra(req.broker, &secret, &extra_in) {
        Ok(e) => e,
        Err(m) => return err(StatusCode::BAD_REQUEST, &m),
    };
    if req.broker != Broker::Zerodha && (present(&req.access_token) || present(&req.request_token)) {
        return err(StatusCode::BAD_REQUEST, "This broker signs in by itself, so no token is needed.");
    }
    let label = req.label.as_deref().map(str::trim).filter(|l| !l.is_empty()).map(|l| l.chars().take(40).collect::<String>());
    if let Some(nc) = &req.new_client {
        if nc.name.trim().is_empty() || nc.name.trim().len() > 60 || nc.group.trim().len() > 40 {
            return err(StatusCode::BAD_REQUEST, "Give the new client a name (up to 60 letters) and a desk (up to 40).");
        }
    }
    if req.client_id.is_none() && req.new_client.is_none() {
        return err(StatusCode::BAD_REQUEST, "Pick a client, or add a new one.");
    }

    // The network call to exchange a request token happens before any lock is held.
    let token = if req.broker == Broker::Zerodha {
        match resolve_token(&api_key, secret.as_deref(), &req.access_token, &req.request_token).await {
            Ok(t) => t,
            Err(m) => return err(StatusCode::BAD_REQUEST, &m),
        }
    } else {
        None
    };

    let mut store = s.store.lock().await;
    if store.vault.is_none() {
        return err(StatusCode::SERVICE_UNAVAILABLE, "Account storage is switched off on this server.");
    }
    // The same broker login added twice would run two pollers that keep signing each other out.
    let who = |e: &BTreeMap<String, String>| ["user_id", "client_code", "ucc"].iter().find_map(|k| e.get(*k).cloned()).unwrap_or_default();
    let me = who(&extra);
    if store.data.accounts.iter().any(|a| a.info.broker == req.broker && a.api_key == api_key && who(&a.extra) == me) {
        return err(StatusCode::CONFLICT, "This broker login is already added. Remove it first if you want to replace it.");
    }
    let mut inner = s.inner.write().await;

    // Client: an existing one, or a new one created with the account.
    let mut new_client: Option<ClientInfo> = None;
    let client_id = match (&req.client_id, &req.new_client) {
        (Some(id), _) => {
            if !inner.clients.iter().any(|c| &c.id == id) {
                return err(StatusCode::BAD_REQUEST, "That client does not exist.");
            }
            id.clone()
        }
        (None, Some(nc)) => {
            let mut n = inner.clients.len() + 1;
            while inner.clients.iter().any(|c| c.id == format!("c{n}")) {
                n += 1;
            }
            let id = format!("c{n}");
            new_client = Some(ClientInfo { id: id.clone(), name: nc.name.trim().to_string(), group: nc.group.trim().to_string() });
            id
        }
        (None, None) => unreachable!("checked above"),
    };

    let word = broker_word(req.broker);
    let mut account_id = format!("{client_id}-{word}");
    let mut k = 2;
    while inner.health.iter().any(|h| h.info.account_id == account_id) {
        account_id = format!("{client_id}-{word}-{k}");
        k += 1;
    }
    let default_label = match req.broker {
        Broker::Zerodha => "Kite",
        Broker::Angel => "Angel One",
        Broker::Kotak => "Kotak Neo",
    };
    let info = AccountInfo { account_id: account_id.clone(), client_id: client_id.clone(), broker: req.broker, label: label.unwrap_or_else(|| default_label.into()) };
    let stored = StoredAccount { info: info.clone(), api_key: api_key.clone(), api_secret: secret, access_token: token.clone().unwrap_or_default(), token_at: token.as_ref().map(|_| Utc::now()), extra };

    // Save first: if the disk write fails nothing is added in memory either.
    let mut next = store.data.clone();
    if let Some(c) = &new_client {
        next.clients.push(c.clone());
    }
    next.accounts.push(stored);
    if let Err(e) = store.vault.as_ref().expect("checked").save(&next) {
        tracing::error!(error = %e, "could not save the account vault");
        return err(StatusCode::INTERNAL_SERVER_ERROR, "Could not save the account. Check the server's data folder.");
    }
    store.data = next;

    if let Some(c) = new_client {
        inner.clients.push(c);
    }
    inner.health.push(AccountHealth { info: info.clone(), status: HealthStatus::Connecting, last_ok: None, last_error: None, latency_ms: None, polls: 0 });
    drop(inner);

    let live = build_live(store.data.accounts.last().expect("just added"), Arc::new(Control::default()));
    monitor::spawn_account(s.clone(), info.clone(), live.adapter.clone(), store.poll, live.ctl.clone());
    store.live.insert(account_id.clone(), live);
    drop(store);
    let _ = s.notify.send(());
    (StatusCode::CREATED, Json(json!({ "account_id": account_id }))).into_response()
}

/// Give an account today's token (Kite tokens expire every morning).
pub async fn set_session(State(s): State<Shared>, user: AuthUser, Path(id): Path<String>, body: Result<Json<SessionReq>, JsonRejection>) -> Response {
    if !user.is_admin() {
        return err(StatusCode::FORBIDDEN, "Only the administrator can manage accounts");
    }
    let req = body.map(|Json(r)| r).unwrap_or(SessionReq { access_token: None, request_token: None });
    let (api_key, secret, auto, broker) = {
        let store = s.store.lock().await;
        match store.data.accounts.iter().find(|a| a.info.account_id == id) {
            Some(a) => (a.api_key.clone(), a.api_secret.clone(), is_auto(a), a.info.broker),
            None => return err(StatusCode::NOT_FOUND, "Account not found"),
        }
    };
    // Nothing to paste: just ask the poller to try its automatic login again now.
    if !present(&req.access_token) && !present(&req.request_token) {
        if auto {
            let store = s.store.lock().await;
            if let Some(l) = store.live.get(&id) {
                l.ctl.wake.notify_one();
            }
            return StatusCode::NO_CONTENT.into_response();
        }
        return err(StatusCode::BAD_REQUEST, "Paste an access token or a request token.");
    }
    if broker != Broker::Zerodha {
        return err(StatusCode::BAD_REQUEST, "This broker signs in by itself, so no token is needed. Use Retry sign-in.");
    }
    let token = match resolve_token(&api_key, secret.as_deref(), &req.access_token, &req.request_token).await {
        Ok(Some(t)) => t,
        Ok(None) => return err(StatusCode::BAD_REQUEST, "Paste an access token or a request token."),
        Err(m) => return err(StatusCode::BAD_REQUEST, &m),
    };
    let mut store = s.store.lock().await;
    let Some(pos) = store.data.accounts.iter().position(|a| a.info.account_id == id) else { return err(StatusCode::NOT_FOUND, "Account not found") };
    let mut next = store.data.clone();
    next.accounts[pos].access_token = token.clone();
    next.accounts[pos].token_at = Some(Utc::now());
    if let Err(e) = store.vault.as_ref().map(|v| v.save(&next)).unwrap_or(Ok(())) {
        tracing::error!(error = %e, "could not save the account vault");
        return err(StatusCode::INTERNAL_SERVER_ERROR, "Could not save the token. Check the server's data folder.");
    }
    store.data = next;
    if let Some(l) = store.live.get(&id) {
        if let Some(k) = &l.kite {
            k.set_token(&token);
        }
        l.ctl.wake.notify_one(); // retry now instead of after the back-off
    }
    StatusCode::NO_CONTENT.into_response()
}

pub async fn remove(State(s): State<Shared>, user: AuthUser, Path(id): Path<String>) -> Response {
    if !user.is_admin() {
        return err(StatusCode::FORBIDDEN, "Only the administrator can manage accounts");
    }
    let mut store = s.store.lock().await;
    let Some(pos) = store.data.accounts.iter().position(|a| a.info.account_id == id) else { return err(StatusCode::NOT_FOUND, "Account not found") };
    let mut next = store.data.clone();
    next.accounts.remove(pos);
    if let Err(e) = store.vault.as_ref().map(|v| v.save(&next)).unwrap_or(Ok(())) {
        tracing::error!(error = %e, "could not save the account vault");
        return err(StatusCode::INTERNAL_SERVER_ERROR, "Could not remove the account. Check the server's data folder.");
    }
    store.data = next;
    if let Some(l) = store.live.remove(&id) {
        l.ctl.stop.store(true, Ordering::Relaxed);
        l.ctl.wake.notify_one();
    }
    drop(store);
    let mut inner = s.inner.write().await;
    inner.health.retain(|h| h.info.account_id != id);
    inner.snapshots.remove(&id);
    inner.legs.remove(&id);
    inner.incidents.incidents.retain(|i| i.account_id != id);
    drop(inner);
    let _ = s.notify.send(());
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_rejects_spaces_and_control_characters() {
        assert_eq!(clean(&Some(" abc123 ".into()), 20).as_deref(), Some("abc123"));
        assert!(clean(&Some("ab c".into()), 20).is_none());
        assert!(clean(&Some("ab\nc".into()), 20).is_none());
        assert!(clean(&Some("x".repeat(30)), 20).is_none());
        assert!(clean(&Some("   ".into()), 20).is_none());
    }
}
