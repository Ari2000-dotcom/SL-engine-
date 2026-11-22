//! Shared in-memory state: latest snapshot and evaluation per account, incidents, risk settings.

use crate::accounts::Store;
use crate::auth::{AuthState, Role};
use crate::incidents::{Incident, IncidentBook};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sl_core::*;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::{broadcast, RwLock, Semaphore};

const MAX_CLOSED_IN_VIEW: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub id: String,
    pub name: String,
    pub group: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    pub account_id: String,
    pub client_id: String,
    pub broker: Broker,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HealthStatus {
    Connecting,
    Ok,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountHealth {
    #[serde(flatten)]
    pub info: AccountInfo,
    pub status: HealthStatus,
    pub last_ok: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub latency_ms: Option<u64>,
    pub polls: u64,
}

#[derive(Debug, Default)]
pub struct Inner {
    pub clients: Vec<ClientInfo>,
    pub health: Vec<AccountHealth>,
    pub snapshots: HashMap<String, Snapshot>,
    pub legs: HashMap<String, Vec<LegEval>>,
    pub incidents: IncidentBook,
    pub risk: RiskBook,
}

pub struct AppState {
    pub inner: RwLock<Inner>,
    pub notify: broadcast::Sender<()>,
    pub auth: AuthState,
    /// Broker accounts added at runtime, kept encrypted on disk.
    pub store: tokio::sync::Mutex<Store>,
    /// Caps concurrent password hashing so a login flood cannot starve the monitor.
    pub login_gate: Semaphore,
    /// username -> (recent failures, window start). Throttles guessing.
    pub login_fails: Mutex<HashMap<String, (u32, DateTime<Utc>)>>,
}

impl AppState {
    pub fn new(auth: AuthState, clients: Vec<ClientInfo>, accounts: Vec<AccountInfo>) -> Self {
        let health = accounts
            .into_iter()
            .map(|info| AccountHealth {
                info,
                status: HealthStatus::Connecting,
                last_ok: None,
                last_error: None,
                latency_ms: None,
                polls: 0,
            })
            .collect();
        let (notify, _) = broadcast::channel(16);
        AppState {
            inner: RwLock::new(Inner { clients, health, ..Default::default() }),
            notify,
            auth,
            store: tokio::sync::Mutex::new(Store::default()),
            login_gate: Semaphore::new(4),
            login_fails: Mutex::new(HashMap::new()),
        }
    }
}

impl Inner {
    /// Re-run the rule engine over an account's latest snapshot, and feed the incident tracker
    /// when the data is fresh. Returns true if anything the dashboard shows changed.
    pub fn reevaluate(&mut self, account_id: &str, now: DateTime<Utc>) -> bool {
        let Some(snap) = self.snapshots.get(account_id) else { return false };
        let fresh = snap.healthy && now - snap.fetched_at <= Duration::seconds(STALE_AFTER_SECS);
        let legs = evaluate(snap, &self.risk, now);
        let mut changed = !self.incidents.update_if(fresh, account_id, &legs, now).is_empty();
        let key = |l: &LegEval| (l.leg_key.clone(), l.status, l.ltp.map(|x| (x * 100.0) as i64), l.sl_qty);
        let old: Vec<_> = self.legs.get(account_id).map(|v| v.iter().map(key).collect()).unwrap_or_default();
        let new: Vec<_> = legs.iter().map(key).collect();
        changed |= old != new;
        self.legs.insert(account_id.to_string(), legs);
        changed
    }

    pub fn reevaluate_all(&mut self, now: DateTime<Utc>) -> bool {
        let ids: Vec<String> = self.snapshots.keys().cloned().collect();
        ids.iter().fold(false, |acc, id| self.reevaluate(id, now) | acc)
    }
}

impl IncidentBook {
    fn update_if(
        &mut self,
        fresh: bool,
        account_id: &str,
        legs: &[LegEval],
        now: DateTime<Utc>,
    ) -> Vec<crate::incidents::Change> {
        if fresh {
            self.update(account_id, legs, now)
        } else {
            Vec::new()
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ClientOverview {
    pub client_id: String,
    pub name: String,
    pub group: String,
    pub red: usize,
    pub amber: usize,
    pub green: usize,
    pub grey: usize,
    pub open_incidents: usize,
    pub accounts_failed: usize,
    pub accounts_total: usize,
}

#[derive(Debug, Serialize)]
pub struct IncidentView {
    #[serde(flatten)]
    pub incident: Incident,
    pub unprotected_secs: i64,
}

/// Everything about one account, fetched on demand when the desk opens it. Kept out of the live
/// push so the push stays small with hundreds of clients.
#[derive(Debug, Serialize)]
pub struct BookView {
    pub account_id: String,
    pub legs: Vec<LegEval>,
    pub orders: Vec<sl_core::Order>,
    pub trades: Vec<sl_core::Trade>,
}

const MAX_BOOK_ROWS: usize = 300;
pub const MAX_PAGE: usize = 500;

#[derive(Debug, Serialize)]
pub struct LegTotals {
    pub total: usize,
    pub red: usize,
    pub amber: usize,
    pub green: usize,
    pub grey: usize,
}

/// Server-side filter for the all-legs table, so the browser never holds thousands of rows.
#[derive(Debug, Default, serde::Deserialize)]
pub struct LegQuery {
    pub severity: Option<String>,
    pub client_id: Option<String>,
    pub account_id: Option<String>,
    pub broker: Option<String>,
    pub q: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct LegPage {
    pub total: usize,
    pub offset: usize,
    pub legs: Vec<LegEval>,
}

#[derive(Debug, Serialize)]
pub struct StateView {
    pub now: DateTime<Utc>,
    pub role: Role,
    pub client_id: Option<String>,
    pub overview: Vec<ClientOverview>,
    /// Only legs that need attention (red, amber, grey). Protected legs are counted in `totals`
    /// and fetched through `/api/legs` or `/api/accounts/:id/book`.
    pub legs: Vec<LegEval>,
    pub totals: LegTotals,
    pub incidents: Vec<IncidentView>,
    pub accounts: Vec<AccountHealth>,
    pub risk: Option<RiskBook>,
}

impl Inner {
    /// Everything a user is allowed to see. Clients only ever receive their own rows.
    pub fn view(&self, role: Role, client: Option<&str>, now: DateTime<Utc>) -> StateView {
        let visible = |cid: &str| role == Role::Admin || client == Some(cid);

        let mut legs: Vec<LegEval> =
            self.legs.values().flatten().filter(|l| visible(&l.client_id)).cloned().collect();
        let sev = |l: &LegEval| match l.severity {
            Severity::Red => 0,
            Severity::Amber => 1,
            Severity::Grey => 2,
            Severity::Green => 3,
        };
        legs.sort_by(|a, b| sev(a).cmp(&sev(b)).then(a.client_id.cmp(&b.client_id)).then(a.symbol.cmp(&b.symbol)));

        let mut incidents: Vec<IncidentView> = self
            .incidents
            .incidents
            .iter()
            .filter(|i| visible(&i.client_id))
            .map(|i| IncidentView { incident: i.clone(), unprotected_secs: i.unprotected_secs(now) })
            .collect();
        incidents.sort_by(|a, b| {
            b.incident.is_open().cmp(&a.incident.is_open()).then(b.incident.opened_at.cmp(&a.incident.opened_at))
        });
        // Open incidents always ship; only the most recent closed ones, so payloads stay small all day.
        let mut closed_seen = 0;
        incidents.retain(|i| {
            if i.incident.is_open() {
                return true;
            }
            closed_seen += 1;
            closed_seen <= MAX_CLOSED_IN_VIEW
        });

        let accounts: Vec<AccountHealth> =
            self.health.iter().filter(|h| visible(&h.info.client_id)).cloned().collect();

        let overview = self
            .clients
            .iter()
            .filter(|c| visible(&c.id))
            .map(|c| {
                let count = |s: Severity| legs.iter().filter(|l| l.client_id == c.id && l.severity == s).count();
                let accs = accounts.iter().filter(|a| a.info.client_id == c.id);
                ClientOverview {
                    client_id: c.id.clone(),
                    name: c.name.clone(),
                    group: c.group.clone(),
                    red: count(Severity::Red),
                    amber: count(Severity::Amber),
                    green: count(Severity::Green),
                    grey: count(Severity::Grey),
                    open_incidents: incidents
                        .iter()
                        .filter(|i| i.incident.client_id == c.id && i.incident.is_open())
                        .count(),
                    accounts_failed: accs.clone().filter(|a| a.status == HealthStatus::Failed).count(),
                    accounts_total: accs.count(),
                }
            })
            .collect();

        StateView {
            now,
            role,
            client_id: client.map(str::to_string),
            overview,
            legs: legs.iter().filter(|l| l.severity != Severity::Green).cloned().collect(),
            totals: LegTotals {
                total: legs.len(),
                red: legs.iter().filter(|l| l.severity == Severity::Red).count(),
                amber: legs.iter().filter(|l| l.severity == Severity::Amber).count(),
                green: legs.iter().filter(|l| l.severity == Severity::Green).count(),
                grey: legs.iter().filter(|l| l.severity == Severity::Grey).count(),
            },
            incidents,
            accounts,
            risk: (role == Role::Admin).then(|| self.risk.clone()),
        }
    }
}

fn sev_rank(l: &LegEval) -> u8 {
    match l.severity {
        Severity::Red => 0,
        Severity::Amber => 1,
        Severity::Grey => 2,
        Severity::Green => 3,
    }
}

impl Inner {
    fn can_see(role: Role, client: Option<&str>, cid: &str) -> bool {
        role == Role::Admin || client == Some(cid)
    }

    /// One account's legs, orders and trades. None when it does not exist or is not the caller's.
    pub fn account_book(&self, role: Role, client: Option<&str>, account_id: &str) -> Option<BookView> {
        let snap = self.snapshots.get(account_id);
        let owner = snap.map(|s| s.client_id.as_str()).or_else(|| {
            self.health.iter().find(|h| h.info.account_id == account_id).map(|h| h.info.client_id.as_str())
        })?;
        if !Self::can_see(role, client, owner) {
            return None;
        }
        let mut legs = self.legs.get(account_id).cloned().unwrap_or_default();
        legs.sort_by(|a, b| sev_rank(a).cmp(&sev_rank(b)).then(a.symbol.cmp(&b.symbol)));
        let (mut orders, mut trades) = snap.map(|s| (s.orders.clone(), s.trades.clone())).unwrap_or_default();
        orders.sort_by(|a, b| b.updated_ts.cmp(&a.updated_ts));
        orders.truncate(MAX_BOOK_ROWS);
        trades.sort_by(|a, b| b.fill_ts.cmp(&a.fill_ts));
        trades.truncate(MAX_BOOK_ROWS);
        Some(BookView { account_id: account_id.to_string(), legs, orders, trades })
    }

    /// Filtered, sorted, paged legs for the all-legs table.
    pub fn legs_page(&self, role: Role, client: Option<&str>, q: &LegQuery) -> LegPage {
        let needle = q.q.as_deref().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty());
        let names: HashMap<&str, String> = self.clients.iter().map(|c| (c.id.as_str(), c.name.to_lowercase())).collect();
        let sev = q.severity.as_deref().map(|s| s.to_ascii_uppercase());
        let broker = q.broker.as_deref().map(|s| s.to_ascii_uppercase());
        let mut rows: Vec<&LegEval> = self
            .legs
            .values()
            .flatten()
            .filter(|l| Self::can_see(role, client, &l.client_id))
            .filter(|l| q.client_id.as_deref().map_or(true, |c| l.client_id == c))
            .filter(|l| q.account_id.as_deref().map_or(true, |a| l.account_id == a))
            .filter(|l| {
                sev.as_deref().map_or(true, |s| {
                    let name = match l.severity {
                        Severity::Red => "RED",
                        Severity::Amber => "AMBER",
                        Severity::Green => "GREEN",
                        Severity::Grey => "GREY",
                    };
                    s == "ALL" || s == name
                })
            })
            .filter(|l| broker.as_deref().map_or(true, |b| b == "ALL" || format!("{:?}", l.broker).to_uppercase() == b))
            .filter(|l| {
                needle.as_deref().map_or(true, |n| {
                    l.symbol.to_lowercase().contains(n)
                        || names.get(l.client_id.as_str()).map_or(false, |c| c.contains(n))
                })
            })
            .collect();
        rows.sort_by(|a, b| {
            sev_rank(a).cmp(&sev_rank(b)).then(a.client_id.cmp(&b.client_id)).then(a.symbol.cmp(&b.symbol))
        });
        let total = rows.len();
        let offset = q.offset.unwrap_or(0).min(total);
        let limit = q.limit.unwrap_or(200).clamp(1, MAX_PAGE);
        LegPage { total, offset, legs: rows.into_iter().skip(offset).take(limit).cloned().collect() }
    }
}
