//! Angel One and Kotak Neo: automatic sign-in and reading the books, against local stand-in servers.

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use sl_brokers::angel_live::AngelBroker;
use sl_brokers::kotak_live::KotakBroker;
use sl_brokers::{BrokerAdapter, BrokerError, InstrumentKey};
use sl_core::Exchange;
use std::sync::{Arc, Mutex};

const SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

fn totp_ok(code: &str) -> bool {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    [now, now.saturating_sub(30), now + 30].iter().any(|t| sl_brokers::totp::code_at(SECRET, *t).as_deref() == Some(code))
}

async fn serve(app: Router) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    format!("http://{addr}")
}

fn h(headers: &HeaderMap, k: &str) -> String {
    headers.get(k).and_then(|v| v.to_str().ok()).unwrap_or("").to_string()
}

// ---------------- Angel One ----------------

#[derive(Clone)]
struct AngelState {
    jwt_valid: Arc<Mutex<bool>>,
}

async fn angel_login(headers: HeaderMap, Json(b): Json<Value>) -> Json<Value> {
    let ok = h(&headers, "x-privatekey") == "shortkey"
        && b["clientcode"] == "A123"
        && b["password"] == "4321"
        && totp_ok(b["totp"].as_str().unwrap_or(""));
    if ok {
        Json(json!({"status": true, "message": "SUCCESS", "data": {"jwtToken": "JWT1", "refreshToken": "R", "feedToken": "F"}}))
    } else {
        Json(json!({"status": false, "message": "Invalid totp", "errorcode": "AB1050", "data": null}))
    }
}

fn angel_gate(s: &AngelState, headers: &HeaderMap) -> Option<Json<Value>> {
    if h(headers, "authorization") != "Bearer JWT1" || !*s.jwt_valid.lock().unwrap() {
        return Some(Json(json!({"status": false, "message": "Invalid Token", "errorcode": "AG8002", "data": null})));
    }
    None
}

async fn angel_orders(State(s): State<AngelState>, headers: HeaderMap) -> Json<Value> {
    if let Some(e) = angel_gate(&s, &headers) {
        return e;
    }
    Json(json!({"status": true, "data": [{
        "orderid": "250101000000001", "exchange": "NFO", "tradingsymbol": "NIFTY28NOV24000CE", "transactiontype": "BUY",
        "ordertype": "STOPLOSS_LIMIT", "variety": "STOPLOSS", "producttype": "CARRYFORWARD", "quantity": "75", "filledshares": "0",
        "price": "130.00", "triggerprice": "130.00", "orderstatus": "trigger pending", "status": "trigger pending",
        "updatetime": "20-Oct-2025 13:10:59", "text": ""
    }]}))
}

async fn angel_trades(State(s): State<AngelState>, headers: HeaderMap) -> Json<Value> {
    if let Some(e) = angel_gate(&s, &headers) {
        return e;
    }
    Json(json!({"status": true, "data": null}))
}

async fn angel_positions(State(s): State<AngelState>, headers: HeaderMap) -> Json<Value> {
    if let Some(e) = angel_gate(&s, &headers) {
        return e;
    }
    Json(json!({"status": true, "data": [{
        "exchange": "NFO", "tradingsymbol": "NIFTY28NOV24000CE", "symboltoken": "12345", "producttype": "CARRYFORWARD",
        "netqty": "-75", "totalsellavgprice": "100.00", "cfbuyqty": "0", "cfsellqty": "75"
    }]}))
}

async fn angel_quote(State(s): State<AngelState>, headers: HeaderMap, Json(b): Json<Value>) -> Json<Value> {
    if let Some(e) = angel_gate(&s, &headers) {
        return e;
    }
    assert_eq!(b["mode"], "LTP");
    assert_eq!(b["exchangeTokens"]["NFO"][0], "12345");
    Json(json!({"status": true, "data": {"fetched": [{"exchange": "NFO", "tradingSymbol": "NIFTY28NOV24000CE", "symbolToken": "12345", "ltp": 91.3}]}}))
}

async fn angel_stand_in() -> (String, AngelState) {
    let st = AngelState { jwt_valid: Arc::new(Mutex::new(true)) };
    let url = serve(
        Router::new()
            .route("/rest/auth/angelbroking/user/v1/loginByPassword", post(angel_login))
            .route("/rest/secure/angelbroking/order/v1/getOrderBook", get(angel_orders))
            .route("/rest/secure/angelbroking/order/v1/getTradeBook", get(angel_trades))
            .route("/rest/secure/angelbroking/order/v1/getPosition", get(angel_positions))
            .route("/rest/secure/angelbroking/market/v1/quote/", post(angel_quote))
            .with_state(st.clone()),
    )
    .await;
    (url, st)
}

#[tokio::test]
async fn angel_signs_in_by_itself_and_reads_the_books_and_prices() {
    let (url, _st) = angel_stand_in().await;
    let a = AngelBroker::with_base(&url, "shortkey", "A123", "4321", SECRET);
    a.login().await.expect("login");
    let books = a.fetch_books().await.expect("books");
    assert_eq!(books.positions.len(), 1);
    assert_eq!(books.positions[0].net_qty, -75);
    assert_eq!(books.orders.len(), 1);
    assert_eq!(books.orders[0].trigger_price, Some(130.0));
    assert!(books.trades.is_empty());
    let key = InstrumentKey { exchange: Exchange::Nfo, symbol: "NIFTY28NOV24000CE".into() };
    let ltp = a.fetch_ltp(&[key.clone()]).await.expect("ltp");
    assert_eq!(ltp.get(&key), Some(&91.3));
}

#[tokio::test]
async fn angel_wrong_totp_is_an_auth_error() {
    let (url, _st) = angel_stand_in().await;
    let a = AngelBroker::with_base(&url, "shortkey", "A123", "4321", "MFRGGZDFMZTWQ2LKNNWG23TPOBYXE43U");
    assert!(matches!(a.login().await, Err(BrokerError::Auth(_)) | Err(BrokerError::Http(_))));
}

#[tokio::test]
async fn angel_an_ended_session_asks_for_a_new_login() {
    let (url, st) = angel_stand_in().await;
    let a = AngelBroker::with_base(&url, "shortkey", "A123", "4321", SECRET);
    a.login().await.unwrap();
    *st.jwt_valid.lock().unwrap() = false; // midnight passes
    let e = a.fetch_books().await.unwrap_err();
    assert!(e.needs_relogin(), "{e:?}");
    *st.jwt_valid.lock().unwrap() = true;
    a.login().await.unwrap();
    assert!(a.fetch_books().await.is_ok());
}

#[tokio::test]
async fn angel_refuses_the_secret_key_before_calling_anyone() {
    let a = AngelBroker::with_base("http://127.0.0.1:9", "1b4c3f1e-9d5a-4a3e-8c11-0a1b2c3d4e5f", "A123", "4321", SECRET);
    assert!(matches!(a.login().await, Err(BrokerError::Auth(m)) if m.contains("secret key")));
}

// ---------------- Kotak Neo ----------------

#[derive(Clone)]
struct KotakState {
    own_url: Arc<Mutex<String>>,
}

async fn kotak_login(headers: HeaderMap, Json(b): Json<Value>) -> Json<Value> {
    if h(&headers, "neo-fin-key") != "neotradeapi" || h(&headers, "authorization") != "consumer-token" {
        return Json(json!({"status": "error", "message": "Missing required field 'NeoFinKey'"}));
    }
    if b["ucc"] == "U123" && b["mobileNumber"] == "+919999999999" && totp_ok(b["totp"].as_str().unwrap_or("")) {
        Json(json!({"data": {"token": "VT", "sid": "VS", "kType": "View"}}))
    } else {
        Json(json!({"status": "error", "message": "Invalid TOTP"}))
    }
}

async fn kotak_validate(State(s): State<KotakState>, headers: HeaderMap, Json(b): Json<Value>) -> Json<Value> {
    if h(&headers, "sid") == "VS" && h(&headers, "auth") == "VT" && b["mpin"] == "246810" {
        let base = s.own_url.lock().unwrap().clone();
        Json(json!({"data": {"token": "TT", "sid": "TS", "baseUrl": base, "kType": "Trade"}}))
    } else {
        Json(json!({"status": "error", "message": "Invalid MPIN"}))
    }
}

fn kotak_gate(headers: &HeaderMap) -> Option<Json<Value>> {
    if h(headers, "sid") != "TS" || h(headers, "auth") != "TT" || h(headers, "neo-fin-key") != "neotradeapi" {
        return Some(Json(json!({"stat": "Not_Ok", "emsg": "Invalid Sid, login again"})));
    }
    None
}

async fn kotak_orders(headers: HeaderMap) -> Json<Value> {
    if let Some(e) = kotak_gate(&headers) {
        return e;
    }
    Json(json!({"stat": "Ok", "data": [{
        "nOrdNo": "2501010000001", "exSeg": "nse_fo", "trdSym": "NIFTY28NOV24000CE", "trnsTp": "B", "prcTp": "SL", "prod": "NRML",
        "qty": "75", "fldQty": "0", "prc": "130.00", "trgPrc": "130.00", "ordSt": "trigger pending", "ordDtTm": "20-Oct-2025 13:10:59"
    }]}))
}

async fn kotak_trades(headers: HeaderMap) -> Json<Value> {
    if let Some(e) = kotak_gate(&headers) {
        return e;
    }
    Json(json!({"stat": "Not_Ok", "stCode": 5203, "errMsg": "No Data"}))
}

async fn kotak_positions(headers: HeaderMap) -> Json<Value> {
    if let Some(e) = kotak_gate(&headers) {
        return e;
    }
    Json(json!({"stat": "Ok", "data": [{
        "exSeg": "nse_fo", "trdSym": "NIFTY28NOV24000CE", "tok": "55555", "prod": "NRML",
        "cfBuyQty": "0", "flBuyQty": "0", "cfSellQty": "0", "flSellQty": "75", "cfBuyAmt": "0", "buyAmt": "0", "cfSellAmt": "0", "sellAmt": "7500.00"
    }]}))
}

async fn kotak_ltp(headers: HeaderMap, Path(list): Path<String>) -> Response {
    assert_eq!(h(&headers, "authorization"), "consumer-token");
    assert_eq!(list, "nse_fo|55555");
    ([(header::CONTENT_TYPE, "application/json")], json!([{"exchange_token": "55555", "exchange": "nse_fo", "ltp": "91.3"}]).to_string()).into_response()
}

async fn kotak_stand_in() -> String {
    let st = KotakState { own_url: Arc::new(Mutex::new(String::new())) };
    let url = serve(
        Router::new()
            .route("/login/1.0/tradeApiLogin", post(kotak_login))
            .route("/login/1.0/tradeApiValidate", post(kotak_validate))
            .route("/quick/user/orders", get(kotak_orders))
            .route("/quick/user/trades", get(kotak_trades))
            .route("/quick/user/positions", get(kotak_positions))
            .route("/script-details/1.0/quotes/neosymbol/:list/ltp", get(kotak_ltp))
            .with_state(st.clone()),
    )
    .await;
    *st.own_url.lock().unwrap() = url.clone();
    url
}

#[tokio::test]
async fn kotak_signs_in_in_two_steps_and_reads_the_books_and_prices() {
    let url = kotak_stand_in().await;
    let k = KotakBroker::with_login_base(&url, "consumer-token", "+919999999999", "U123", "246810", SECRET);
    k.login().await.expect("login");
    let books = k.fetch_books().await.expect("books");
    assert_eq!(books.positions.len(), 1);
    assert_eq!(books.positions[0].net_qty, -75);
    assert_eq!(books.orders.len(), 1);
    assert_eq!(books.orders[0].trigger_price, Some(130.0));
    assert!(books.trades.is_empty(), "No Data is an empty trade book, not an error");
    let key = InstrumentKey { exchange: Exchange::Nfo, symbol: "NIFTY28NOV24000CE".into() };
    let ltp = k.fetch_ltp(&[key.clone()]).await.expect("ltp");
    assert_eq!(ltp.get(&key), Some(&91.3));
}

#[tokio::test]
async fn kotak_wrong_mpin_and_wrong_totp_are_auth_errors() {
    let url = kotak_stand_in().await;
    let bad_pin = KotakBroker::with_login_base(&url, "consumer-token", "+919999999999", "U123", "000000", SECRET);
    assert!(matches!(bad_pin.login().await, Err(BrokerError::Auth(m)) if m.contains("MPIN")));
    let bad_totp = KotakBroker::with_login_base(&url, "consumer-token", "+919999999999", "U123", "246810", "MFRGGZDFMZTWQ2LKNNWG23TPOBYXE43U");
    assert!(matches!(bad_totp.login().await, Err(BrokerError::Auth(m)) if m.contains("TOTP")));
}

#[tokio::test]
async fn kotak_reading_before_login_asks_for_login() {
    let url = kotak_stand_in().await;
    let k = KotakBroker::with_login_base(&url, "consumer-token", "+919999999999", "U123", "246810", SECRET);
    assert!(k.fetch_books().await.unwrap_err().needs_relogin());
}
