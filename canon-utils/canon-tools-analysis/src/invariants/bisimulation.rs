use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SharedEvent {
    RouteSelected,
    PlanningCompleted,
    ObserveCompleted,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BisimViolation {
    pub control_state: String,
    pub constraint_state: String,
    pub shared_event: SharedEvent,
    pub control_decision: String,
    pub constraint_decision: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BisimResult {
    pub ok: bool,
    pub violations: Vec<BisimViolation>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_traces_are_bisimilar() {
        let result = bisim_check(&[], &[]);
        assert!(result.ok);
        assert!(result.violations.is_empty());
    }
}

pub fn bisim_check(control: &[(String, SharedEvent, String)], constraint: &[(String, SharedEvent, String)]) -> BisimResult {
    let mut violations = Vec::new();

    for ((cs, ev_c, dec_c), (ks, ev_k, dec_k)) in control.iter().zip(constraint.iter()) {
        if ev_c != ev_k || dec_c != dec_k {
            violations.push(BisimViolation {
                control_state: cs.clone(),
                constraint_state: ks.clone(),
                shared_event: ev_c.clone(),
                control_decision: dec_c.clone(),
                constraint_decision: dec_k.clone(),
            });
        }
    }

    BisimResult { ok: violations.is_empty(), violations }
}
