//! The one interface every broker implements. It is read-only by construction: there is no
//! method that places, modifies or cancels an order.

use async_trait::async_trait;
use sl_core::*;
use std::collections::HashMap;

#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    #[error("login failed: {0}")]
    Auth(String),
    #[error("session expired")]
    SessionExpired,
    #[error("rate limited")]
    RateLimited,
    #[error("http error: {0}")]
    Http(String),
    #[error("unexpected response: {0}")]
    Parse(String),
}

impl BrokerError {
    /// Whether a fresh login could fix it.
    pub fn needs_relogin(&self) -> bool {
        matches!(self, BrokerError::SessionExpired | BrokerError::Auth(_))
    }
}

#[derive(Debug, Clone, Default)]
pub struct Books {
    pub orders: Vec<Order>,
    pub trades: Vec<Trade>,
    pub positions: Vec<Position>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstrumentKey {
    pub exchange: Exchange,
    pub symbol: String,
}

#[async_trait]
pub trait BrokerAdapter: Send + Sync {
    fn broker(&self) -> Broker;
    async fn login(&self) -> Result<(), BrokerError>;
    /// Orders (including GTT stops), trades and positions, already normalized.
    async fn fetch_books(&self) -> Result<Books, BrokerError>;
    async fn fetch_ltp(&self, keys: &[InstrumentKey]) -> Result<HashMap<InstrumentKey, f64>, BrokerError>;
}
