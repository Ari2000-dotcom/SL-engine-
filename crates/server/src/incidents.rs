//! Incident tracking: a leg that turns red or amber opens an incident; it resolves when the leg is
//! protected again or the position closes. Pure logic, no I/O.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sl_core::{LegEval, LegStatus, Severity};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Resolution {
    SlPlaced,
    PositionClosed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    pub id: u64,
    pub leg_key: String,
    pub account_id: String,
    pub client_id: String,
    pub symbol: String,
    pub first_status: LegStatus,
    pub worst_status: LegStatus,
    pub current_status: LegStatus,
    pub detail: Option<String>,
    pub opened_at: DateTime<Utc>,
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub acknowledged_by: Option<String>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolution: Option<Resolution>,
    pub history: Vec<(DateTime<Utc>, LegStatus)>,
}

impl Incident {
    pub fn is_open(&self) -> bool {
        self.resolved_at.is_none()
    }

    pub fn unprotected_secs(&self, now: DateTime<Utc>) -> i64 {
        (self.resolved_at.unwrap_or(now) - self.opened_at).num_seconds().max(0)
    }
}

/// Statuses that mean "the client should act".
pub fn opens_incident(s: LegStatus) -> bool {
    matches!(s.severity(), Severity::Red | Severity::Amber) && s != LegStatus::Unverified
}

fn rank(s: LegStatus) -> u8 {
    match s.severity() {
        Severity::Red => 2,
        Severity::Amber => 1,
        _ => 0,
    }
}

#[derive(Debug, Default)]
pub struct IncidentBook {
    pub incidents: Vec<Incident>,
    next_id: u64,
    /// Consecutive updates in which an open incident's leg was absent. One empty broker reply
    /// must not close real incidents, so closing needs `MISSES_TO_CLOSE` in a row.
    missed: HashMap<String, u8>,
}

const MISSES_TO_CLOSE: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Opened(u64),
    Escalated(u64),
    Resolved(u64),
}

impl IncidentBook {
    /// Apply one account's fresh evaluation. Call only with healthy data, otherwise a broker
    /// glitch would resolve real incidents.
    pub fn update(&mut self, account_id: &str, evals: &[LegEval], now: DateTime<Utc>) -> Vec<Change> {
        let mut changes = Vec::new();
        let seen: HashSet<&str> = evals.iter().map(|e| e.leg_key.as_str()).collect();

        for e in evals {
            let open_idx = self.incidents.iter().position(|i| i.is_open() && i.leg_key == e.leg_key);
            if opens_incident(e.status) {
                match open_idx {
                    None => {
                        self.next_id += 1;
                        let id = self.next_id;
                        self.incidents.push(Incident {
                            id,
                            leg_key: e.leg_key.clone(),
                            account_id: e.account_id.clone(),
                            client_id: e.client_id.clone(),
                            symbol: e.symbol.clone(),
                            first_status: e.status,
                            worst_status: e.status,
                            current_status: e.status,
                            detail: e.detail.clone(),
                            opened_at: now,
                            acknowledged_at: None,
                            acknowledged_by: None,
                            resolved_at: None,
                            resolution: None,
                            history: vec![(now, e.status)],
                        });
                        changes.push(Change::Opened(id));
                    }
                    Some(i) => {
                        let inc = &mut self.incidents[i];
                        inc.detail = e.detail.clone();
                        if inc.current_status != e.status {
                            inc.current_status = e.status;
                            inc.history.push((now, e.status));
                            if rank(e.status) > rank(inc.worst_status) {
                                inc.worst_status = e.status;
                                // A worse status deserves a fresh alert.
                                inc.acknowledged_at = None;
                                inc.acknowledged_by = None;
                                changes.push(Change::Escalated(inc.id));
                            }
                        }
                    }
                }
            } else if e.status == LegStatus::Protected {
                if let Some(i) = open_idx {
                    let inc = &mut self.incidents[i];
                    inc.resolved_at = Some(now);
                    inc.resolution = Some(Resolution::SlPlaced);
                    inc.current_status = LegStatus::Protected;
                    inc.history.push((now, LegStatus::Protected));
                    changes.push(Change::Resolved(inc.id));
                }
            }
        }

        // Positions that stay absent for two updates in a row are closed.
        for inc in self.incidents.iter_mut() {
            if !inc.is_open() || inc.account_id != account_id {
                continue;
            }
            if seen.contains(inc.leg_key.as_str()) {
                self.missed.remove(&inc.leg_key);
                continue;
            }
            let n = self.missed.entry(inc.leg_key.clone()).or_insert(0);
            *n += 1;
            if *n >= MISSES_TO_CLOSE {
                inc.resolved_at = Some(now);
                inc.resolution = Some(Resolution::PositionClosed);
                changes.push(Change::Resolved(inc.id));
                self.missed.remove(&inc.leg_key);
            }
        }
        changes
    }

    pub fn acknowledge(&mut self, id: u64, by: &str, now: DateTime<Utc>) -> bool {
        match self.incidents.iter_mut().find(|i| i.id == id && i.is_open()) {
            Some(i) => {
                i.acknowledged_at = Some(now);
                i.acknowledged_by = Some(by.to_string());
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use sl_core::*;

    fn eval(key: &str, status: LegStatus) -> LegEval {
        LegEval {
            leg_key: key.into(),
            account_id: "A".into(),
            client_id: "C".into(),
            broker: Broker::Zerodha,
            exchange: Exchange::Nfo,
            segment: Segment::Opt,
            symbol: key.into(),
            product: Product::Carryforward,
            net_qty: -75,
            avg_price: 100.0,
            ltp: Some(90.0),
            sl_order_ids: vec![],
            sl_qty: 0,
            sl_trigger: None,
            risk_mode: RiskMode::Pct,
            limit_value: 40.0,
            dist_pct: None,
            risk_rupee: None,
            status,
            severity: status.severity(),
            detail: None,
            opened_ts: None,
            evaluated_at: Utc::now(),
        }
    }

    #[test]
    fn opens_once_then_resolves_when_protected() {
        let mut b = IncidentBook::default();
        let t = Utc::now();
        assert_eq!(b.update("A", &[eval("L1", LegStatus::NoSl)], t), vec![Change::Opened(1)]);
        assert!(b.update("A", &[eval("L1", LegStatus::NoSl)], t).is_empty());
        let c = b.update("A", &[eval("L1", LegStatus::Protected)], t + Duration::seconds(30));
        assert_eq!(c, vec![Change::Resolved(1)]);
        assert_eq!(b.incidents[0].resolution, Some(Resolution::SlPlaced));
        assert_eq!(b.incidents[0].unprotected_secs(t + Duration::seconds(99)), 30);
    }

    #[test]
    fn grace_stale_and_unverified_do_not_open() {
        let mut b = IncidentBook::default();
        for s in [LegStatus::Grace, LegStatus::StaleData, LegStatus::Unverified, LegStatus::Protected] {
            assert!(b.update("A", &[eval("L1", s)], Utc::now()).is_empty());
        }
        assert!(b.incidents.is_empty());
    }

    #[test]
    fn stale_does_not_resolve_an_open_incident() {
        let mut b = IncidentBook::default();
        b.update("A", &[eval("L1", LegStatus::NoSl)], Utc::now());
        b.update("A", &[eval("L1", LegStatus::StaleData)], Utc::now());
        assert!(b.incidents[0].is_open());
    }

    #[test]
    fn escalation_clears_acknowledgement() {
        let mut b = IncidentBook::default();
        let t = Utc::now();
        b.update("A", &[eval("L1", LegStatus::PartialSl)], t);
        assert!(b.acknowledge(1, "admin", t));
        let c = b.update("A", &[eval("L1", LegStatus::NoSl)], t);
        assert_eq!(c, vec![Change::Escalated(1)]);
        assert!(b.incidents[0].acknowledged_at.is_none());
        assert_eq!(b.incidents[0].worst_status, LegStatus::NoSl);
    }

    #[test]
    fn closed_position_resolves_only_its_own_account() {
        let mut b = IncidentBook::default();
        b.update("A", &[eval("L1", LegStatus::NoSl)], Utc::now());
        let mut other = eval("L9", LegStatus::NoSl);
        other.account_id = "B".into();
        b.update("B", &[other], Utc::now());
        // The first absence is not enough, a second consecutive one closes it.
        assert!(b.update("A", &[], Utc::now()).is_empty());
        let c = b.update("A", &[], Utc::now());
        assert_eq!(c, vec![Change::Resolved(1)]);
        assert_eq!(b.incidents[0].resolution, Some(Resolution::PositionClosed));
        assert!(b.incidents[1].is_open());
    }

    #[test]
    fn one_empty_reply_does_not_close_incidents() {
        let mut b = IncidentBook::default();
        b.update("A", &[eval("L1", LegStatus::NoSl)], Utc::now());
        assert!(b.update("A", &[], Utc::now()).is_empty());
        // The leg comes back, so the counter resets and a later single gap is harmless again.
        b.update("A", &[eval("L1", LegStatus::NoSl)], Utc::now());
        assert!(b.update("A", &[], Utc::now()).is_empty());
        assert!(b.incidents[0].is_open());
    }

    #[test]
    fn reopens_as_a_new_incident_after_resolution() {
        let mut b = IncidentBook::default();
        b.update("A", &[eval("L1", LegStatus::NoSl)], Utc::now());
        b.update("A", &[eval("L1", LegStatus::Protected)], Utc::now());
        let c = b.update("A", &[eval("L1", LegStatus::SlCancelled)], Utc::now());
        assert_eq!(c, vec![Change::Opened(2)]);
    }
}
