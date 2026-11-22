//! Zerodha Kite Connect normalizer. Pure functions over the `data` payloads, so they test offline.

use crate::util::*;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sl_core::*;

pub fn exchange(s: &str) -> Option<Exchange> {
    Some(match s {
        "NSE" => Exchange::Nse,
        "BSE" => Exchange::Bse,
        "NFO" => Exchange::Nfo,
        "BFO" => Exchange::Bfo,
        "MCX" => Exchange::Mcx,
        "CDS" | "BCD" => Exchange::Cds,
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
    if s.eq_ignore_ascii_case("SELL") {
        Side::Sell
    } else {
        Side::Buy
    }
}

fn kind(s: &str) -> OrderKind {
    match s {
        "SL" => OrderKind::SlLimit,
        "SL-M" => OrderKind::SlMarket,
        "MARKET" => OrderKind::Market,
        _ => OrderKind::Limit,
    }
}

fn items(data: &Value) -> &[Value] {
    data.as_array().map(Vec::as_slice).unwrap_or(&[])
}

pub fn parse_orders(data: &Value, now: DateTime<Utc>) -> Vec<Order> {
    items(data)
        .iter()
        .filter_map(|o| {
            let exchange = exchange(&str_of(o, "exchange"))?;
            let qty = i64_of(o, "quantity");
            let filled = i64_of(o, "filled_quantity");
            let pending = opt_f64(o, "pending_quantity").map(|f| f as i64).unwrap_or(qty - filled);
            let status_raw = str_of(o, "status");
            let reason = str_of(o, "status_message");
            let trigger = opt_f64(o, "trigger_price").filter(|t| *t > 0.0);
            let ts = parse_ts(&str_of(o, "order_timestamp"), now);
            Some(Order {
                broker_order_id: str_of(o, "order_id"),
                exchange,
                symbol: str_of(o, "tradingsymbol"),
                side: side(&str_of(o, "transaction_type")),
                kind: kind(&str_of(o, "order_type")),
                product: product(&str_of(o, "product")),
                qty,
                filled_qty: filled,
                pending_qty: pending,
                price: f64_of(o, "price"),
                trigger_price: trigger,
                state: OrderState::from_raw(&status_raw),
                status_raw,
                reject_reason: (!reason.is_empty()).then_some(reason),
                order_ts: ts,
                updated_ts: parse_ts(&str_of(o, "exchange_update_timestamp"), ts),
            })
        })
        .collect()
}

pub fn parse_trades(data: &Value, now: DateTime<Utc>) -> Vec<Trade> {
    // Kite trades carry exchange, tradingsymbol, product and transaction_type directly.
    items(data)
        .iter()
        .filter_map(|t| {
            Some(Trade {
                trade_id: str_of(t, "trade_id"),
                broker_order_id: str_of(t, "order_id"),
                exchange: exchange(&str_of(t, "exchange"))?,
                symbol: str_of(t, "tradingsymbol"),
                side: side(&str_of(t, "transaction_type")),
                product: product(&str_of(t, "product")),
                qty: i64_of(t, "quantity"),
                price: f64_of(t, "average_price"),
                // An unreadable fill time must never extend the grace period, so fall back to "old".
                fill_ts: parse_ts(&str_of(t, "fill_timestamp"), now - chrono::Duration::hours(1)),
            })
        })
        .collect()
}

/// `data` is the `net` array of GET /portfolio/positions.
pub fn parse_positions(net: &Value) -> Vec<Position> {
    items(net)
        .iter()
        .filter_map(|p| {
            let exchange = exchange(&str_of(p, "exchange"))?;
            let symbol = str_of(p, "tradingsymbol");
            let multiplier = opt_f64(p, "multiplier").filter(|m| *m > 0.0).unwrap_or(1.0);
            Some(Position {
                segment: Segment::infer(exchange, &symbol),
                exchange,
                symbol,
                product: product(&str_of(p, "product")),
                net_qty: i64_of(p, "quantity"),
                avg_price: f64_of(p, "average_price"),
                overnight_qty: i64_of(p, "overnight_quantity"),
                ltp: opt_f64(p, "last_price").filter(|l| *l > 0.0),
                multiplier,
            })
        })
        .collect()
}

/// GET /gtt/triggers. Only active rules protect; each rule leg becomes a synthetic stop order.
pub fn parse_gtts(data: &Value, now: DateTime<Utc>) -> Vec<Order> {
    let mut out = Vec::new();
    for g in items(data) {
        let cond = &g["condition"];
        let Some(exchange) = exchange(&str_of(cond, "exchange")) else { continue };
        let symbol = str_of(cond, "tradingsymbol");
        let triggers: Vec<f64> = cond["trigger_values"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default();
        let status = str_of(g, "status");
        let state = match status.as_str() {
            "active" => OrderState::TriggerPending,
            "triggered" => OrderState::Complete,
            "rejected" => OrderState::Rejected,
            _ => OrderState::Cancelled, // disabled, expired, cancelled, deleted
        };
        let ts = parse_ts(&str_of(g, "updated_at"), now);
        let legs = g["orders"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        for (i, leg) in legs.iter().enumerate() {
            let qty = i64_of(leg, "quantity");
            out.push(Order {
                broker_order_id: format!("GTT-{}-{}", str_of(g, "id"), i),
                exchange,
                symbol: symbol.clone(),
                side: side(&str_of(leg, "transaction_type")),
                kind: OrderKind::GttSl,
                product: product(&str_of(leg, "product")),
                qty,
                filled_qty: 0,
                pending_qty: qty,
                price: f64_of(leg, "price"),
                trigger_price: triggers.get(i).copied(),
                state,
                status_raw: format!("gtt:{status}"),
                reject_reason: None,
                order_ts: ts,
                updated_ts: ts,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sl_m_order_in_trigger_pending() {
        let data = json!([{
            "order_id": "2211", "exchange": "NFO", "tradingsymbol": "NIFTY26NOV24000CE",
            "transaction_type": "BUY", "order_type": "SL-M", "product": "NRML",
            "quantity": 75, "filled_quantity": 0, "pending_quantity": 75,
            "price": 0, "trigger_price": 130.5, "status": "TRIGGER PENDING",
            "status_message": null, "order_timestamp": "2026-11-23 10:00:01"
        }]);
        let o = parse_orders(&data, Utc::now());
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].kind, OrderKind::SlMarket);
        assert_eq!(o[0].state, OrderState::TriggerPending);
        assert_eq!(o[0].product, Product::Carryforward);
        assert_eq!(o[0].trigger_price, Some(130.5));
        assert_eq!(o[0].pending_qty, 75);
        assert!(o[0].reject_reason.is_none());
    }

    #[test]
    fn rejected_order_keeps_reason() {
        let data = json!([{
            "order_id": "9", "exchange": "NFO", "tradingsymbol": "X", "transaction_type": "BUY",
            "order_type": "SL", "product": "MIS", "quantity": 75, "filled_quantity": 0,
            "pending_quantity": 0, "trigger_price": 10, "status": "REJECTED",
            "status_message": "Insufficient funds", "order_timestamp": "2026-11-23 10:00:01"
        }]);
        let o = parse_orders(&data, Utc::now());
        assert_eq!(o[0].state, OrderState::Rejected);
        assert_eq!(o[0].reject_reason.as_deref(), Some("Insufficient funds"));
        assert_eq!(o[0].product, Product::Intraday);
    }

    #[test]
    fn short_position_is_negative() {
        let net = json!([{
            "tradingsymbol": "NIFTY26NOV24000CE", "exchange": "NFO", "product": "NRML",
            "quantity": -75, "overnight_quantity": 0, "average_price": 101.25,
            "last_price": 95.0, "multiplier": 1
        }]);
        let p = parse_positions(&net);
        assert_eq!(p[0].net_qty, -75);
        assert_eq!(p[0].segment, Segment::Opt);
        assert_eq!(p[0].ltp, Some(95.0));
    }

    #[test]
    fn active_gtt_becomes_protecting_stop() {
        let data = json!([{
            "id": 77, "status": "active", "type": "single",
            "condition": {"exchange": "NFO", "tradingsymbol": "X", "trigger_values": [130.0], "last_price": 90.0},
            "orders": [{"transaction_type": "BUY", "quantity": 75, "product": "NRML", "price": 135.0}],
            "updated_at": "2026-11-23 10:00:00"
        }]);
        let o = parse_gtts(&data, Utc::now());
        assert_eq!(o.len(), 1);
        assert!(o[0].kind.is_stop());
        assert!(o[0].state.is_live_protection());
        assert_eq!(o[0].trigger_price, Some(130.0));
    }

    #[test]
    fn non_active_gtt_does_not_protect() {
        let data = json!([{
            "id": 1, "status": "expired",
            "condition": {"exchange": "NFO", "tradingsymbol": "X", "trigger_values": [130.0]},
            "orders": [{"transaction_type": "BUY", "quantity": 75, "product": "NRML"}]
        }]);
        assert!(!parse_gtts(&data, Utc::now())[0].state.is_live_protection());
    }

    #[test]
    fn unknown_exchange_rows_are_skipped_not_fatal() {
        let data = json!([{"order_id": "1", "exchange": "ZZZ"}]);
        assert!(parse_orders(&data, Utc::now()).is_empty());
    }
}
