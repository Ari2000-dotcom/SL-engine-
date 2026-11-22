//! Canonical, broker-independent models. Every broker adapter normalizes into these.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Broker {
    Kotak,
    Zerodha,
    Angel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Exchange {
    Nse,
    Bse,
    Nfo,
    Bfo,
    Mcx,
    Cds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Segment {
    Eq,
    Fut,
    Opt,
    Cur,
    Com,
}

impl Segment {
    /// Best-effort segment from exchange and trading symbol (options end in CE/PE).
    pub fn infer(exchange: Exchange, symbol: &str) -> Segment {
        match exchange {
            Exchange::Nse | Exchange::Bse => Segment::Eq,
            Exchange::Nfo | Exchange::Bfo => {
                if symbol.ends_with("CE") || symbol.ends_with("PE") {
                    Segment::Opt
                } else {
                    Segment::Fut
                }
            }
            Exchange::Mcx => Segment::Com,
            Exchange::Cds => Segment::Cur,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn opposite(self) -> Side {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Product {
    Intraday,
    Carryforward,
    Delivery,
    Mtf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderKind {
    Market,
    Limit,
    SlLimit,
    SlMarket,
    /// Synthetic: built from an active broker GTT rule.
    GttSl,
}

impl OrderKind {
    pub fn is_stop(self) -> bool {
        matches!(self, OrderKind::SlLimit | OrderKind::SlMarket | OrderKind::GttSl)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderState {
    PendingNew,
    Open,
    TriggerPending,
    ModifyPending,
    CancelPending,
    Partial,
    Complete,
    Cancelled,
    Rejected,
    Amo,
    Unknown,
}

impl OrderState {
    /// States in which a stop order really protects the position.
    pub fn is_live_protection(self) -> bool {
        matches!(
            self,
            OrderState::PendingNew
                | OrderState::Open
                | OrderState::TriggerPending
                | OrderState::ModifyPending
                | OrderState::Partial
        )
    }

    /// Map raw broker status text (any broker) to a canonical state. Fuzzy and lowercase.
    pub fn from_raw(raw: &str) -> OrderState {
        let s = raw.trim().to_ascii_lowercase();
        if s.is_empty() {
            return OrderState::Unknown;
        }
        if s.contains("reject") {
            OrderState::Rejected
        } else if s.contains("cancel pending") {
            OrderState::CancelPending
        } else if s.contains("cancel") {
            OrderState::Cancelled
        } else if s.contains("after market") || s.contains("amo req") || s == "amo" {
            OrderState::Amo
        } else if s.contains("modify") && s.contains("pending") {
            OrderState::ModifyPending
        } else if s == "modified" {
            // The order was modified and is still live at the exchange.
            OrderState::Open
        } else if s.contains("trigger pending") {
            OrderState::TriggerPending
        } else if s.contains("validation pending")
            || s.contains("put order req")
            || s.contains("open pending")
        {
            OrderState::PendingNew
        } else if s.contains("partial") {
            OrderState::Partial
        } else if s == "open" {
            OrderState::Open
        } else if s.contains("complete") || s == "filled" || s == "traded" {
            OrderState::Complete
        } else {
            OrderState::Unknown
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub broker_order_id: String,
    pub exchange: Exchange,
    pub symbol: String,
    pub side: Side,
    pub kind: OrderKind,
    pub product: Product,
    pub qty: i64,
    pub filled_qty: i64,
    pub pending_qty: i64,
    pub price: f64,
    pub trigger_price: Option<f64>,
    pub state: OrderState,
    pub status_raw: String,
    pub reject_reason: Option<String>,
    pub order_ts: DateTime<Utc>,
    pub updated_ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub trade_id: String,
    pub broker_order_id: String,
    pub exchange: Exchange,
    pub symbol: String,
    pub side: Side,
    pub product: Product,
    pub qty: i64,
    pub price: f64,
    pub fill_ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub exchange: Exchange,
    pub segment: Segment,
    pub symbol: String,
    pub product: Product,
    /// Signed: positive long, negative short.
    pub net_qty: i64,
    pub avg_price: f64,
    /// Quantity carried from earlier days (signed).
    pub overnight_qty: i64,
    pub ltp: Option<f64>,
    pub multiplier: f64,
}

/// Everything known about one broker account at one moment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub account_id: String,
    pub client_id: String,
    pub broker: Broker,
    pub orders: Vec<Order>,
    pub trades: Vec<Trade>,
    pub positions: Vec<Position>,
    pub fetched_at: DateTime<Utc>,
    /// false when the login failed or the last poll errored.
    pub healthy: bool,
}
