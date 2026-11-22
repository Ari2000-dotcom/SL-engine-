//! Angel One SmartAPI normalizer. Numbers arrive as strings; timestamps use "20-Oct-2020 13:10:59".

use crate::util::*;
use chrono::{DateTime, NaiveDate, Utc};
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
        "INTRADAY" | "BO" => Product::Intraday,
        "DELIVERY" => Product::Delivery,
        // CARRYFORWARD and MARGIN are the F&O carry products.
        _ => Product::Carryforward,
    }
}

fn side(s: &str) -> Side {
    if s.eq_ignore_ascii_case("SELL") {
        Side::Sell
    } else {
        Side::Buy
    }
}

fn kind(ordertype: &str, variety: &str) -> OrderKind {
    match ordertype {
        "STOPLOSS_LIMIT" => OrderKind::SlLimit,
        "STOPLOSS_MARKET" => OrderKind::SlMarket,
        "MARKET" => OrderKind::Market,
        // STOPLOSS variety with a plain LIMIT type is still a stop order.
        _ if variety == "STOPLOSS" => OrderKind::SlLimit,
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
            let filled = i64_of(o, "filledshares");
            let status_raw = {
                let s = str_of(o, "orderstatus");
                if s.is_empty() { str_of(o, "status") } else { s }
            };
            let pending = opt_f64(o, "unfilledshares").map(|f| f as i64).unwrap_or(qty - filled);
            let reason = str_of(o, "text");
            let ts = parse_ts(&str_of(o, "updatetime"), now);
            Some(Order {
                broker_order_id: str_of(o, "orderid"),
                exchange,
                symbol: str_of(o, "tradingsymbol"),
                side: side(&str_of(o, "transactiontype")),
                kind: kind(&str_of(o, "ordertype"), &str_of(o, "variety")),
                product: product(&str_of(o, "producttype")),
                qty,
                filled_qty: filled,
                pending_qty: pending,
                price: f64_of(o, "price"),
                trigger_price: opt_f64(o, "triggerprice").filter(|t| *t > 0.0),
                state: OrderState::from_raw(&status_raw),
                status_raw,
                reject_reason: (!reason.is_empty()).then_some(reason),
                order_ts: ts,
                updated_ts: ts,
            })
        })
        .collect()
}

/// `filltime` has no date, so it is combined with today's IST date.
pub fn parse_trades(data: &Value, today_ist: NaiveDate, now: DateTime<Utc>) -> Vec<Trade> {
    items(data)
        .iter()
        .filter_map(|t| {
            Some(Trade {
                trade_id: str_of(t, "fillid"),
                broker_order_id: str_of(t, "orderid"),
                exchange: exchange(&str_of(t, "exchange"))?,
                symbol: str_of(t, "tradingsymbol"),
                side: side(&str_of(t, "transactiontype")),
                product: product(&str_of(t, "producttype")),
                qty: i64_of(t, "fillsize"),
                price: f64_of(t, "fillprice"),
                // An unreadable fill time must never extend the grace period, so fall back to "old".
                fill_ts: time_today(&str_of(t, "filltime"), today_ist, now - chrono::Duration::hours(1)),
            })
        })
        .collect()
}

pub fn parse_positions(data: &Value) -> Vec<Position> {
    items(data)
        .iter()
        .filter_map(|p| {
            let exchange = exchange(&str_of(p, "exchange"))?;
            let symbol = str_of(p, "tradingsymbol");
            let net = i64_of(p, "netqty");
            let avg = if net > 0 {
                first_opt_f64(p, &["totalbuyavgprice", "buyavgprice", "netprice", "avgnetprice"])
            } else {
                first_opt_f64(p, &["totalsellavgprice", "sellavgprice", "netprice", "avgnetprice"])
            }
            .unwrap_or(0.0);
            Some(Position {
                segment: Segment::infer(exchange, &symbol),
                exchange,
                symbol,
                product: product(&str_of(p, "producttype")),
                net_qty: net,
                avg_price: avg,
                overnight_qty: i64_of(p, "cfbuyqty") - i64_of(p, "cfsellqty"),
                ltp: None, // not in getPosition; filled from the quote API
                multiplier: opt_f64(p, "multiplier").filter(|m| *m > 0.0).unwrap_or(1.0),
            })
        })
        .collect()
}

/// POST market/v1/quote response: data.fetched[] with tradingSymbol and ltp.
pub fn parse_ltp(data: &Value) -> Vec<(String, f64)> {
    data["fetched"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| Some((r["tradingSymbol"].as_str()?.to_string(), r["ltp"].as_f64()?)))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stoploss_market_trigger_pending() {
        let data = json!([{
            "variety": "STOPLOSS", "ordertype": "STOPLOSS_MARKET", "producttype": "CARRYFORWARD",
            "quantity": "75", "filledshares": "0", "unfilledshares": "75", "price": "0",
            "triggerprice": "130.50", "tradingsymbol": "NIFTY26NOV24000CE", "transactiontype": "BUY",
            "exchange": "NFO", "orderid": "2301", "orderstatus": "trigger pending", "text": "",
            "updatetime": "23-Nov-2026 10:00:01"
        }]);
        let o = parse_orders(&data, Utc::now());
        assert_eq!(o[0].kind, OrderKind::SlMarket);
        assert_eq!(o[0].state, OrderState::TriggerPending);
        assert_eq!(o[0].trigger_price, Some(130.5));
        assert_eq!(o[0].pending_qty, 75);
        assert_eq!(o[0].product, Product::Carryforward);
    }

    #[test]
    fn rejection_text_is_kept() {
        let data = json!([{
            "variety": "STOPLOSS", "ordertype": "STOPLOSS_LIMIT", "producttype": "INTRADAY",
            "quantity": "75", "filledshares": "0", "unfilledshares": "75", "triggerprice": "10",
            "tradingsymbol": "X", "transactiontype": "BUY", "exchange": "NFO", "orderid": "5",
            "orderstatus": "rejected", "text": "Your order has been rejected due to Insufficient Funds",
            "updatetime": "23-Nov-2026 10:00:01"
        }]);
        let o = parse_orders(&data, Utc::now());
        assert_eq!(o[0].state, OrderState::Rejected);
        assert!(o[0].reject_reason.as_ref().unwrap().contains("Insufficient"));
    }

    #[test]
    fn short_position_uses_sell_average() {
        let data = json!([{
            "exchange": "NFO", "tradingsymbol": "X", "producttype": "CARRYFORWARD",
            "netqty": "-75", "buyqty": "0", "sellqty": "75", "cfbuyqty": "0", "cfsellqty": "0",
            "totalbuyavgprice": "0.00", "totalsellavgprice": "101.25", "multiplier": "1"
        }]);
        let p = parse_positions(&data);
        assert_eq!(p[0].net_qty, -75);
        assert_eq!(p[0].avg_price, 101.25);
        assert!(p[0].ltp.is_none());
    }

    #[test]
    fn fill_time_gets_todays_date() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 23).unwrap();
        let data = json!([{
            "exchange": "NFO", "tradingsymbol": "X", "producttype": "CARRYFORWARD",
            "transactiontype": "SELL", "fillprice": "101.25", "fillsize": "75",
            "orderid": "1", "fillid": "F1", "filltime": "10:00:05"
        }]);
        let t = parse_trades(&data, today, Utc::now());
        assert_eq!(t[0].side, Side::Sell);
        assert_eq!(t[0].fill_ts.format("%Y-%m-%d %H:%M:%S").to_string(), "2026-11-23 04:30:05");
    }

    #[test]
    fn quote_response_to_pairs() {
        let data = json!({"fetched": [{"exchange": "NFO", "tradingSymbol": "X", "symbolToken": "1", "ltp": 95.5}], "unfetched": []});
        assert_eq!(parse_ltp(&data), vec![("X".to_string(), 95.5)]);
    }
}
