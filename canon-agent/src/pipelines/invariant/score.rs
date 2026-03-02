//! Score phase — reward from exit-check result.

/// R = +1.0 if exit check passed, -1.0 if act failed, 0.0 otherwise.
pub fn compute_reward(exit_ok: bool, act_failed: bool) -> f64 {
    if act_failed {
        return -1.0;
    }
    if exit_ok {
        1.0
    } else {
        0.0
    }
}
