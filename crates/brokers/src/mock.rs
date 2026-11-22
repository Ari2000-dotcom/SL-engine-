//! A scripted broker for tests only. The server no longer uses it. Each account gets a fixed set of legs; two of them change
//! over time so a running dashboard shows incidents opening and resolving.

use crate::adapter::*;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use sl_core::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    /// Everything protected.
    Clean,
    /// Short straddle where the put leg never gets a stop.
    OneLegMissing,
    /// Add-on left the stop covering half the position.
    PartialAfterAddOn,
    /// Stop was rejected by the broker.
    RejectedSl,
    /// Stop is protected, then gets cancelled after 20 polls.
    CancelledLater,
    /// No stop at first, the client places it after 12 polls.
    FixedLater,
    /// Trigger sits on the wrong side of the market.
    WrongSide,
    /// Stop is far beyond the loss limit.
    TooFar,
}

pub struct MockBroker {
    broker: Broker,
    script: Script,
    ticks: AtomicU64,
    login_fails: bool,
}

impl MockBroker {
    pub fn new(broker: Broker, script: Script) -> Self {
        MockBroker { broker, script, ticks: AtomicU64::new(0), login_fails: false }
    }

    pub fn failing_login(mut self) -> Self {
        self.login_fails = true;
        self
    }
}

const CE: &str = "NIFTY26NOV24000CE";
const PE: &str = "NIFTY26NOV24000PE";

fn short_leg(symbol: &str, qty: i64, avg: f64, ltp: f64) -> Position {
    Position {
        exchange: Exchange::Nfo,
        segment: Segment::Opt,
        symbol: symbol.into(),
        product: Product::Carryforward,
        net_qty: -qty,
        avg_price: avg,
        overnight_qty: 0,
        ltp: Some(ltp),
        multiplier: 1.0,
    }
}

fn stop(id: &str, symbol: &str, qty: i64, trigger: f64, state: OrderState, now: DateTime<Utc>) -> Order {
    Order {
        broker_order_id: id.into(),
        exchange: Exchange::Nfo,
        symbol: symbol.into(),
        side: Side::Buy,
        kind: OrderKind::SlMarket,
        product: Product::Carryforward,
        qty,
        filled_qty: 0,
        pending_qty: qty,
        price: 0.0,
        trigger_price: Some(trigger),
        state,
        status_raw: format!("{state:?}"),
        reject_reason: (state == OrderState::Rejected).then(|| "RMS: Insufficient margin".to_string()),
        order_ts: now - Duration::seconds(70),
        updated_ts: now - Duration::seconds(60),
    }
}

fn entry(symbol: &str, qty: i64, secs_ago: i64, now: DateTime<Utc>, id: u32) -> Trade {
    Trade {
        trade_id: format!("T{id}"),
        broker_order_id: format!("E{id}"),
        exchange: Exchange::Nfo,
        symbol: symbol.into(),
        side: Side::Sell,
        product: Product::Carryforward,
        qty,
        price: 100.0,
        fill_ts: now - Duration::seconds(secs_ago),
    }
}

impl MockBroker {
    pub fn books_at(&self, tick: u64, now: DateTime<Utc>) -> Books {
        // A little price movement so the LTP column is alive.
        let wiggle = ((tick % 8) as f64 - 4.0) * 0.35;
        let (ce_ltp, pe_ltp) = (92.0 + wiggle, 88.0 - wiggle);
        let ts = |n| format!("{}{}", self.script as u8, n);
        let (positions, orders, trades);
        match self.script {
            Script::Clean => {
                positions = vec![short_leg(CE, 75, 100.0, ce_ltp), short_leg(PE, 75, 100.0, pe_ltp)];
                orders = vec![
                    stop(&ts(1), CE, 75, 130.0, OrderState::TriggerPending, now),
                    stop(&ts(2), PE, 75, 130.0, OrderState::TriggerPending, now),
                ];
                trades = vec![entry(CE, 75, 1800, now, 1), entry(PE, 75, 1800, now, 2)];
            }
            Script::OneLegMissing => {
                positions = vec![short_leg(CE, 75, 100.0, ce_ltp), short_leg(PE, 75, 100.0, pe_ltp)];
                orders = vec![stop(&ts(1), CE, 75, 130.0, OrderState::TriggerPending, now)];
                trades = vec![entry(CE, 75, 1500, now, 1), entry(PE, 75, 1500, now, 2)];
            }
            Script::PartialAfterAddOn => {
                positions = vec![short_leg(CE, 150, 100.0, ce_ltp)];
                orders = vec![stop(&ts(1), CE, 75, 130.0, OrderState::TriggerPending, now)];
                trades = vec![entry(CE, 75, 2400, now, 1), entry(CE, 75, 600, now, 2)];
            }
            Script::RejectedSl => {
                positions = vec![short_leg(PE, 75, 100.0, pe_ltp)];
                orders = vec![stop(&ts(1), PE, 75, 130.0, OrderState::Rejected, now)];
                trades = vec![entry(PE, 75, 900, now, 1)];
            }
            Script::CancelledLater => {
                positions = vec![short_leg(CE, 75, 100.0, ce_ltp)];
                let state = if tick < 20 { OrderState::TriggerPending } else { OrderState::Cancelled };
                orders = vec![stop(&ts(1), CE, 75, 130.0, state, now)];
                trades = vec![entry(CE, 75, 3000, now, 1)];
            }
            Script::FixedLater => {
                positions = vec![short_leg(PE, 75, 100.0, pe_ltp)];
                orders = if tick >= 12 {
                    vec![stop(&ts(1), PE, 75, 130.0, OrderState::TriggerPending, now)]
                } else {
                    vec![]
                };
                trades = vec![entry(PE, 75, 400, now, 1)];
            }
            Script::WrongSide => {
                positions = vec![short_leg(CE, 75, 100.0, ce_ltp)];
                orders = vec![stop(&ts(1), CE, 75, 80.0, OrderState::TriggerPending, now)];
                trades = vec![entry(CE, 75, 1200, now, 1)];
            }
            Script::TooFar => {
                positions = vec![short_leg(PE, 75, 100.0, pe_ltp)];
                orders = vec![stop(&ts(1), PE, 75, 190.0, OrderState::TriggerPending, now)];
                trades = vec![entry(PE, 75, 1200, now, 1)];
            }
        }
        Books { orders, trades, positions }
    }
}

#[async_trait]
impl BrokerAdapter for MockBroker {
    fn broker(&self) -> Broker {
        self.broker
    }

    async fn login(&self) -> Result<(), BrokerError> {
        if self.login_fails {
            Err(BrokerError::Auth("mock: invalid TOTP".into()))
        } else {
            Ok(())
        }
    }

    async fn fetch_books(&self) -> Result<Books, BrokerError> {
        if self.login_fails {
            return Err(BrokerError::SessionExpired);
        }
        let tick = self.ticks.fetch_add(1, Ordering::SeqCst);
        Ok(self.books_at(tick, Utc::now()))
    }

    async fn fetch_ltp(&self, keys: &[InstrumentKey]) -> Result<HashMap<InstrumentKey, f64>, BrokerError> {
        Ok(keys.iter().map(|k| (k.clone(), 90.0)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(script: Script, tick: u64) -> Vec<LegEval> {
        let now = Utc::now();
        let b = MockBroker::new(Broker::Zerodha, script);
        let books = b.books_at(tick, now);
        let snap = Snapshot {
            account_id: "A".into(),
            client_id: "C".into(),
            broker: Broker::Zerodha,
            orders: books.orders,
            trades: books.trades,
            positions: books.positions,
            fetched_at: now,
            healthy: true,
        };
        evaluate(&snap, &RiskBook::default(), now)
    }

    fn statuses(script: Script, tick: u64) -> Vec<LegStatus> {
        run(script, tick).into_iter().map(|l| l.status).collect()
    }

    #[test]
    fn every_script_produces_its_intended_status() {
        assert_eq!(statuses(Script::Clean, 0), vec![LegStatus::Protected, LegStatus::Protected]);
        assert_eq!(statuses(Script::OneLegMissing, 0), vec![LegStatus::Protected, LegStatus::NoSl]);
        assert_eq!(statuses(Script::PartialAfterAddOn, 0), vec![LegStatus::PartialSl]);
        assert_eq!(statuses(Script::RejectedSl, 0), vec![LegStatus::SlRejected]);
        assert_eq!(statuses(Script::WrongSide, 0), vec![LegStatus::WrongSide]);
        assert_eq!(statuses(Script::TooFar, 0), vec![LegStatus::TooFar]);
    }

    #[test]
    fn scripts_change_over_time() {
        assert_eq!(statuses(Script::FixedLater, 0), vec![LegStatus::NoSl]);
        assert_eq!(statuses(Script::FixedLater, 12), vec![LegStatus::Protected]);
        assert_eq!(statuses(Script::CancelledLater, 0), vec![LegStatus::Protected]);
        assert_eq!(statuses(Script::CancelledLater, 20), vec![LegStatus::SlCancelled]);
    }

    #[tokio::test]
    async fn failing_login_reports_needs_relogin() {
        let b = MockBroker::new(Broker::Angel, Script::Clean).failing_login();
        assert!(b.login().await.unwrap_err().needs_relogin());
        assert!(b.fetch_books().await.unwrap_err().needs_relogin());
    }
}
