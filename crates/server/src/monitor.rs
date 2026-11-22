//! One supervised polling task per broker account, plus a ticker that keeps time-based statuses
//! current (grace expiring, data going stale) between polls.
//!
//! Every broker call has a timeout, and a panic in an account task is caught and restarted, so
//! one hung or crashing account can never leave the dashboard silently showing old data.

use crate::state::{AccountInfo, AppState, HealthStatus};
use chrono::{Duration as ChronoDuration, Utc};
use sl_brokers::{BrokerAdapter, BrokerError, InstrumentKey};
use sl_core::{Snapshot, STALE_AFTER_SECS};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;
use std::time::{Duration, Instant};

const CALL_TIMEOUT: Duration = Duration::from_secs(15);
/// Consecutive sign-in refusals before an account stops retrying on its own.
const MAX_AUTH_TRIES: u32 = 3;

async fn limited<T>(f: impl Future<Output = Result<T, BrokerError>>) -> Result<T, BrokerError> {
    match tokio::time::timeout(CALL_TIMEOUT, f).await {
        Ok(r) => r,
        Err(_) => Err(BrokerError::Http(format!("no reply within {}s", CALL_TIMEOUT.as_secs()))),
    }
}

/// Lets the admin API stop an account's poller (when the account is removed) or wake it early
/// (when a fresh token has just been added) instead of waiting out a back-off.
#[derive(Default)]
pub struct Control {
    pub stop: AtomicBool,
    pub wake: Notify,
}

impl Control {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
    /// Sleep for `d`, or less if woken or stopped.
    async fn nap(&self, d: Duration) {
        tokio::select! {
            _ = tokio::time::sleep(d) => {}
            _ = self.wake.notified() => {}
        }
    }
}

pub fn spawn_account(state: Arc<AppState>, info: AccountInfo, adapter: Arc<dyn BrokerAdapter>, poll: Duration, ctl: Arc<Control>) {
    tokio::spawn(async move {
        // Supervisor: if the worker ever panics, say so on the dashboard and start it again.
        loop {
            let worker = tokio::spawn(run_account(state.clone(), info.clone(), adapter.clone(), poll, ctl.clone()));
            match worker.await {
                Ok(()) => break, // the worker only returns when the account was removed
                Err(e) => {
                    tracing::error!(account = %info.account_id, error = %e, "account task crashed, restarting");
                    mark_failed(&state, &info.account_id, "Internal error, restarting".to_string()).await;
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
            if ctl.stopped() {
                break;
            }
        }
    });
}

async fn run_account(state: Arc<AppState>, info: AccountInfo, adapter: Arc<dyn BrokerAdapter>, poll: Duration, ctl: Arc<Control>) {
    let mut logged_in = false;
    let mut login_backoff = Duration::from_secs(5);
    let mut auth_fails = 0u32;
    loop {
        if ctl.stopped() {
            return;
        }
        if !logged_in {
            match limited(adapter.login()).await {
                Ok(()) => {
                    logged_in = true;
                    auth_fails = 0;
                    login_backoff = Duration::from_secs(5);
                    tracing::info!(account = %info.account_id, "logged in");
                }
                Err(e) => {
                    tracing::warn!(account = %info.account_id, error = %e, "login failed");
                    // A refusal (wrong password, MPIN, TOTP secret) will not fix itself, and brokers lock an
                    // account after a few wrong attempts. So after a few tries, stop and wait for the admin.
                    if matches!(e, BrokerError::Auth(_)) {
                        auth_fails += 1;
                    } else {
                        auth_fails = 0;
                    }
                    if auth_fails >= MAX_AUTH_TRIES {
                        mark_failed(&state, &info.account_id, format!("{e}. Stopped trying after {MAX_AUTH_TRIES} refusals so the broker does not lock the account. Fix the details, then press Retry sign-in")).await;
                        ctl.wake.notified().await; // parked until Retry sign-in, a new token, or removal
                        auth_fails = 0;
                        login_backoff = Duration::from_secs(5);
                        continue;
                    }
                    mark_failed(&state, &info.account_id, e.to_string()).await;
                    ctl.nap(login_backoff).await;
                    login_backoff = (login_backoff * 2).min(Duration::from_secs(120));
                    continue;
                }
            }
        }

        let started = Instant::now();
        match limited(adapter.fetch_books()).await {
            Ok(mut books) => {
                let missing: Vec<InstrumentKey> = books
                    .positions
                    .iter()
                    .filter(|p| p.net_qty != 0 && p.ltp.is_none())
                    .map(|p| InstrumentKey { exchange: p.exchange, symbol: p.symbol.clone() })
                    .collect();
                if !missing.is_empty() {
                    match limited(adapter.fetch_ltp(&missing)).await {
                        Ok(map) => {
                            for p in books.positions.iter_mut().filter(|p| p.ltp.is_none()) {
                                let k = InstrumentKey { exchange: p.exchange, symbol: p.symbol.clone() };
                                p.ltp = map.get(&k).copied();
                            }
                        }
                        Err(e) => tracing::warn!(account = %info.account_id, error = %e, "ltp fetch failed"),
                    }
                }
                let latency = started.elapsed().as_millis() as u64;
                let now = Utc::now();
                let snap = Snapshot {
                    account_id: info.account_id.clone(),
                    client_id: info.client_id.clone(),
                    broker: info.broker,
                    orders: books.orders,
                    trades: books.trades,
                    positions: books.positions,
                    fetched_at: now,
                    healthy: true,
                };
                let mut inner = state.inner.write().await;
                if ctl.stopped() || !inner.health.iter().any(|h| h.info.account_id == info.account_id) {
                    return;
                }
                inner.snapshots.insert(info.account_id.clone(), snap);
                if let Some(h) = inner.health.iter_mut().find(|h| h.info.account_id == info.account_id) {
                    h.status = HealthStatus::Ok;
                    h.last_ok = Some(now);
                    h.last_error = None;
                    h.latency_ms = Some(latency);
                    h.polls += 1;
                }
                inner.reevaluate(&info.account_id, now);
                drop(inner);
                let _ = state.notify.send(());
            }
            Err(e) => {
                tracing::warn!(account = %info.account_id, error = %e, "fetch failed");
                if e.needs_relogin() {
                    logged_in = false;
                }
                mark_failed(&state, &info.account_id, e.to_string()).await;
            }
        }
        ctl.nap(poll).await;
    }
}

/// A failed poll leaves the last snapshot in place but marks it unhealthy, so every leg turns
/// grey instead of staying green on old data.
async fn mark_failed(state: &AppState, account_id: &str, error: String) {
    let now = Utc::now();
    let mut inner = state.inner.write().await;
    if let Some(h) = inner.health.iter_mut().find(|h| h.info.account_id == account_id) {
        h.status = HealthStatus::Failed;
        h.last_error = Some(error);
    }
    if let Some(s) = inner.snapshots.get_mut(account_id) {
        s.healthy = false;
    }
    inner.reevaluate(account_id, now);
    drop(inner);
    let _ = state.notify.send(());
}

/// Re-evaluates everything every two seconds so grace periods end and stale data is noticed
/// even when no poll arrives. It also flags an account as failed when its last good read is too
/// old, which catches a task that is stuck rather than erroring.
pub fn spawn_ticker(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        loop {
            tick.tick().await;
            let now = Utc::now();
            let mut inner = state.inner.write().await;
            let mut changed = false;
            let stale_after = ChronoDuration::seconds(STALE_AFTER_SECS);
            let overdue: Vec<String> = inner
                .health
                .iter()
                .filter(|h| h.status == HealthStatus::Ok && h.last_ok.is_some_and(|t| now - t > stale_after))
                .map(|h| h.info.account_id.clone())
                .collect();
            for id in overdue {
                if let Some(h) = inner.health.iter_mut().find(|h| h.info.account_id == id) {
                    h.status = HealthStatus::Failed;
                    h.last_error = Some(format!("No data for more than {STALE_AFTER_SECS}s"));
                    changed = true;
                }
            }
            changed |= inner.reevaluate_all(now);
            drop(inner);
            if changed {
                let _ = state.notify.send(());
            }
        }
    });
}
