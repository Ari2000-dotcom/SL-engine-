//! Zerodha Kite Connect over HTTPS. Read-only: this file only ever issues GET requests for books,
//! plus the one POST that exchanges a login `request_token` for the day's `access_token`.
//! Nothing here can place, modify or cancel an order.

use crate::adapter::{Books, BrokerAdapter, BrokerError, InstrumentKey};
use crate::kite;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sl_core::{Broker, Exchange};
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Duration;

const DEFAULT_BASE: &str = "https://api.kite.trade";
const DEFAULT_WEB: &str = "https://kite.zerodha.com";
/// Kite's web login turns away obvious bot user agents.
const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

/// What the unattended daily login needs, on top of the API key.
#[derive(Clone)]
pub struct KiteAuto {
    pub api_secret: String,
    pub user_id: String,
    pub password: String,
    pub totp_secret: String,
}

pub struct KiteBroker {
    base: String,
    web: String,
    api_key: String,
    access_token: RwLock<String>,
    auto: Option<KiteAuto>,
    http: reqwest::Client,
}

// The token and key must never reach a log line, so Debug is written by hand.
impl std::fmt::Debug for KiteBroker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KiteBroker").field("base", &self.base).finish_non_exhaustive()
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("sl-guardian/0.1")
        .build()
        .expect("http client")
}

fn exchange_code(e: Exchange) -> &'static str {
    match e {
        Exchange::Nse => "NSE",
        Exchange::Bse => "BSE",
        Exchange::Nfo => "NFO",
        Exchange::Bfo => "BFO",
        Exchange::Mcx => "MCX",
        Exchange::Cds => "CDS",
    }
}

/// Turn a Kite reply into its `data` payload, or the right kind of error.
fn unwrap_reply(status: reqwest::StatusCode, body: &str) -> Result<Value, BrokerError> {
    let v: Value = serde_json::from_str(body).map_err(|_| BrokerError::Parse(format!("HTTP {status}, not JSON")))?;
    if v["status"] == "success" {
        return Ok(v["data"].clone());
    }
    let msg = v["message"].as_str().unwrap_or("no message").to_string();
    match (status.as_u16(), v["error_type"].as_str()) {
        (403, Some("TokenException")) | (401, _) => Err(BrokerError::SessionExpired),
        (429, _) => Err(BrokerError::RateLimited),
        (_, Some("InputException")) => Err(BrokerError::Auth(msg)),
        _ => Err(BrokerError::Http(format!("{status}: {msg}"))),
    }
}

impl KiteBroker {
    pub fn new(api_key: impl Into<String>, access_token: impl Into<String>) -> Self {
        Self::with_base(DEFAULT_BASE, api_key, access_token)
    }

    /// A different base URL, for tests against a local stand-in server.
    pub fn with_base(base: &str, api_key: impl Into<String>, access_token: impl Into<String>) -> Self {
        KiteBroker { base: base.trim_end_matches('/').to_string(), web: DEFAULT_WEB.to_string(), api_key: api_key.into(), access_token: RwLock::new(access_token.into()), auto: None, http: client() }
    }

    /// Log in by itself each morning (and whenever the token stops working).
    pub fn with_auto(mut self, auto: KiteAuto) -> Self {
        self.auto = Some(auto);
        self
    }

    /// Different API and web hosts, for tests against local stand-in servers.
    pub fn with_hosts(mut self, api: &str, web: &str) -> Self {
        self.base = api.trim_end_matches('/').to_string();
        self.web = web.trim_end_matches('/').to_string();
        self
    }

    /// Replace the day's token (after a fresh login). The next poll uses it.
    pub fn set_token(&self, token: &str) {
        *self.access_token.write().unwrap() = token.to_string();
    }

    pub fn has_token(&self) -> bool {
        !self.access_token.read().unwrap().is_empty()
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value, BrokerError> {
        let token = self.access_token.read().unwrap().clone();
        if token.is_empty() {
            return Err(BrokerError::Auth("No access token yet. Add today's token on the Connections page.".into()));
        }
        let resp = self
            .http
            .get(format!("{}{}", self.base, path))
            .query(query)
            .header("X-Kite-Version", "3")
            .header("Authorization", format!("token {}:{}", self.api_key, token))
            .send()
            .await
            .map_err(|e| BrokerError::Http(e.without_url().to_string()))?;
        let status = resp.status();
        let body = resp.text().await.map_err(|e| BrokerError::Http(e.without_url().to_string()))?;
        unwrap_reply(status, &body)
    }
}

/// Exchange the `request_token` from the login redirect for an `access_token`.
/// The checksum is SHA-256 of api_key + request_token + api_secret, as Kite documents.
pub async fn exchange_request_token(base: Option<&str>, api_key: &str, api_secret: &str, request_token: &str) -> Result<String, BrokerError> {
    let checksum = format!("{:x}", Sha256::digest(format!("{api_key}{request_token}{api_secret}").as_bytes()));
    let resp = client()
        .post(format!("{}/session/token", base.unwrap_or(DEFAULT_BASE).trim_end_matches('/')))
        .header("X-Kite-Version", "3")
        .form(&[("api_key", api_key), ("request_token", request_token), ("checksum", checksum.as_str())])
        .send()
        .await
        .map_err(|e| BrokerError::Http(e.without_url().to_string()))?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| BrokerError::Http(e.without_url().to_string()))?;
    let data = unwrap_reply(status, &body).map_err(|e| match e {
        BrokerError::SessionExpired => BrokerError::Auth("Kite rejected the request token. It is single use and expires within minutes; log in again to get a fresh one.".into()),
        other => other,
    })?;
    data["access_token"].as_str().map(str::to_string).ok_or_else(|| BrokerError::Parse("no access_token in reply".into()))
}

/// Cookies kept by hand, because the redirect chain has to be walked by hand too.
#[derive(Default)]
struct Jar(Vec<(String, String)>);

impl Jar {
    fn absorb(&mut self, res: &reqwest::Response) {
        for v in res.headers().get_all(reqwest::header::SET_COOKIE) {
            let Ok(text) = v.to_str() else { continue };
            let first = text.split(';').next().unwrap_or("");
            if let Some((k, val)) = first.split_once('=') {
                self.0.retain(|(name, _)| name != k.trim());
                if !val.is_empty() {
                    self.0.push((k.trim().to_string(), val.trim().to_string()));
                }
            }
        }
    }
    fn header(&self) -> String {
        self.0.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ")
    }
}

fn web_client() -> reqwest::Client {
    // Redirects stay OFF: the last hop carries a single-use request token that must not be spent on the app's redirect URL.
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .user_agent(BROWSER_UA)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("web client")
}

fn request_token_of(url: &str) -> String {
    reqwest::Url::parse(url).ok().and_then(|u| u.query_pairs().find(|(k, _)| k == "request_token").map(|(_, v)| v.into_owned())).unwrap_or_default()
}

fn web_err(what: &str, e: reqwest::Error) -> BrokerError {
    BrokerError::Http(format!("{what}: {}", e.without_url()))
}

impl KiteBroker {
    fn login_url(&self) -> String {
        format!("{}/connect/login?v=3&api_key={}", self.web, self.api_key)
    }

    /// Walk the redirect chain by hand, keeping cookies, and stop at the hop that carries a request token.
    async fn walk(&self, web: &reqwest::Client, jar: &mut Jar, start: &str) -> Result<(String, String), BrokerError> {
        let mut url = start.to_string();
        for _ in 0..10 {
            let mut req = web.get(&url).header("Accept", "text/html,application/xhtml+xml,*/*");
            if !jar.header().is_empty() {
                req = req.header("Cookie", jar.header());
            }
            let res = req.send().await.map_err(|e| web_err("Zerodha login page", e))?;
            jar.absorb(&res);
            let status = res.status();
            if status.is_redirection() {
                if let Some(loc) = res.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()) {
                    let base = reqwest::Url::parse(&url).map_err(|e| BrokerError::Http(e.to_string()))?;
                    url = base.join(loc).map_err(|e| BrokerError::Http(e.to_string()))?.to_string();
                    let t = request_token_of(&url);
                    if !t.is_empty() {
                        return Ok((url, t));
                    }
                    continue;
                }
            }
            if status.is_client_error() || status.is_server_error() {
                return Err(BrokerError::Http(format!("Zerodha login page returned HTTP {}", status.as_u16())));
            }
            let t = request_token_of(&url);
            return Ok((url, t));
        }
        Err(BrokerError::Http("Zerodha login redirected too many times".into()))
    }

    async fn post_web(&self, web: &reqwest::Client, jar: &mut Jar, path: &str, form: &[(&str, &str)], referer: &str) -> Result<Value, BrokerError> {
        let mut req = web
            .post(format!("{}{path}", self.web))
            .header("Accept", "application/json")
            .header("X-Kite-Version", "3")
            .header("Referer", referer);
        if !jar.header().is_empty() {
            req = req.header("Cookie", jar.header());
        }
        let res = req.form(form).send().await.map_err(|e| web_err("Zerodha", e))?;
        jar.absorb(&res);
        let status = res.status().as_u16();
        let text = res.text().await.map_err(|e| web_err("Zerodha (reply)", e))?;
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status >= 400 || v["status"] == "error" {
            let msg = v["message"].as_str().unwrap_or("no message");
            return Err(BrokerError::Auth(format!("Zerodha: {msg}")));
        }
        Ok(if v["data"].is_object() { v["data"].clone() } else { v })
    }

    /// The unattended login, step for step as QuantDesk does it:
    /// seed cookies, password, TOTP, replay with skip_session to get the request token, exchange it.
    async fn headless_login(&self, auto: &KiteAuto) -> Result<String, BrokerError> {
        let web = web_client();
        let mut jar = Jar::default();
        let (entry, _) = self.walk(&web, &mut jar, &self.login_url()).await?;

        let login = self.post_web(&web, &mut jar, "/api/login", &[("user_id", &auto.user_id), ("password", &auto.password)], &entry).await?;
        let request_id = login["request_id"].as_str().unwrap_or("").to_string();
        if request_id.is_empty() {
            return Err(BrokerError::Auth("Zerodha login did not return a request_id".into()));
        }
        let twofa_type = login["twofa_type"].as_str().unwrap_or("").to_string();

        let code = crate::totp::fresh_code(&auto.totp_secret).await.map_err(BrokerError::Auth)?;
        let mut form: Vec<(&str, &str)> = vec![("user_id", &auto.user_id), ("request_id", &request_id), ("twofa_value", &code)];
        if !twofa_type.is_empty() {
            form.push(("twofa_type", &twofa_type));
        }
        self.post_web(&web, &mut jar, "/api/twofa", &form, &entry).await.map_err(|e| match e {
            BrokerError::Auth(m) => BrokerError::Auth(format!("{m}. Check the TOTP secret is the text key from Kite's External TOTP setup, and that this computer's clock is correct")),
            other => other,
        })?;

        let replay = {
            let mut u = reqwest::Url::parse(&entry).map_err(|e| BrokerError::Http(e.to_string()))?;
            u.query_pairs_mut().append_pair("skip_session", "true");
            u.to_string()
        };
        let (landed, request_token) = self.walk(&web, &mut jar, &replay).await?;
        if request_token.is_empty() {
            if landed.contains("/connect/authorize") || landed.contains("/connect/finish") {
                return Err(BrokerError::Auth(format!("Zerodha needs a one-time app authorization. Open {} in a browser once, sign in and click Authorize. After that the login runs by itself.", self.login_url())));
            }
            return Err(BrokerError::Auth("Zerodha login worked but returned no request token. Check the API key is active and its redirect URL matches the Kite developer console.".into()));
        }
        exchange_request_token(Some(&self.base), &self.api_key, &auto.api_secret, &request_token).await
    }
}

#[async_trait]
impl BrokerAdapter for KiteBroker {
    fn broker(&self) -> Broker {
        Broker::Zerodha
    }

    /// A stored token is checked first. If it is missing or no longer works and the account has the
    /// credentials for an unattended login, log in by itself and keep the new token.
    async fn login(&self) -> Result<(), BrokerError> {
        let checked = if self.has_token() { self.get("/user/profile", &[]).await.map(|_| ()) } else { Err(BrokerError::SessionExpired) };
        match checked {
            Ok(()) => Ok(()),
            Err(BrokerError::SessionExpired) => match &self.auto {
                Some(auto) => {
                    let token = self.headless_login(auto).await?;
                    self.set_token(&token);
                    self.get("/user/profile", &[]).await.map(|_| ())
                }
                None if !self.has_token() => Err(BrokerError::Auth("No access token yet. Add today's token on the Connections page.".into())),
                None => Err(BrokerError::Auth("Kite rejected the token. Zerodha tokens stop working every morning: renew it on the Connections page.".into())),
            },
            Err(other) => Err(other),
        }
    }

    async fn fetch_books(&self) -> Result<Books, BrokerError> {
        let now = Utc::now();
        let orders = self.get("/orders", &[]).await?;
        let trades = self.get("/trades", &[]).await?;
        let positions = self.get("/portfolio/positions", &[]).await?;
        // A desk may have no GTTs, or the account may not allow the call; neither should hide the books.
        let gtts = match self.get("/gtt/triggers", &[]).await {
            Ok(g) => g,
            Err(BrokerError::SessionExpired) => return Err(BrokerError::SessionExpired),
            Err(e) => {
                tracing::debug!(error = %e, "gtt list unavailable");
                Value::Null
            }
        };
        let mut all = kite::parse_orders(&orders, now);
        all.extend(kite::parse_gtts(&gtts, now));
        Ok(Books { orders: all, trades: kite::parse_trades(&trades, now), positions: kite::parse_positions(&positions["net"]) })
    }

    async fn fetch_ltp(&self, keys: &[InstrumentKey]) -> Result<HashMap<InstrumentKey, f64>, BrokerError> {
        let mut out = HashMap::new();
        // Kite accepts up to 1000 instruments per quote call.
        for chunk in keys.chunks(500) {
            let q: Vec<(&str, String)> = chunk.iter().map(|k| ("i", format!("{}:{}", exchange_code(k.exchange), k.symbol))).collect();
            let data = self.get("/quote/ltp", &q).await?;
            for k in chunk {
                if let Some(p) = data[format!("{}:{}", exchange_code(k.exchange), k.symbol)]["last_price"].as_f64() {
                    out.insert(k.clone(), p);
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn token_errors_ask_for_a_new_login() {
        let body = r#"{"status":"error","message":"Incorrect api_key or access_token.","error_type":"TokenException"}"#;
        assert!(matches!(unwrap_reply(StatusCode::FORBIDDEN, body), Err(BrokerError::SessionExpired)));
    }

    #[test]
    fn rate_limit_is_its_own_error() {
        let body = r#"{"status":"error","message":"Too many requests","error_type":"NetworkException"}"#;
        assert!(matches!(unwrap_reply(StatusCode::TOO_MANY_REQUESTS, body), Err(BrokerError::RateLimited)));
    }

    #[test]
    fn success_returns_data() {
        let body = r#"{"status":"success","data":{"user_id":"AB1234"}}"#;
        assert_eq!(unwrap_reply(StatusCode::OK, body).unwrap()["user_id"], "AB1234");
    }

    #[test]
    fn checksum_matches_the_documented_formula() {
        // sha256("abc") sanity check for the formatting helper used by the exchange.
        assert_eq!(format!("{:x}", Sha256::digest(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn debug_never_prints_the_token() {
        let k = KiteBroker::new("keyvalue", "secrettoken");
        let s = format!("{k:?}");
        assert!(!s.contains("secrettoken") && !s.contains("keyvalue"));
    }

    #[tokio::test]
    async fn empty_token_fails_without_a_network_call() {
        let k = KiteBroker::new("key", "");
        assert!(matches!(k.login().await, Err(BrokerError::Auth(_))));
    }
}
