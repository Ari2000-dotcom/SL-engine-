//! The rule engine: a pure function from (snapshot, risk settings, time) to one status per open leg.
//! No I/O and no clock reads, so every status can be unit-tested.

use crate::model::*;
use crate::risk::{RiskBook, RiskMode};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// A snapshot older than this is treated as stale and never shown as protected.
pub const STALE_AFTER_SECS: i64 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LegStatus {
    Grace,
    Protected,
    NoSl,
    SlRejected,
    SlCancelled,
    PartialSl,
    OverCovered,
    WrongSide,
    TooFar,
    /// A stop exists but a check could not run (no live price, unusable average price).
    Unverified,
    StaleData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Severity {
    Red,
    Amber,
    Green,
    Grey,
}

impl LegStatus {
    pub fn severity(self) -> Severity {
        match self {
            LegStatus::NoSl | LegStatus::SlRejected | LegStatus::SlCancelled | LegStatus::WrongSide => {
                Severity::Red
            }
            LegStatus::PartialSl | LegStatus::OverCovered | LegStatus::TooFar | LegStatus::Unverified => {
                Severity::Amber
            }
            LegStatus::Protected => Severity::Green,
            LegStatus::Grace | LegStatus::StaleData => Severity::Grey,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegEval {
    pub leg_key: String,
    pub account_id: String,
    pub client_id: String,
    pub broker: Broker,
    pub exchange: Exchange,
    pub segment: Segment,
    pub symbol: String,
    pub product: Product,
    pub net_qty: i64,
    pub avg_price: f64,
    pub ltp: Option<f64>,
    pub sl_order_ids: Vec<String>,
    pub sl_qty: i64,
    /// The trigger that would fire first.
    pub sl_trigger: Option<f64>,
    pub risk_mode: RiskMode,
    pub limit_value: f64,
    /// Loss at the worst trigger as % of average price.
    pub dist_pct: Option<f64>,
    /// Loss at the worst trigger in rupees.
    pub risk_rupee: Option<f64>,
    pub status: LegStatus,
    pub severity: Severity,
    pub detail: Option<String>,
    /// When the current position was opened today (flat -> non-flat). None if carried overnight.
    pub opened_ts: Option<DateTime<Utc>>,
    pub evaluated_at: DateTime<Utc>,
}

pub fn leg_key(account_id: &str, exchange: Exchange, symbol: &str, product: Product) -> String {
    format!("{account_id}|{exchange:?}|{symbol}|{product:?}")
}

pub fn evaluate(snap: &Snapshot, risk: &RiskBook, now: DateTime<Utc>) -> Vec<LegEval> {
    let stale = !snap.healthy || now - snap.fetched_at > Duration::seconds(STALE_AFTER_SECS);
    snap.positions
        .iter()
        .filter(|p| p.net_qty != 0)
        .map(|p| evaluate_leg(snap, p, risk, now, stale))
        .collect()
}

fn same_leg(exchange: Exchange, symbol: &str, product: Product, p: &Position) -> bool {
    exchange == p.exchange && symbol == p.symbol && product == p.product
}

/// When the current position was opened: the fill that took it from flat (or across zero).
/// None when the position was carried overnight and never crossed zero today, so no grace applies.
fn position_open_ts(snap: &Snapshot, p: &Position) -> Option<DateTime<Utc>> {
    let mut fills: Vec<&Trade> = snap
        .trades
        .iter()
        .filter(|t| same_leg(t.exchange, &t.symbol, t.product, p))
        .collect();
    fills.sort_by_key(|t| t.fill_ts);
    let mut running = p.overnight_qty;
    let mut opened = None;
    for t in fills {
        let before = running;
        running += if t.side == Side::Buy { t.qty } else { -t.qty };
        if before == 0 || (before > 0) != (running > 0) && running != 0 {
            opened = Some(t.fill_ts);
        }
    }
    opened
}

fn evaluate_leg(snap: &Snapshot, p: &Position, risk: &RiskBook, now: DateTime<Utc>, stale: bool) -> LegEval {
    let long = p.net_qty > 0;
    let entry_side = if long { Side::Buy } else { Side::Sell };
    let exit_side = entry_side.opposite();
    let abs_qty = p.net_qty.saturating_abs();
    let setting = risk.resolve(&snap.client_id, p.segment);
    let grace_sec = setting.grace_sec.clamp(0, 3600);
    let multiplier = if p.multiplier.is_finite() && p.multiplier != 0.0 { p.multiplier.abs() } else { 1.0 };

    let opened_ts = position_open_ts(snap, p);
    let in_grace = opened_ts.map(|ts| now - ts < Duration::seconds(grace_sec)).unwrap_or(false);

    let stops_on_leg = || {
        snap.orders.iter().filter(|o| {
            same_leg(o.exchange, &o.symbol, o.product, p) && o.side == exit_side && o.kind.is_stop()
        })
    };
    let protecting: Vec<&Order> = stops_on_leg().filter(|o| o.state.is_live_protection()).collect();

    let sl_qty: i64 = protecting.iter().map(|o| o.pending_qty).sum();
    let triggers: Vec<f64> = protecting.iter().filter_map(|o| o.trigger_price).collect();
    // The trigger that fires first is the one closest to the market, on the exit side.
    let sl_trigger = if long {
        triggers.iter().cloned().reduce(f64::max)
    } else {
        triggers.iter().cloned().reduce(f64::min)
    };
    // Loss distance per unit: positive means the stop sits on the loss side of the entry.
    let loss_per_unit = |trigger: f64| if long { p.avg_price - trigger } else { trigger - p.avg_price };
    let worst_loss = triggers.iter().cloned().map(loss_per_unit).reduce(f64::max);
    let avg_ok = p.avg_price.is_finite() && p.avg_price > 0.0;
    let dist_pct = worst_loss.filter(|_| avg_ok).map(|l| l / p.avg_price * 100.0);
    let risk_rupee = worst_loss.map(|l| l.max(0.0) * abs_qty as f64 * multiplier);

    let ltp_ok = p.ltp.filter(|l| l.is_finite() && *l > 0.0);
    let wrong_side = match ltp_ok {
        Some(ltp) => triggers.iter().any(|&t| if long { t >= ltp } else { t <= ltp }),
        None => false,
    };

    let (status, detail) = if stale {
        (LegStatus::StaleData, Some("Broker data is stale or the login is down".to_string()))
    } else if protecting.is_empty() {
        if in_grace {
            (LegStatus::Grace, Some(format!("Within {grace_sec}s of opening the position")))
        } else {
            // Only stops from the current position count; an old cancelled SL must not be blamed.
            let current = stops_on_leg().filter(|o| opened_ts.map(|t| o.updated_ts >= t).unwrap_or(true));
            match current.max_by_key(|o| o.updated_ts) {
                Some(o) if o.state == OrderState::Rejected => (
                    LegStatus::SlRejected,
                    Some(o.reject_reason.clone().unwrap_or_else(|| o.status_raw.clone())),
                ),
                Some(o) if o.state == OrderState::Cancelled => {
                    (LegStatus::SlCancelled, Some(format!("Order {} was cancelled", o.broker_order_id)))
                }
                _ => (LegStatus::NoSl, Some("No stop-loss order on this leg".to_string())),
            }
        }
    } else if wrong_side {
        (LegStatus::WrongSide, Some("Trigger is on the wrong side of the last price".to_string()))
    } else if sl_qty < abs_qty {
        (LegStatus::PartialSl, Some(format!("SL covers {sl_qty} of {abs_qty}")))
    } else if sl_qty > abs_qty {
        (LegStatus::OverCovered, Some(format!("SL covers {sl_qty} but position is {abs_qty}")))
    } else {
        let too_far = match setting.mode {
            RiskMode::Pct => dist_pct.map(|d| d > setting.max_pct).unwrap_or(false),
            RiskMode::Rupee => risk_rupee.map(|r| r > setting.max_rupee).unwrap_or(false),
        };
        if too_far {
            (LegStatus::TooFar, Some("Stop is beyond the allowed loss limit".to_string()))
        } else if triggers.is_empty() {
            (LegStatus::Protected, Some("Trigger price unavailable, loss limit not checked".to_string()))
        } else if ltp_ok.is_none() {
            (LegStatus::Unverified, Some("No live price, wrong-side check skipped".to_string()))
        } else if !avg_ok && setting.mode == RiskMode::Pct {
            (LegStatus::Unverified, Some("Average price unusable, loss limit not checked".to_string()))
        } else {
            (LegStatus::Protected, None)
        }
    };

    LegEval {
        leg_key: leg_key(&snap.account_id, p.exchange, &p.symbol, p.product),
        account_id: snap.account_id.clone(),
        client_id: snap.client_id.clone(),
        broker: snap.broker,
        exchange: p.exchange,
        segment: p.segment,
        symbol: p.symbol.clone(),
        product: p.product,
        net_qty: p.net_qty,
        avg_price: p.avg_price,
        ltp: p.ltp,
        sl_order_ids: protecting.iter().map(|o| o.broker_order_id.clone()).collect(),
        sl_qty,
        sl_trigger,
        risk_mode: setting.mode,
        limit_value: match setting.mode {
            RiskMode::Pct => setting.max_pct,
            RiskMode::Rupee => setting.max_rupee,
        },
        dist_pct,
        risk_rupee,
        status,
        severity: status.severity(),
        detail,
        opened_ts,
        evaluated_at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::{RiskRule, RiskSetting};
    use chrono::TimeZone;

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 23, 5, 0, 0).unwrap()
    }

    fn pos(symbol: &str, net_qty: i64, avg: f64, ltp: f64) -> Position {
        Position {
            exchange: Exchange::Nfo,
            segment: Segment::Opt,
            symbol: symbol.into(),
            product: Product::Carryforward,
            net_qty,
            avg_price: avg,
            overnight_qty: 0,
            ltp: Some(ltp),
            multiplier: 1.0,
        }
    }

    fn stop(id: &str, symbol: &str, side: Side, qty: i64, trigger: f64, state: OrderState) -> Order {
        Order {
            broker_order_id: id.into(),
            exchange: Exchange::Nfo,
            symbol: symbol.into(),
            side,
            kind: OrderKind::SlMarket,
            product: Product::Carryforward,
            qty,
            filled_qty: 0,
            pending_qty: qty,
            price: 0.0,
            trigger_price: Some(trigger),
            state,
            status_raw: format!("{state:?}"),
            reject_reason: None,
            order_ts: t0(),
            updated_ts: t0(),
        }
    }

    fn fill(symbol: &str, side: Side, secs_before_now: i64) -> Trade {
        Trade {
            trade_id: format!("T-{symbol}-{secs_before_now}"),
            broker_order_id: "E1".into(),
            exchange: Exchange::Nfo,
            symbol: symbol.into(),
            side,
            product: Product::Carryforward,
            qty: 1,
            price: 100.0,
            fill_ts: t0() - Duration::seconds(secs_before_now),
        }
    }

    fn snap(positions: Vec<Position>, orders: Vec<Order>, trades: Vec<Trade>) -> Snapshot {
        Snapshot {
            account_id: "A1".into(),
            client_id: "C1".into(),
            broker: Broker::Zerodha,
            orders,
            trades,
            positions,
            fetched_at: t0(),
            healthy: true,
        }
    }

    fn eval_one(s: &Snapshot) -> LegEval {
        evaluate(s, &RiskBook::default(), t0()).remove(0)
    }

    fn status_of(positions: Vec<Position>, orders: Vec<Order>, trades: Vec<Trade>) -> LegStatus {
        eval_one(&snap(positions, orders, trades)).status
    }

    #[test]
    fn straddle_with_one_leg_missing_sl() {
        let s = snap(
            vec![pos("NIFTY26NOV24000CE", -75, 100.0, 90.0), pos("NIFTY26NOV24000PE", -75, 100.0, 90.0)],
            vec![stop("S1", "NIFTY26NOV24000CE", Side::Buy, 75, 130.0, OrderState::TriggerPending)],
            vec![fill("NIFTY26NOV24000CE", Side::Sell, 300), fill("NIFTY26NOV24000PE", Side::Sell, 300)],
        );
        let legs = evaluate(&s, &RiskBook::default(), t0());
        assert_eq!(legs.len(), 2);
        assert_eq!(legs[0].status, LegStatus::Protected);
        assert_eq!(legs[1].status, LegStatus::NoSl);
        assert_eq!(legs[1].severity, Severity::Red);
    }

    #[test]
    fn closed_position_is_ignored() {
        let s = snap(vec![pos("X", 0, 100.0, 90.0)], vec![], vec![]);
        assert!(evaluate(&s, &RiskBook::default(), t0()).is_empty());
    }

    #[test]
    fn grace_period_after_fill_then_red() {
        let p = vec![pos("X", -75, 100.0, 90.0)];
        assert_eq!(status_of(p.clone(), vec![], vec![fill("X", Side::Sell, 5)]), LegStatus::Grace);
        assert_eq!(status_of(p, vec![], vec![fill("X", Side::Sell, 60)]), LegStatus::NoSl);
    }

    #[test]
    fn add_on_restarts_grace_and_leaves_partial_sl() {
        let p = vec![pos("X", -150, 100.0, 90.0)];
        let o = vec![stop("S1", "X", Side::Buy, 75, 130.0, OrderState::TriggerPending)];
        let trades = vec![fill("X", Side::Sell, 600), fill("X", Side::Sell, 3)];
        // SL exists, so grace does not hide the partial cover.
        assert_eq!(status_of(p, o, trades), LegStatus::PartialSl);
    }

    #[test]
    fn rejected_sl_is_reported_with_reason() {
        let mut o = stop("S1", "X", Side::Buy, 75, 130.0, OrderState::Rejected);
        o.reject_reason = Some("RMS: Insufficient margin".into());
        let e = eval_one(&snap(vec![pos("X", -75, 100.0, 90.0)], vec![o], vec![fill("X", Side::Sell, 300)]));
        assert_eq!(e.status, LegStatus::SlRejected);
        assert_eq!(e.detail.as_deref(), Some("RMS: Insufficient margin"));
    }

    #[test]
    fn cancelled_sl_is_reported() {
        let o = stop("S1", "X", Side::Buy, 75, 130.0, OrderState::Cancelled);
        let st = status_of(vec![pos("X", -75, 100.0, 90.0)], vec![o], vec![fill("X", Side::Sell, 300)]);
        assert_eq!(st, LegStatus::SlCancelled);
    }

    #[test]
    fn cancel_pending_does_not_protect() {
        let o = stop("S1", "X", Side::Buy, 75, 130.0, OrderState::CancelPending);
        let st = status_of(vec![pos("X", -75, 100.0, 90.0)], vec![o], vec![fill("X", Side::Sell, 300)]);
        assert_eq!(st, LegStatus::NoSl);
    }

    #[test]
    fn stop_on_entry_side_does_not_count() {
        // A BUY stop on a long position adds exposure, it does not protect.
        let o = stop("S1", "X", Side::Buy, 75, 80.0, OrderState::TriggerPending);
        let st = status_of(vec![pos("X", 75, 100.0, 90.0)], vec![o], vec![fill("X", Side::Buy, 300)]);
        assert_eq!(st, LegStatus::NoSl);
    }

    #[test]
    fn wrong_side_long_and_short() {
        let long = status_of(
            vec![pos("X", 75, 100.0, 90.0)],
            vec![stop("S1", "X", Side::Sell, 75, 95.0, OrderState::TriggerPending)],
            vec![],
        );
        assert_eq!(long, LegStatus::WrongSide);
        let short = status_of(
            vec![pos("X", -75, 100.0, 90.0)],
            vec![stop("S1", "X", Side::Buy, 75, 85.0, OrderState::TriggerPending)],
            vec![],
        );
        assert_eq!(short, LegStatus::WrongSide);
    }

    #[test]
    fn partial_and_over_covered() {
        let p = vec![pos("X", -150, 100.0, 90.0)];
        let partial = status_of(
            p.clone(),
            vec![stop("S1", "X", Side::Buy, 75, 130.0, OrderState::TriggerPending)],
            vec![],
        );
        assert_eq!(partial, LegStatus::PartialSl);
        let two = vec![
            stop("S1", "X", Side::Buy, 75, 130.0, OrderState::TriggerPending),
            stop("S2", "X", Side::Buy, 75, 130.0, OrderState::TriggerPending),
        ];
        assert_eq!(status_of(p.clone(), two, vec![]), LegStatus::Protected);
        let big = vec![stop("S1", "X", Side::Buy, 300, 130.0, OrderState::TriggerPending)];
        assert_eq!(status_of(p, big, vec![]), LegStatus::OverCovered);
    }

    #[test]
    fn too_far_in_pct_mode() {
        // Short at 100, SL at 150 is a 50% loss; default limit is 40%.
        let e = eval_one(&snap(
            vec![pos("X", -75, 100.0, 90.0)],
            vec![stop("S1", "X", Side::Buy, 75, 150.0, OrderState::TriggerPending)],
            vec![],
        ));
        assert_eq!(e.status, LegStatus::TooFar);
        assert!((e.dist_pct.unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn too_far_in_rupee_mode() {
        let book = RiskBook {
            rules: vec![RiskRule {
                client_id: None,
                segment: None,
                setting: RiskSetting { mode: RiskMode::Rupee, max_pct: 40.0, max_rupee: 2_000.0, grace_sec: 15 },
            }],
        };
        // Loss = (130 - 100) * 75 = 2250 > 2000
        let s = snap(
            vec![pos("X", -75, 100.0, 90.0)],
            vec![stop("S1", "X", Side::Buy, 75, 130.0, OrderState::TriggerPending)],
            vec![],
        );
        let e = evaluate(&s, &book, t0()).remove(0);
        assert_eq!(e.status, LegStatus::TooFar);
        assert!((e.risk_rupee.unwrap() - 2250.0).abs() < 1e-9);
    }

    #[test]
    fn trailing_stop_in_profit_is_not_too_far() {
        // Long at 100, stop trailed up to 110: loss distance is negative.
        let e = eval_one(&snap(
            vec![pos("X", 75, 100.0, 130.0)],
            vec![stop("S1", "X", Side::Sell, 75, 110.0, OrderState::TriggerPending)],
            vec![],
        ));
        assert_eq!(e.status, LegStatus::Protected);
        assert_eq!(e.risk_rupee, Some(0.0));
    }

    #[test]
    fn stale_data_is_never_green() {
        let mut s = snap(
            vec![pos("X", -75, 100.0, 90.0)],
            vec![stop("S1", "X", Side::Buy, 75, 130.0, OrderState::TriggerPending)],
            vec![],
        );
        s.healthy = false;
        assert_eq!(eval_one(&s).status, LegStatus::StaleData);
        s.healthy = true;
        s.fetched_at = t0() - Duration::seconds(STALE_AFTER_SECS + 1);
        let e = eval_one(&s);
        assert_eq!(e.status, LegStatus::StaleData);
        assert_eq!(e.severity, Severity::Grey);
    }

    #[test]
    fn missing_trigger_price_is_protected_with_note() {
        // Kotak's trigger field is unconfirmed; the engine must not guess.
        let mut o = stop("S1", "X", Side::Buy, 75, 0.0, OrderState::TriggerPending);
        o.trigger_price = None;
        let e = eval_one(&snap(vec![pos("X", -75, 100.0, 90.0)], vec![o], vec![]));
        assert_eq!(e.status, LegStatus::Protected);
        assert!(e.detail.unwrap().contains("unavailable"));
    }

    #[test]
    fn gtt_counts_as_protection() {
        let mut o = stop("G1", "X", Side::Buy, 75, 130.0, OrderState::TriggerPending);
        o.kind = OrderKind::GttSl;
        assert_eq!(status_of(vec![pos("X", -75, 100.0, 90.0)], vec![o], vec![]), LegStatus::Protected);
    }

    #[test]
    fn state_mapping_from_raw_broker_text() {
        assert_eq!(OrderState::from_raw("TRIGGER PENDING"), OrderState::TriggerPending);
        assert_eq!(OrderState::from_raw("trigger pending"), OrderState::TriggerPending);
        assert_eq!(OrderState::from_raw("validation pending"), OrderState::PendingNew);
        assert_eq!(OrderState::from_raw("put order req received"), OrderState::PendingNew);
        assert_eq!(OrderState::from_raw("after market order req received"), OrderState::Amo);
        assert_eq!(OrderState::from_raw("cancel pending"), OrderState::CancelPending);
        assert_eq!(OrderState::from_raw("cancelled"), OrderState::Cancelled);
        assert_eq!(OrderState::from_raw("REJECTED"), OrderState::Rejected);
        assert_eq!(OrderState::from_raw("complete"), OrderState::Complete);
        assert_eq!(OrderState::from_raw("modified"), OrderState::Open);
        assert_eq!(OrderState::from_raw("???"), OrderState::Unknown);
    }

    #[test]
    fn overnight_unprotected_leg_gets_no_grace_from_a_fresh_add() {
        let mut p = pos("X", -150, 100.0, 90.0);
        p.overnight_qty = -75;
        let st = status_of(vec![p], vec![], vec![fill("X", Side::Sell, 5)]);
        assert_eq!(st, LegStatus::NoSl);
    }

    #[test]
    fn keeps_adding_without_sl_does_not_extend_grace() {
        // Opened 10 minutes ago, added again 5s ago: grace is measured from opening.
        let p = vec![pos("X", -150, 100.0, 90.0)];
        let trades = vec![fill("X", Side::Sell, 600), fill("X", Side::Sell, 5)];
        assert_eq!(status_of(p, vec![], trades), LegStatus::NoSl);
    }

    #[test]
    fn old_cancelled_sl_is_not_blamed_on_a_fresh_position() {
        let mut old = stop("S1", "X", Side::Buy, 75, 130.0, OrderState::Cancelled);
        old.updated_ts = t0() - Duration::seconds(7200);
        // Opened 2h ago, closed, reopened 5 min ago.
        let trades = vec![
            fill("X", Side::Sell, 10_800),
            fill("X", Side::Buy, 7_500),
            fill("X", Side::Sell, 300),
        ];
        let st = status_of(vec![pos("X", -1, 100.0, 90.0)], vec![old], trades);
        assert_eq!(st, LegStatus::NoSl);
    }

    #[test]
    fn missing_or_zero_ltp_is_never_plain_green() {
        let o = vec![stop("S1", "X", Side::Buy, 75, 130.0, OrderState::TriggerPending)];
        let mut p = pos("X", -75, 100.0, 90.0);
        p.ltp = None;
        assert_eq!(status_of(vec![p.clone()], o.clone(), vec![]), LegStatus::Unverified);
        p.ltp = Some(0.0);
        assert_eq!(status_of(vec![p.clone()], o.clone(), vec![]), LegStatus::Unverified);
        p.ltp = Some(f64::NAN);
        assert_eq!(status_of(vec![p], o, vec![]), LegStatus::Unverified);
    }

    #[test]
    fn unusable_average_price_is_unverified_in_pct_mode() {
        let o = vec![stop("S1", "X", Side::Buy, 75, 500.0, OrderState::TriggerPending)];
        for avg in [f64::NAN, f64::INFINITY, 0.0] {
            let st = status_of(vec![pos("X", -75, avg, 90.0)], o.clone(), vec![]);
            assert_eq!(st, LegStatus::Unverified, "avg {avg}");
        }
    }

    #[test]
    fn negative_multiplier_still_checks_rupee_limit() {
        let book = RiskBook {
            rules: vec![RiskRule {
                client_id: None,
                segment: None,
                setting: RiskSetting { mode: RiskMode::Rupee, max_pct: 40.0, max_rupee: 2_000.0, grace_sec: 15 },
            }],
        };
        let mut p = pos("X", -75, 100.0, 90.0);
        p.multiplier = -1.0;
        let s = snap(
            vec![p],
            vec![stop("S1", "X", Side::Buy, 75, 130.0, OrderState::TriggerPending)],
            vec![],
        );
        assert_eq!(evaluate(&s, &book, t0()).remove(0).status, LegStatus::TooFar);
    }

    #[test]
    fn absurd_grace_setting_does_not_panic() {
        for g in [i64::MIN, -5, i64::MAX] {
            let book = RiskBook {
                rules: vec![RiskRule {
                    client_id: None,
                    segment: None,
                    setting: RiskSetting { grace_sec: g, ..RiskSetting::default() },
                }],
            };
            let s = snap(vec![pos("X", -75, 100.0, 90.0)], vec![], vec![fill("X", Side::Sell, 5)]);
            let _ = evaluate(&s, &book, t0());
        }
        let mut p = pos("X", i64::MIN, 100.0, 90.0);
        p.overnight_qty = 0;
        let _ = evaluate(&snap(vec![p], vec![], vec![]), &RiskBook::default(), t0());
    }

    #[test]
    fn partial_sl_order_uses_pending_not_original_qty() {
        let mut o = stop("S1", "X", Side::Buy, 75, 130.0, OrderState::Partial);
        o.pending_qty = 25;
        let e = eval_one(&snap(vec![pos("X", -25, 100.0, 90.0)], vec![o], vec![]));
        assert_eq!(e.sl_qty, 25);
        assert_eq!(e.status, LegStatus::Protected);
    }

    #[test]
    fn more_raw_statuses() {
        assert_eq!(OrderState::from_raw("AMO REQ RECEIVED"), OrderState::Amo);
        assert_eq!(OrderState::from_raw("MODIFY VALIDATION PENDING"), OrderState::ModifyPending);
        assert_eq!(OrderState::from_raw("OPEN PENDING"), OrderState::PendingNew);
    }
}
