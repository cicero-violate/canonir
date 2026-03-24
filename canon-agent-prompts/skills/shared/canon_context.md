## Canon Runtime Context

Canon is a multi-agent Rust runtime. The event bus dispatches RuntimeEvent variants to consumer threads. Consumers return EventOutcome (Emit/EmitMany/NoOp/Error). The capability pipeline processes LlmCall, Cargo, File, and Bash events.

Working directory: /workspace/ai_sandbox/canon
Tlog: $CANON_REPORTS_TLOG
Reports output: $CANON_REPORTS_OUT
