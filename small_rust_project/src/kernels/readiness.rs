use crate::dag::Status;

/// Pure readiness kernel.
///
/// Determines if a node should transition to Ready based on
/// the status of its dependencies.
pub fn compute_ready(dep_statuses: &[Status]) -> Status {
    let mut any_failed = false;
    let mut all_completed = true;

    for s in dep_statuses {
        if *s == Status::Failed {
            any_failed = true;
        }
        if *s != Status::Completed {
            all_completed = false;
        }
    }

    if any_failed {
        Status::Blocked
    } else if all_completed {
        Status::Ready
    } else {
        Status::Pending
    }
}
