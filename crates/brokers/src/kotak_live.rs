//! Kotak Neo over HTTPS. Read-only: two-step sign-in, then GET order book, trade book and positions,
//! plus one GET for last prices. Nothing here can place, modify or cancel an order.
//!
//! Signs in by itself: the TOTP code made from the stored secret gives a view token, and the MPIN turns
//! that into a trade token, a session id and the data-centre address that every later call goes to.
//! Details follow QuantDesk's working adapter.

use crate::adapter::{Books, BrokerAdapter, BrokerError, InstrumentKey};
use crate::kotak;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use sl_core::Broker;
use std::collections::HashMap;
use std::sync::{Mutex, RwLock};
use std::time::Duration;

const DEFAULT_LOGIN_BASE: &str = "https://mis.kotaksecurities.com";
/// Required on every call. A constant, not a secret; leaving it out fails with a message about "NeoFinKey".
const NEO_FIN_KEY: &str = "neotradeapi";

#[derive(Clone)]
struct Session {
    token: String,
    sid: String,
    base_url: String,
}

pub struct KotakBroker {
    login_base: String,
    /// The app's access token from the Neo API dashboard (a long-lived per-app credential).
    consumer_token: String,
    mobile: String,
    ucc: String,
    mpin: String,
    totp_secret: String,
    session: RwLock<Option<Session>>,
    /// (exchange, trading symbol) -> "segment|token", learned from the positions book, for the quote call.
    quote_keys: Mutex<HashMap<InstrumentKey, String>>,
    http: reqwest::Client,
}

impl std::fmt::Debug for KotakBroker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KotakBroker").finish_non_exhaustive()
    }
}

impl KotakBroker {
    pub fn new(consumer_token: &str, mobile: &str, ucc: &str, mpin: &str, totp_secret: &str) -> Self {
        Self::with_login_base(DEFAULT_LOGIN_BASE, consumer_token, mobile, ucc, mpin, totp_secret)
    }

    pub fn with_login_base(base: &str, consumer_token: &str, mobile: &str, ucc: &str, mpin: &str, totp_secret: &str) -> Self {
        KotakBroker {
            login_base: base.trim_end_matches('/').to_string(),
            consumer_token: consumer_token.trim().to_string(),
            mobile: mobile.trim().to_string(),
            ucc: ucc.trim().to_string(),
            mpin: mpin.to_string(),
            totp_secret: totp_secret.to_string(),
            session: RwLock::new(None),
            quote_keys: Mutex::new(HashMap::new()),
            http: reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(15)).user_agent("sl-guardian/0.1").build().expect("http client"),
        }
    }

    /// Kotak answers a failed login with HTTP 200 and `status: "error"`, so the body is checked, not the status.
    async fn post_login(&self, path: &str, extra: &[(&str, &str)], body: Value) -> Result<Value, BrokerError> {
        let mut req = self
            .http
            .post(format!("{}{path}", self.login_base))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header("neo-fin-key", NEO_FIN_KEY)
            .header("Authorization", &self.consumer_token);
        for (k, v) in extra {
            req = req.header(*k, *v);
        }
        let res = req.json(&body).send().await.map_err(|e| BrokerError::Http(e.without_url().to_string()))?;
        let status = res.status().as_u16();
        let text = res.text().await.map_err(|e| BrokerError::Http(e.without_url().to_string()))?;
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let errored = text_of(&v, "status").eq_ignore_ascii_case("error") || text_of(&v, "stat").eq_ignore_ascii_case("not_ok");
        if status >= 400 || errored {
            return Err(BrokerError::Auth(format!("Kotak: {}", message_of(&v, status))));
        }
        Ok(if v["data"].is_object() { v["data"].clone() } else { v })
    }

    /// One read-only GET against the account's own data centre.
    async fn get(&self, path: &str) -> Result<Value, BrokerError> {
        let s = self.session.read().unwrap().clone().ok_or(BrokerError::SessionExpired)?;
        let res = self
            .http
            .get(format!("{}{path}", s.base_url))
            .header("Accept", "application/json")
            .header("Sid", &s.sid)
            .header("Auth", &s.token)
            .header("neo-fin-key", NEO_FIN_KEY)
            .send()
            .await
            .map_err(|e| BrokerError::Http(e.without_url().to_string()))?;
        let status = res.status().as_u16();
        let text = res.text().await.map_err(|e| BrokerError::Http(e.without_url().to_string()))?;
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        read_reply(status, &v)
    }
}

fn text_of(v: &Value, key: &str) -> String {
    match &v[key] {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn message_of(v: &Value, status: u16) -> String {
    ["emsg", "errMsg", "message", "error"].iter().map(|k| text_of(v, k)).find(|m| !m.is_empty()).unwrap_or_else(|| format!("HTTP {status}"))
}

/// A report with nothing in it comes back as `stat: Not_Ok`, `stCode: 5203`, "No Data". That is an
/// empty book, not a failed read.
fn is_no_data(v: &Value) -> bool {
    text_of(v, "stCode") == "5203" || ["errMsg", "emsg"].iter().any(|k| text_of(v, k).eq_ignore_ascii_case("no data"))
}

/// Judge one read. Only a 401/403 or a message about the session or token means the session is over; any
/// other refusal stays an ordinary error so it can never log a healthy account out.
fn read_reply(status: u16, v: &Value) -> Result<Value, BrokerError> {
    if status < 400 && is_no_data(v) {
        return Ok(json!([]));
    }
    let errored = text_of(v, "status").eq_ignore_ascii_case("error") || text_of(v, "stat").eq_ignore_ascii_case("not_ok");
    if status >= 400 || errored {
        let msg = message_of(v, status);
        let lower = msg.to_ascii_lowercase();
        if matches!(status, 401 | 403) || ["session", "token", "unauthori", "invalid sid", "login again"].iter().any(|w| lower.contains(w)) {
            return Err(BrokerError::SessionExpired);
        }
        if status == 429 {
            return Err(BrokerError::RateLimited);
        }
        return Err(BrokerError::Http(format!("Kotak: {msg}")));
    }
    Ok(v["data"].clone())
}

#[async_trait]
impl BrokerAdapter for KotakBroker {
    fn broker(&self) -> Broker {
        Broker::Kotak
    }

    async fn login(&self) -> Result<(), BrokerError> {
        let totp = crate::totp::fresh_code(&self.totp_secret).await.map_err(BrokerError::Auth)?;
        // 1. TOTP gives a view token.
        let view = self.post_login("/login/1.0/tradeApiLogin", &[], json!({ "mobileNumber": self.mobile, "ucc": self.ucc, "totp": totp })).await?;
        let (view_token, view_sid) = (text_of(&view, "token"), text_of(&view, "sid"));
        if view_token.is_empty() || view_sid.is_empty() {
            return Err(BrokerError::Auth("Kotak login returned no view token".into()));
        }
        // 2. MPIN turns it into the trade token and the data-centre address.
        let v = self.post_login("/login/1.0/tradeApiValidate", &[("sid", view_sid.as_str()), ("Auth", view_token.as_str())], json!({ "mpin": self.mpin })).await?;
        let token = text_of(&v, "token");
        let base = text_of(&v, "baseUrl").trim_end_matches('/').to_string();
        if token.is_empty() {
            return Err(BrokerError::Auth("Kotak MPIN check returned no trade token".into()));
        }
        if base.is_empty() {
            return Err(BrokerError::Auth("Kotak returned no baseUrl, so this account's data centre is unknown".into()));
        }
        let sid = Some(text_of(&v, "sid")).filter(|s| !s.is_empty()).unwrap_or(view_sid);
        *self.session.write().unwrap() = Some(Session { token, sid, base_url: base });
        Ok(())
    }

    async fn fetch_books(&self) -> Result<Books, BrokerError> {
        let now = Utc::now();
        let orders = self.get("/quick/user/orders").await?;
        let trades = self.get("/quick/user/trades").await?;
        let positions = self.get("/quick/user/positions").await?;

        let mut map = HashMap::new();
        for row in positions.as_array().into_iter().flatten() {
            if let Some(exchange) = kotak::exchange(&text_of(row, "exSeg")) {
                let (tok, sym) = (text_of(row, "tok"), text_of(row, "trdSym"));
                if !tok.is_empty() && !sym.is_empty() {
                    map.insert(InstrumentKey { exchange, symbol: sym }, format!("{}|{tok}", text_of(row, "exSeg").to_ascii_lowercase()));
                }
            }
        }
        *self.quote_keys.lock().unwrap() = map;

        Ok(Books { orders: kotak::parse_orders(&orders, now), trades: kotak::parse_trades(&trades, now), positions: kotak::parse_positions(&positions) })
    }

    async fn fetch_ltp(&self, keys: &[InstrumentKey]) -> Result<HashMap<InstrumentKey, f64>, BrokerError> {
        let known: Vec<(InstrumentKey, String)> = {
            let t = self.quote_keys.lock().unwrap();
            keys.iter().filter_map(|k| t.get(k).map(|q| (k.clone(), q.clone()))).collect()
        };
        let mut out = HashMap::new();
        let Some(s) = self.session.read().unwrap().clone() else { return Err(BrokerError::SessionExpired) };
        for chunk in known.chunks(50) {
            let list = chunk.iter().map(|(_, q)| q.as_str()).collect::<Vec<_>>().join(",").replace('|', "%7C").replace(',', "%2C");
            let res = self
                .http
                .get(format!("{}/script-details/1.0/quotes/neosymbol/{list}/ltp", s.base_url))
                .header("Accept", "application/json")
                .header("Authorization", &self.consumer_token)
                .send()
                .await
                .map_err(|e| BrokerError::Http(e.without_url().to_string()))?;
            if !res.status().is_success() {
                return Err(BrokerError::Http(format!("Kotak quotes: HTTP {}", res.status().as_u16())));
            }
            let body: Value = res.json().await.map_err(|e| BrokerError::Parse(e.without_url().to_string()))?;
            let rows = body.as_array().cloned().or_else(|| body["data"].as_array().cloned()).unwrap_or_default();
            for q in &rows {
                let token = ["exchange_token", "tok", "tk"].iter().map(|k| text_of(q, k)).find(|t| !t.is_empty()).unwrap_or_default();
                let ltp = ["ltp", "last_traded_price"].iter().filter_map(|k| text_of(q, k).parse::<f64>().ok()).find(|v| *v > 0.0);
                if let (Some(ltp), false) = (ltp, token.is_empty()) {
                    if let Some((k, _)) = chunk.iter().find(|(_, qk)| qk.ends_with(&format!("|{token}"))) {
                        out.insert(k.clone(), ltp);
                    }
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
    fn no_data_is_an_empty_book_not_a_refusal() {
        let v = json!({"stat": "Not_Ok", "stCode": 5203, "errMsg": "No Data"});
        assert_eq!(read_reply(200, &v).unwrap(), json!([]));
    }

    #[test]
    fn only_session_problems_end_the_session() {
        assert!(matches!(read_reply(200, &json!({"stat": "Not_Ok", "emsg": "Invalid Sid, login again"})), Err(BrokerError::SessionExpired)));
        assert!(matches!(read_reply(401, &json!({})), Err(BrokerError::SessionExpired)));
        assert!(matches!(read_reply(200, &json!({"stat": "Not_Ok", "emsg": "Something else went wrong"})), Err(BrokerError::Http(_))));
    }

    #[test]
    fn a_good_reply_returns_data() {
        assert_eq!(read_reply(200, &json!({"stat": "Ok", "data": [1, 2]})).unwrap(), json!([1, 2]));
    }

    #[test]
    fn debug_never_prints_secrets() {
        let k = KotakBroker::new("consumer", "+919999999999", "U123", "654321", "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
        let s = format!("{k:?}");
        assert!(!s.contains("654321") && !s.contains("consumer") && !s.contains("GEZDGNBV"));
    }
}
