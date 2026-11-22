//! Daily store: Parquet files written and read through DuckDB. Everything lives under
//! `<root>/<YYYY-MM-DD>/` and is deleted when the trading day rolls over at 08:30 IST.
//!
//! Snapshot tables (legs, incidents, ...) are one file, rewritten atomically. Event tables are
//! appended as `table/part-N.parquet` so a crash never corrupts earlier data.

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, Utc};
use duckdb::Connection;
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("duckdb: {0}")]
    Db(#[from] duckdb::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsafe path or table name: {0}")]
    BadName(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// The trading day a moment belongs to. The day flips at 08:30 IST, before the market opens,
/// so late-night and pre-open activity still belongs to the previous day.
pub fn trading_day(now: DateTime<Utc>) -> NaiveDate {
    let ist = FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("valid offset");
    (now.with_timezone(&ist) - Duration::minutes(8 * 60 + 30)).date_naive()
}

pub struct DayStore {
    root: PathBuf,
    day: Mutex<NaiveDate>,
    conn: Mutex<Connection>,
    seq: AtomicU64,
}

fn check_name(name: &str) -> Result<()> {
    let ok = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok { Ok(()) } else { Err(StoreError::BadName(name.to_string())) }
}

fn sql_path(p: &Path) -> Result<String> {
    let s = p.to_string_lossy().replace('\\', "/");
    if s.contains('\'') {
        return Err(StoreError::BadName(s));
    }
    Ok(s)
}

fn parse_day_dir(name: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(name, "%Y-%m-%d").ok()
}

impl DayStore {
    pub fn open(root: impl Into<PathBuf>, now: DateTime<Utc>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        let store = DayStore {
            root,
            day: Mutex::new(trading_day(now)),
            conn: Mutex::new(Connection::open_in_memory()?),
            seq: AtomicU64::new(0),
        };
        store.purge_other_days()?;
        fs::create_dir_all(store.day_dir())?;
        Ok(store)
    }

    pub fn day_dir(&self) -> PathBuf {
        self.root.join(self.day.lock().expect("day lock").format("%Y-%m-%d").to_string())
    }

    /// Delete every dated folder except the current trading day. Only folders whose name is a
    /// plain date are touched, so nothing else under the root can be removed.
    fn purge_other_days(&self) -> Result<()> {
        let keep = *self.day.lock().expect("day lock");
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if entry.file_type()?.is_dir() && parse_day_dir(&name).is_some_and(|d| d != keep) {
                fs::remove_dir_all(entry.path())?;
            }
        }
        Ok(())
    }

    /// Roll to the new trading day if the clock crossed 08:30 IST. Returns true when it did.
    pub fn rollover_if_needed(&self, now: DateTime<Utc>) -> Result<bool> {
        let today = trading_day(now);
        {
            let mut day = self.day.lock().expect("day lock");
            if *day == today {
                return Ok(false);
            }
            *day = today;
        }
        self.purge_other_days()?;
        fs::create_dir_all(self.day_dir())?;
        Ok(true)
    }

    fn write_jsonl<T: Serialize>(&self, rows: &[T], path: &Path) -> Result<()> {
        let mut f = fs::File::create(path)?;
        for r in rows {
            serde_json::to_writer(&mut f, r)?;
            f.write_all(b"\n")?;
        }
        Ok(())
    }

    fn jsonl_to_parquet(&self, jsonl: &Path, out_tmp: &Path) -> Result<()> {
        let sql = format!(
            "COPY (SELECT * FROM read_json_auto('{}')) TO '{}' (FORMAT PARQUET)",
            sql_path(jsonl)?,
            sql_path(out_tmp)?
        );
        self.conn.lock().expect("conn lock").execute_batch(&sql)?;
        Ok(())
    }

    /// Replace `<day>/<table>.parquet` with these rows. An empty slice removes the file so old
    /// rows cannot linger.
    pub fn write_snapshot<T: Serialize>(&self, table: &str, rows: &[T]) -> Result<()> {
        check_name(table)?;
        let dir = self.day_dir();
        fs::create_dir_all(&dir)?;
        let out = dir.join(format!("{table}.parquet"));
        if rows.is_empty() {
            let _ = fs::remove_file(&out);
            return Ok(());
        }
        let jsonl = dir.join(format!("{table}.jsonl.tmp"));
        let tmp = dir.join(format!("{table}.parquet.tmp"));
        self.write_jsonl(rows, &jsonl)?;
        let result = self.jsonl_to_parquet(&jsonl, &tmp);
        let _ = fs::remove_file(&jsonl);
        result?;
        fs::rename(&tmp, &out)?;
        Ok(())
    }

    /// Add rows as a new part file `<day>/<table>/part-N.parquet`.
    pub fn append<T: Serialize>(&self, table: &str, rows: &[T]) -> Result<()> {
        check_name(table)?;
        if rows.is_empty() {
            return Ok(());
        }
        let dir = self.day_dir().join(table);
        fs::create_dir_all(&dir)?;
        let n = self.seq.fetch_add(1, Ordering::SeqCst);
        let stamp = Utc::now().timestamp_millis();
        let jsonl = dir.join(format!("part-{stamp}-{n}.jsonl.tmp"));
        let tmp = dir.join(format!("part-{stamp}-{n}.parquet.tmp"));
        let out = dir.join(format!("part-{stamp}-{n}.parquet"));
        self.write_jsonl(rows, &jsonl)?;
        let result = self.jsonl_to_parquet(&jsonl, &tmp);
        let _ = fs::remove_file(&jsonl);
        result?;
        fs::rename(&tmp, &out)?;
        Ok(())
    }

    /// Run a read-only query over today's files. Use `{day}` in the SQL for the day folder, for
    /// example `SELECT status, count(*) AS n FROM read_parquet('{day}/legs.parquet') GROUP BY 1`.
    pub fn query(&self, sql: &str) -> Result<Vec<serde_json::Value>> {
        let day = sql_path(&self.day_dir())?;
        let sql = sql.replace("{day}", &day);
        let wrapped = format!("SELECT to_json(t)::VARCHAR FROM ({sql}) t");
        let conn = self.conn.lock().expect("conn lock");
        let mut stmt = conn.prepare(&wrapped)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(serde_json::from_str(&r?)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde::Serialize;

    #[derive(Serialize)]
    struct Row {
        symbol: String,
        status: String,
        qty: i64,
        tags: Vec<String>,
    }

    fn row(symbol: &str, status: &str, qty: i64) -> Row {
        Row { symbol: symbol.into(), status: status.into(), qty, tags: vec!["a".into(), "b".into()] }
    }

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("slg-store-test-{name}-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap_or(0)));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        // Given in IST for readability.
        let ist = FixedOffset::east_opt(19800).unwrap();
        ist.with_ymd_and_hms(y, m, d, h, min, 0).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn trading_day_flips_at_0830_ist() {
        assert_eq!(trading_day(at(2026, 11, 23, 8, 29)), NaiveDate::from_ymd_opt(2026, 11, 22).unwrap());
        assert_eq!(trading_day(at(2026, 11, 23, 8, 30)), NaiveDate::from_ymd_opt(2026, 11, 23).unwrap());
        assert_eq!(trading_day(at(2026, 11, 23, 23, 59)), NaiveDate::from_ymd_opt(2026, 11, 23).unwrap());
    }

    #[test]
    fn snapshot_round_trips_and_is_replaced() {
        let root = scratch("snap");
        let store = DayStore::open(&root, at(2026, 11, 23, 10, 0)).unwrap();
        store.write_snapshot("legs", &[row("X", "NO_SL", 75), row("Y", "PROTECTED", 150)]).unwrap();
        let r = store.query("SELECT status, qty, tags FROM read_parquet('{day}/legs.parquet') ORDER BY qty").unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0]["status"], "NO_SL");
        assert_eq!(r[1]["qty"], 150);
        assert_eq!(r[0]["tags"].as_array().unwrap().len(), 2);

        store.write_snapshot("legs", &[row("Z", "TOO_FAR", 1)]).unwrap();
        let r = store.query("SELECT symbol FROM read_parquet('{day}/legs.parquet')").unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0]["symbol"], "Z");

        store.write_snapshot::<Row>("legs", &[]).unwrap();
        assert!(!store.day_dir().join("legs.parquet").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn append_accumulates_parts() {
        let root = scratch("append");
        let store = DayStore::open(&root, at(2026, 11, 23, 10, 0)).unwrap();
        store.append("events", &[row("A", "NO_SL", 1)]).unwrap();
        store.append("events", &[row("B", "NO_SL", 2), row("C", "NO_SL", 3)]).unwrap();
        let r = store.query("SELECT count(*) AS n FROM read_parquet('{day}/events/*.parquet')").unwrap();
        assert_eq!(r[0]["n"], 3);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rollover_deletes_old_days_only() {
        let root = scratch("roll");
        let store = DayStore::open(&root, at(2026, 11, 23, 10, 0)).unwrap();
        store.write_snapshot("legs", &[row("X", "NO_SL", 1)]).unwrap();
        let old_day = store.day_dir();
        fs::create_dir_all(root.join("not-a-date")).unwrap();

        assert!(!store.rollover_if_needed(at(2026, 11, 23, 15, 0)).unwrap());
        assert!(old_day.exists());

        assert!(store.rollover_if_needed(at(2026, 11, 24, 8, 31)).unwrap());
        assert!(!old_day.exists(), "yesterday's folder must be deleted");
        assert!(store.day_dir().exists());
        assert!(root.join("not-a-date").exists(), "unrelated folders must be untouched");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn open_purges_stale_days_left_by_a_previous_run() {
        let root = scratch("stale");
        fs::create_dir_all(root.join("2026-11-01")).unwrap();
        let store = DayStore::open(&root, at(2026, 11, 23, 10, 0)).unwrap();
        assert!(!root.join("2026-11-01").exists());
        assert!(store.day_dir().exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_unsafe_table_names() {
        let root = scratch("names");
        let store = DayStore::open(&root, at(2026, 11, 23, 10, 0)).unwrap();
        assert!(store.write_snapshot("../evil", &[row("X", "A", 1)]).is_err());
        assert!(store.append("a'b", &[row("X", "A", 1)]).is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
