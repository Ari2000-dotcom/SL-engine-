//! Kotak Neo normalizer.
//!
//! UNCONFIRMED against live data (the public docs omit them), each one isolated below so a live
//! call can fix it in one place:
//!   * the order-book trigger price key (`TRIGGER_KEYS`),
//!   * the timestamp format of `ordDtTm` / `exTm` (several formats are tried),
//!   * whether quantities are in units or lots (treated as units).

use crate::util::*;
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sl_core::*;

/// Candidate response keys for the SL trigger price. The request field is `tp`.
pub const TRIGGER_KEYS: [&str; 5] = ["trgPrc", "trigPrc", "tp", "trgprc", "triggerPrice"];

pub fn exchange(seg: &str) -> Option<Exchange> {
    Some(match seg {
        "nse_cm" => Exchange::Nse,
        "bse_cm" => Exchange::Bse,
        "nse_fo" => Exchange::Nfo,
        "bse_fo" => Exchange::Bfo,
        "mcx_fo" => Exchange::Mcx,
        "cde_fo" | "bcs_fo" => Exchange::Cds,
        _ => return None,
    })
}

pub fn product(s: &str) -> Product {
    match s {
        "MIS" | "CO" | "BO" => Product::Intraday,
        "CNC" => Product::Delivery,
        "MTF" => Product::Mtf,
        _ => Product::Carryforward, // NRML
    }
}

fn side(s: &str) -> Side {
    if s.eq_ignore_ascii_case("S") || s.eq_ignore_ascii_case("SELL") {
        Side::Sell
    } else {
        Side::Buy
    }
}

fn kind(s: &str) -> OrderKind {
    match s {
        "SL" => OrderKind::SlLimit,
        "SL-M" => OrderKind::SlMarket,
        "MKT" => OrderKind::Market,
        _ => OrderKind::Limit, // L, LMT
    }
}

fn items(data: &Value) -> &[Value] {
    data.as_array().map(Vec::as_slice).unwrap_or(&[])
}

pub fn parse_orders(data: &Value, now: DateTime<Utc>) -> Vec<Order> {
    items(data)
        .iter()
        .filter_map(|o| {
            let exchange = exchange(&str_of(o, "exSeg"))?;
            let qty = i64_of(o, "qty");
            let filled = i64_of(o, "fldQty");
            let pending = opt_f64(o, "unFldSz").map(|f| f as i64).unwrap_or(qty - filled);
            let status_raw = str_of(o, "ordSt");
            let reason = str_of(o, "rejRsn");
            let ts = parse_ts(&str_of(o, "ordDtTm"), now);
            Some(Order {
                broker_order_id: str_of(o, "nOrdNo"),
                exchange,
                symbol: str_of(o, "trdSym"),
                side: side(&str_of(o, "trnsTp")),
                kind: kind(&str_of(o, "prcTp")),
                product: product(&str_of(o, "prod")),
                qty,
                filled_qty: filled,
                pending_qty: pending,
                price: f64_of(o, "prc"),
                trigger_price: first_opt_f64(o, &TRIGGER_KEYS).filter(|t| *t > 0.0),
                state: OrderState::from_raw(&status_raw),
                status_raw,
                reject_reason: (!reason.is_empty()).then_some(reason),
                order_ts: ts,
                updated_ts: parse_ts(&str_of(o, "flDtTm"), ts),
            })
        })
        .collect()
}

pub fn parse_trades(data: &Value, now: DateTime<Utc>) -> Vec<Trade> {
    items(data)
        .iter()
        .filter_map(|t| {
            Some(Trade {
                trade_id: format!("{}-{}", str_of(t, "nOrdNo"), str_of(t, "exOrdId")),
                broker_order_id: str_of(t, "nOrdNo"),
                exchange: exchange(&str_of(t, "exSeg"))?,
                symbol: str_of(t, "trdSym"),
                side: side(&str_of(t, "trnsTp")),
                product: product(&str_of(t, "prod")),
                qty: i64_of(t, "fldQty"),
                price: f64_of(t, "avgPrc"),
                // An unreadable fill time must never extend the grace period, so fall back to "old".
                fill_ts: parse_ts(&str_of(t, "exTm"), now - Duration::hours(1)),
            })
        })
        .collect()
}

/// Net quantity and average from carry-forward plus day fills. All fields arrive as strings.
pub fn parse_positions(data: &Value) -> Vec<Position> {
    items(data)
        .iter()
        .filter_map(|p| {
            let exchange = exchange(&str_of(p, "exSeg"))?;
            let symbol = str_of(p, "trdSym");
            let buy_qty = i64_of(p, "cfBuyQty") + i64_of(p, "flBuyQty");
            let sell_qty = i64_of(p, "cfSellQty") + i64_of(p, "flSellQty");
            let net = buy_qty - sell_qty;
            let buy_amt = f64_of(p, "cfBuyAmt") + f64_of(p, "buyAmt");
            let sell_amt = f64_of(p, "cfSellAmt") + f64_of(p, "sellAmt");
            let avg = if net > 0 && buy_qty > 0 {
                buy_amt / buy_qty as f64
            } else if net < 0 && sell_qty > 0 {
                sell_amt / sell_qty as f64
            } else {
                0.0
            };
            Some(Position {
                segment: Segment::infer(exchange, &symbol),
                exchange,
                symbol,
                product: product(&str_of(p, "prod")),
                net_qty: net,
                avg_price: avg,
                overnight_qty: i64_of(p, "cfBuyQty") - i64_of(p, "cfSellQty"),
                ltp: None,
                multiplier: opt_f64(p, "multiplier").filter(|m| *m > 0.0).unwrap_or(1.0),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sl_order_with_guessed_trigger_key() {
        let data = json!([{
            "nOrdNo": "2411", "ordSt": "trigger pending", "trdSym": "NIFTY26NOV24000CE",
            "qty": 75, "prc": "0", "trnsTp": "B", "prcTp": "SL-M", "exSeg": "nse_fo",
            "prod": "NRML", "trgPrc": "130.5", "ordDtTm": "23-Nov-2026 10:00:01"
        }]);
        let o = parse_orders(&data, Utc::now());
        assert_eq!(o[0].kind, OrderKind::SlMarket);
        assert_eq!(o[0].state, OrderState::TriggerPending);
        assert_eq!(o[0].trigger_price, Some(130.5));
        assert_eq!(o[0].exchange, Exchange::Nfo);
        assert_eq!(o[0].pending_qty, 75);
    }

    #[test]
    fn missing_trigger_key_gives_none_not_zero() {
        let data = json!([{
            "nOrdNo": "1", "ordSt": "open", "trdSym": "X", "qty": 10, "trnsTp": "S",
            "prcTp": "SL", "exSeg": "nse_fo", "prod": "MIS"
        }]);
        assert_eq!(parse_orders(&data, Utc::now())[0].trigger_price, None);
    }

    #[test]
    fn short_position_from_day_fills() {
        let data = json!([{
            "exSeg": "nse_fo", "trdSym": "NIFTY26NOV24000CE", "prod": "NRML",
            "cfBuyQty": "0", "cfSellQty": "0", "flBuyQty": "0", "flSellQty": "75",
            "cfBuyAmt": "0", "cfSellAmt": "0", "buyAmt": "0", "sellAmt": "7593.75"
        }]);
        let p = parse_positions(&data);
        assert_eq!(p[0].net_qty, -75);
        assert!((p[0].avg_price - 101.25).abs() < 1e-9);
    }

    #[test]
    fn carried_long_position() {
        let data = json!([{
            "exSeg": "nse_fo", "trdSym": "X", "prod": "NRML",
            "cfBuyQty": "75", "cfSellQty": "0", "flBuyQty": "0", "flSellQty": "0",
            "cfBuyAmt": "7500", "cfSellAmt": "0", "buyAmt": "0", "sellAmt": "0"
        }]);
        let p = parse_positions(&data);
        assert_eq!(p[0].net_qty, 75);
        assert_eq!(p[0].overnight_qty, 75);
        assert_eq!(p[0].avg_price, 100.0);
    }

    #[test]
    fn unreadable_fill_time_is_old_not_now() {
        let now = Utc::now();
        let data = json!([{"nOrdNo": "1", "exOrdId": "9", "exSeg": "nse_fo", "trdSym": "X",
            "trnsTp": "S", "prod": "NRML", "fldQty": 75, "avgPrc": "100", "exTm": "???"}]);
        let t = parse_trades(&data, now);
        assert!(now - t[0].fill_ts >= Duration::minutes(59));
    }
}
