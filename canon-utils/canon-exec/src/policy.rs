use crate::exec::ExecutableEvent;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionRisk {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionPolicyDecision {
    Allow,
    Review,
    Forbid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionPolicyOutcome {
    pub decision: ExecutionPolicyDecision,
    pub risk: ExecutionRisk,
    pub reason: &'static str,
}

pub fn evaluate_execution_policy(event: &ExecutableEvent) -> ExecutionPolicyOutcome {
    match event {
        ExecutableEvent::Analysis(_) | ExecutableEvent::Llm(_) => {
            ExecutionPolicyOutcome { decision: ExecutionPolicyDecision::Allow, risk: ExecutionRisk::Low, reason: "non-mutating semantic/external reasoning step" }
        }
        ExecutableEvent::File(canon_event::FileEvent::Read(_)) => ExecutionPolicyOutcome { decision: ExecutionPolicyDecision::Allow, risk: ExecutionRisk::Low, reason: "read-only file access" },
        ExecutableEvent::File(canon_event::FileEvent::Write(_)) | ExecutableEvent::File(canon_event::FileEvent::Patch(_)) | ExecutableEvent::Edit(_) => {
            ExecutionPolicyOutcome { decision: ExecutionPolicyDecision::Review, risk: ExecutionRisk::Medium, reason: "workspace mutation requires policy review" }
        }
        ExecutableEvent::Cargo(canon_event::CargoEvent::Check(_)) => {
            ExecutionPolicyOutcome { decision: ExecutionPolicyDecision::Allow, risk: ExecutionRisk::Low, reason: "cargo check is validation-only" }
        }
        ExecutableEvent::Cargo(_) => ExecutionPolicyOutcome { decision: ExecutionPolicyDecision::Review, risk: ExecutionRisk::Medium, reason: "cargo build/run may write artifacts or execute code" },
        ExecutableEvent::Bash(bash) => {
            let cmd = bash.cmd.trim();
            let destructive = ["rm ", "git reset", "git clean", "chmod ", "chown ", "sudo "].iter().any(|pattern| cmd.contains(pattern));
            if destructive {
                ExecutionPolicyOutcome { decision: ExecutionPolicyDecision::Forbid, risk: ExecutionRisk::High, reason: "destructive shell command requires explicit higher-level approval" }
            } else if cmd.starts_with("cargo check") || cmd.starts_with("rg ") || cmd.starts_with("ls ") || cmd.starts_with("cat ") {
                ExecutionPolicyOutcome { decision: ExecutionPolicyDecision::Allow, risk: ExecutionRisk::Low, reason: "shell command is read-only or validation-oriented" }
            } else {
                ExecutionPolicyOutcome { decision: ExecutionPolicyDecision::Review, risk: ExecutionRisk::High, reason: "generic shell command requires policy review" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{evaluate_execution_policy, ExecutionPolicyDecision};
    use crate::exec::ExecutableEvent;

    #[test]
    fn cargo_check_is_allowed() {
        let event = ExecutableEvent::Cargo(canon_event::CargoEvent::Check(canon_event::CargoCheck { request_id: "req".into(), crate_name: "demo".into(), queued: false }));
        let outcome = evaluate_execution_policy(&event);
        assert_eq!(outcome.decision, ExecutionPolicyDecision::Allow);
    }

    #[test]
    fn destructive_bash_is_forbidden() {
        let event = ExecutableEvent::Bash(canon_event::BashInvoke { request_id: "req".into(), cmd: "rm -rf target".into(), cwd: Some("/tmp".into()), queued: false });
        let outcome = evaluate_execution_policy(&event);
        assert_eq!(outcome.decision, ExecutionPolicyDecision::Forbid);
    }
}
