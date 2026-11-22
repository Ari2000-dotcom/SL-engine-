//! Angel One SmartAPI over HTTPS. Read-only: sign-in, then GET order book, trade book and positions,
//! plus one POST for last prices. Nothing here can place, modify or cancel an order.
//!
//! Signs in by itself with the client code, MPIN and a TOTP code made from the stored secret. The
//! session (JWT) lasts until midnight; when SmartAPI says it is over, the poller signs in again.

use crate::adapter::{Books, BrokerAdapter, BrokerError, InstrumentKey};
use crate::angel;
use crate::util::ist;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use sl_core::Broker;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, RwLock};
use std::time::Duration;

const DEFAULT_BASE: &str = "https://apiconnect.angelone.in";
const LOGIN_PATH: &str = "/rest/auth/angelbroking/user/v1/loginByPassword";
const POSITION_PATH: &str = "/rest/secure/angelbroking/order/v1/getPosition";
const ORDER_BOOK_PATH: &str = "/rest/secure/angelbroking/order/v1/getOrderBook";
const TRADE_BOOK_PATH: &str = "/rest/secure/angelbroking/order/v1/getTradeBook";
const QUOTE_PATH: &str = "/rest/secure/angelbroking/market/v1/quote/";

/// SmartAPI answers an expired session with HTTP 200 and `status: false`, so these codes are what tells
/// "the session is over" apart from an ordinary refusal.
const AUTH_ERROR_CODES: [&str; 6] = ["AG8001", "AG8002", "AG8003", "AB8050", "AB8051", "AB1010"];

pub struct AngelBroker {
    base: String,
    api_key: String,
    client_code: String,
    pin: String,
    totp_secret: String,
    jwt: RwLock<String>,
    /// (exchange, trading symbol) -> (exchange name, symbol token), learned from the positions book.
    tokens: Mutex<HashMap<InstrumentKey, (String, String)>>,
    http: reqwest::Client,
}

impl std::fmt::Debug for AngelBroker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AngelBroker").field("base", &self.base).finish_non_exhaustive()
    }
}

/// SmartAPI shows an API key (short) and a secret key (a UUID). Sign-in accepts the secret in the
/// key's place and then every account read fails, which looks like an empty account.
pub fn api_key_problem(key: &str) -> Option<&'static str> {
    let parts: Vec<&str> = key.trim().split('-').collect();
    let uuid = parts.len() == 5 && [8, 4, 4, 4, 12].iter().zip(&parts).all(|(n, p)| p.len() == *n && p.chars().all(|c| c.is_ascii_hexdigit()));
    uuid.then_some("That is Angel's secret key (the long code with dashes). Enter the API key instead, the short code shown next to it in the SmartAPI dashboard.")
}

impl AngelBroker {
    pub fn new(api_key: &str, client_code: &str, pin: &str, totp_secret: &str) -> Self {
        Self::with_base(DEFAULT_BASE, api_key, client_code, pin, totp_secret)
    }

    pub fn with_base(base: &str, api_key: &str, client_code: &str, pin: &str, totp_secret: &str) -> Self {
        AngelBroker {
            base: base.trim_end_matches('/').to_string(),
            api_key: api_key.trim().to_string(),
            client_code: client_code.trim().to_string(),
            pin: pin.to_string(),
            totp_secret: totp_secret.to_string(),
            jwt: RwLock::new(String::new()),
            tokens: Mutex::new(HashMap::new()),
            http: reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(15)).user_agent("sl-guardian/0.1").build().expect("http client"),
        }
    }

    async fn call(&self, method: reqwest::Method, path: &str, authed: bool, body: Option<Value>) -> Result<Value, BrokerError> {
        let mut req = self
            .http
            .request(method, format!("{}{path}", self.base))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header("X-UserType", "USER")
            .header("X-SourceID", "WEB")
            // Required by SmartAPI but not validated; loopback keeps the machine's real address out of it.
            .header("X-ClientLocalIP", "127.0.0.1")
            .header("X-ClientPublicIP", "127.0.0.1")
            .header("X-MACAddress", "00:00:00:00:00:00")
            .header("X-PrivateKey", &self.api_key);
        if authed {
            let jwt = self.jwt.read().unwrap().clone();
            if jwt.is_empty() {
                return Err(BrokerError::SessionExpired);
            }
            req = req.header("Authorization", format!("Bearer {jwt}"));
        }
        if let Some(b) = &body {
            req = req.json(b);
        }
        let res = req.send().await.map_err(|e| BrokerError::Http(e.without_url().to_string()))?;
        let status = res.status().as_u16();
        let text = res.text().await.map_err(|e| BrokerError::Http(e.without_url().to_string()))?;
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        interpret(status, &v)
    }
}

/// Angel uses two envelope shapes (`status`/`errorcode` and `success`/`errorCode`) and answers failures with HTTP 200.
fn interpret(status: u16, v: &Value) -> Result<Value, BrokerError> {
    let ok = v["status"].as_bool().or_else(|| v["success"].as_bool()).unwrap_or(status < 400);
    if status < 400 && ok {
        return Ok(v["data"].clone());
    }
    let code = v["errorcode"].as_str().or_else(|| v["errorCode"].as_str()).unwrap_or("").to_ascii_uppercase();
    if AUTH_ERROR_CODES.contains(&code.as_str()) || status == 401 {
        return Err(BrokerError::SessionExpired);
    }
    // Angel's throttle is a bare HTTP 403 with no error code.
    if status == 429 || (status == 403 && code.is_empty()) {
        return Err(BrokerError::RateLimited);
    }
    let msg = v["message"].as_str().unwrap_or("no message");
    if code == "AG8004" {
        return Err(BrokerError::Auth("Angel refuses this API key for account data (Invalid API Key) though sign-in accepts it. Check the key and its app in the SmartAPI dashboard.".into()));
    }
    if status < 400 {
        // HTTP 200 with status false and no known session code: understood and refused.
        return Err(BrokerError::Auth(if code.is_empty() { msg.to_string() } else { format!("{msg} [{code}]") }));
    }
    Err(BrokerError::Http(format!("HTTP {status}: {msg}")))
}

fn text(v: &Value, key: &str) -> String {
    match &v[key] {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

#[async_trait]
impl BrokerAdapter for AngelBroker {
    fn broker(&self) -> Broker {
        Broker::Angel
    }

    async fn login(&self) -> Result<(), BrokerError> {
        if let Some(p) = api_key_problem(&self.api_key) {
            return Err(BrokerError::Auth(p.into()));
        }
        let totp = crate::totp::fresh_code(&self.totp_secret).await.map_err(BrokerError::Auth)?;
        let data = self
            .call(reqwest::Method::POST, LOGIN_PATH, false, Some(json!({ "clientcode": self.client_code, "password": self.pin, "totp": totp })))
            .await
            .map_err(|e| match e {
                BrokerError::SessionExpired | BrokerError::RateLimited => BrokerError::Auth("Angel One sign-in was refused. Check the client code, MPIN and TOTP secret.".into()),
                other => other,
            })?;
        let jwt = text(&data, "jwtToken");
        if jwt.is_empty() {
            return Err(BrokerError::Auth("Angel login returned no jwtToken".into()));
        }
        *self.jwt.write().unwrap() = jwt;
        Ok(())
    }

    async fn fetch_books(&self) -> Result<Books, BrokerError> {
        let now = Utc::now();
        let orders = self.call(reqwest::Method::GET, ORDER_BOOK_PATH, true, None).await?;
        let trades = self.call(reqwest::Method::GET, TRADE_BOOK_PATH, true, None).await?;
        let positions = self.call(reqwest::Method::GET, POSITION_PATH, true, None).await?;

        // Remember each position's symbol token: the quote call needs it and the book is the only place it appears.
        let mut map = HashMap::new();
        for row in positions.as_array().into_iter().flatten() {
            if let Some(exchange) = angel::exchange(&text(row, "exchange")) {
                let (tok, sym) = (text(row, "symboltoken"), text(row, "tradingsymbol"));
                if !tok.is_empty() && !sym.is_empty() {
                    map.insert(InstrumentKey { exchange, symbol: sym }, (text(row, "exchange"), tok));
                }
            }
        }
        *self.tokens.lock().unwrap() = map;

        let today = now.with_timezone(&ist()).date_naive();
        Ok(Books { orders: angel::parse_orders(&orders, now), trades: angel::parse_trades(&trades, today, now), positions: angel::parse_positions(&positions) })
    }

    async fn fetch_ltp(&self, keys: &[InstrumentKey]) -> Result<HashMap<InstrumentKey, f64>, BrokerError> {
        let known: Vec<(InstrumentKey, String, String)> = {
            let t = self.tokens.lock().unwrap();
            keys.iter().filter_map(|k| t.get(k).map(|(ex, tok)| (k.clone(), ex.clone(), tok.clone()))).collect()
        };
        let mut out = HashMap::new();
        // The quote call takes at most 50 tokens.
        for chunk in known.chunks(50) {
            let mut by_exchange: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (_, ex, tok) in chunk {
                by_exchange.entry(ex.clone()).or_default().push(tok.clone());
            }
            let data = self.call(reqwest::Method::POST, QUOTE_PATH, true, Some(json!({ "mode": "LTP", "exchangeTokens": by_exchange }))).await?;
            for row in data["fetched"].as_array().into_iter().flatten() {
                let ltp = match &row["ltp"] {
                    Value::Number(n) => n.as_f64(),
                    Value::String(s) => s.parse().ok(),
                    _ => None,
                };
                let tok = text(row, "symbolToken");
                if let (Some(ltp), Some((k, _, _))) = (ltp, chunk.iter().find(|(_, _, t)| *t == tok)) {
                    out.insert(k.clone(), ltp);
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_secret_key_is_refused_where_the_api_key_belongs() {
        assert!(api_key_problem("1b4c3f1e-9d5a-4a3e-8c11-0a1b2c3d4e5f").is_some());
        assert!(api_key_problem("AbCd1234").is_none());
    }

    #[test]
    fn both_envelopes_are_read() {
        assert!(interpret(200, &json!({"status": true, "data": {"a": 1}})).is_ok());
        assert!(matches!(interpret(200, &json!({"status": false, "errorcode": "AG8002", "message": "x"})), Err(BrokerError::SessionExpired)));
        assert!(matches!(interpret(200, &json!({"success": false, "errorCode": "AG8001"})), Err(BrokerError::SessionExpired)));
        assert!(matches!(interpret(403, &json!({})), Err(BrokerError::RateLimited)));
        assert!(matches!(interpret(200, &json!({"status": false, "errorcode": "AB1007", "message": "bad totp"})), Err(BrokerError::Auth(_))));
    }

    #[test]
    fn debug_never_prints_secrets() {
        let a = AngelBroker::new("key", "A1", "4321", "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
        let s = format!("{a:?}");
        assert!(!s.contains("4321") && !s.contains("GEZDGNBV"));
    }
}
