//! Tolerant JSON and time helpers. Brokers mix strings and numbers freely.

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde_json::Value;

pub fn ist() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("valid offset")
}

/// Read a field as f64 whether it is a JSON number or a numeric string. Missing or junk gives 0.
pub fn f64_of(v: &Value, key: &str) -> f64 {
    opt_f64(v, key).unwrap_or(0.0)
}

pub fn opt_f64(v: &Value, key: &str) -> Option<f64> {
    match v.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().replace(',', "").parse::<f64>().ok(),
        _ => None,
    }
}

pub fn i64_of(v: &Value, key: &str) -> i64 {
    opt_f64(v, key).map(|f| f.round() as i64).unwrap_or(0)
}

pub fn str_of(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

pub fn first_opt_f64(v: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|k| opt_f64(v, k))
}

/// Interpret a naive IST timestamp as UTC.
pub fn ist_to_utc(naive: NaiveDateTime) -> DateTime<Utc> {
    ist().from_local_datetime(&naive).single().map(|d| d.with_timezone(&Utc)).unwrap_or_else(Utc::now)
}

/// Try several broker timestamp formats; fall back to `fallback` rather than failing the poll.
pub fn parse_ts(s: &str, fallback: DateTime<Utc>) -> DateTime<Utc> {
    const FORMATS: [&str; 5] = [
        "%Y-%m-%d %H:%M:%S",
        "%d-%b-%Y %H:%M:%S",
        "%d-%m-%Y %H:%M:%S",
        "%d-%b-%Y %I:%M:%S %p",
        "%Y-%m-%dT%H:%M:%S",
    ];
    let s = s.trim();
    for f in FORMATS {
        if let Ok(n) = NaiveDateTime::parse_from_str(s, f) {
            return ist_to_utc(n);
        }
    }
    fallback
}

/// Combine a time-only value ("13:27:53") with today's IST date.
pub fn time_today(s: &str, today_ist: NaiveDate, fallback: DateTime<Utc>) -> DateTime<Utc> {
    match NaiveTime::parse_from_str(s.trim(), "%H:%M:%S") {
        Ok(t) => ist_to_utc(today_ist.and_time(t)),
        Err(_) => fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_from_strings_and_numbers() {
        let v = json!({"a": "12.5", "b": 7, "c": "1,250.50", "d": "x"});
        assert_eq!(f64_of(&v, "a"), 12.5);
        assert_eq!(i64_of(&v, "b"), 7);
        assert_eq!(f64_of(&v, "c"), 1250.5);
        assert_eq!(opt_f64(&v, "d"), None);
        assert_eq!(f64_of(&v, "missing"), 0.0);
    }

    #[test]
    fn timestamps_are_ist() {
        let fb = Utc::now();
        // 10:00 IST is 04:30 UTC.
        let t = parse_ts("2026-11-23 10:00:00", fb);
        assert_eq!(t.format("%H:%M").to_string(), "04:30");
        let t = parse_ts("20-Oct-2020 13:10:59", fb);
        assert_eq!(t.format("%Y-%m-%d %H:%M").to_string(), "2020-10-20 07:40");
        assert_eq!(parse_ts("garbage", fb), fb);
    }
}
