//! Risk limits the admin configures. Resolution order:
//! client+segment, client+ALL, global+segment, global+ALL, then built-in defaults.

use crate::model::Segment;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskMode {
    /// Maximum loss distance as % of average price.
    Pct,
    /// Maximum loss per leg in rupees.
    Rupee,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RiskSetting {
    pub mode: RiskMode,
    pub max_pct: f64,
    pub max_rupee: f64,
    pub grace_sec: i64,
}

impl Default for RiskSetting {
    fn default() -> Self {
        RiskSetting {
            mode: RiskMode::Pct,
            max_pct: 40.0,
            max_rupee: 25_000.0,
            grace_sec: 15,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskRule {
    /// None = global.
    pub client_id: Option<String>,
    /// None = all segments.
    pub segment: Option<Segment>,
    pub setting: RiskSetting,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RiskBook {
    pub rules: Vec<RiskRule>,
}

impl RiskBook {
    pub fn resolve(&self, client_id: &str, segment: Segment) -> RiskSetting {
        let find = |client: Option<&str>, seg: Option<Segment>| {
            self.rules
                .iter()
                .find(|r| r.client_id.as_deref() == client && r.segment == seg)
                .map(|r| r.setting)
        };
        find(Some(client_id), Some(segment))
            .or_else(|| find(Some(client_id), None))
            .or_else(|| find(None, Some(segment)))
            .or_else(|| find(None, None))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(client: Option<&str>, seg: Option<Segment>, pct: f64) -> RiskRule {
        RiskRule {
            client_id: client.map(str::to_string),
            segment: seg,
            setting: RiskSetting { max_pct: pct, ..RiskSetting::default() },
        }
    }

    #[test]
    fn most_specific_rule_wins() {
        let book = RiskBook {
            rules: vec![
                rule(None, None, 1.0),
                rule(None, Some(Segment::Opt), 2.0),
                rule(Some("c1"), None, 3.0),
                rule(Some("c1"), Some(Segment::Opt), 4.0),
            ],
        };
        assert_eq!(book.resolve("c1", Segment::Opt).max_pct, 4.0);
        assert_eq!(book.resolve("c1", Segment::Fut).max_pct, 3.0);
        assert_eq!(book.resolve("c2", Segment::Opt).max_pct, 2.0);
        assert_eq!(book.resolve("c2", Segment::Eq).max_pct, 1.0);
    }

    #[test]
    fn falls_back_to_defaults() {
        assert_eq!(RiskBook::default().resolve("x", Segment::Eq), RiskSetting::default());
    }
}
