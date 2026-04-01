#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConstraintTier {
    Deterministic,
    Discovered,
    Meta,
}

#[derive(Clone, Debug)]
pub struct ConstraintRef {
    pub fingerprint: u64,
    pub tier: ConstraintTier,
    pub support: usize,
}

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct ConflictRecord {
    pub c1: u64,
    pub c2: u64,
    pub action: String,
    pub resolution: String,
}

pub fn resolve_conflict(c1: &ConstraintRef, c2: &ConstraintRef, action: &str) -> (u64, ConflictRecord) {
    let winner = if c1.tier != c2.tier {
        if c1.tier > c2.tier {
            c1
        } else {
            c2
        }
    } else {
        if c1.support >= c2.support {
            c1
        } else {
            c2
        }
    };

    let loser = if winner.fingerprint == c1.fingerprint { c2 } else { c1 };

    let record = ConflictRecord { c1: c1.fingerprint, c2: c2.fingerprint, action: action.to_string(), resolution: format!("winner={} loser={}", winner.fingerprint, loser.fingerprint) };

    (winner.fingerprint, record)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_beats_discovered_on_conflict() {
        let meta = ConstraintRef { fingerprint: 1, tier: ConstraintTier::Meta, support: 100 };
        let disc = ConstraintRef { fingerprint: 2, tier: ConstraintTier::Discovered, support: 1000 };
        let (winner_fp, _record) = resolve_conflict(&meta, &disc, "test_rule");
        assert_eq!(winner_fp, 1);
    }
}
