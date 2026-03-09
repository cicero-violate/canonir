# git log -n 8
# bat -n /workspace/ai_sandbox/canon/PROJECT_OVERVIEW.md
# bat -n /workspace/ai_sandbox/canon/PROJECT_STATUS.md
# bat -n /workspace/ai_sandbox/canon/AGENT.md

bat -n /workspace/ai_sandbox/canon/canon-agent-prompts/AGENT_ACT.md
bat -n /workspace/ai_sandbox/canon/canon-agent-prompts/AGENT_BOOTSTRAP.md
bat -n /workspace/ai_sandbox/canon/canon-agent-prompts/agent_config.toml
bat -n /workspace/ai_sandbox/canon/canon-agent-prompts/AGENT_GOAL.md
bat -n /workspace/ai_sandbox/canon/canon-agent-prompts/AGENT_OBSERVE.md
bat -n /workspace/ai_sandbox/canon/canon-agent-prompts/AGENT_PLAN.md
bat -n /workspace/ai_sandbox/canon/canon-agent-prompts/AGENT_VERIFY.md


# bat -n /workspace/ai_sandbox/canon/ISSUE.md
bat -n /workspace/ai_sandbox/canon/GOAL.md
# bat -n /workspace/ai_sandbox/canon/PLAN.md
# bat -n /workspace/ai_sandbox/canon/EXECUTION_STATUS.md
# bat -n /workspace/ai_sandbox/canon/EXECUTION_REF.md
# bat -n /workspace/ai_sandbox/canon/AGENT_STATE.md

tree --gitignore -I 'chatgpt_rustsandbox'


bat -n canon-agent/src/pipelines/invariant/observe.rs
bat -n canon-agent/src/pipelines/invariant/config.rs
bat -n canon-agent/src/pipelines/invariant/mod.rs
bat -n canon-agent/src/pipelines/invariant/plan.rs
bat -n canon-agent/src/pipelines/invariant/act.rs
bat -n canon-agent/src/pipelines/invariant/score.rs
bat -n canon-agent-prompts/AGENT_BOOTSTRAP.md
bat -n canon-agent-prompts/AGENT_OBSERVE.md
bat -n canon-agent-prompts/AGENT_PLAN.md
bat -n canon-agent-prompts/AGENT_ACT.md
bat -n canon-agent-prompts/AGENT_VERIFY.md
bat -n canon-agent-prompts/agent_config.toml

bat -n canon-utils/upg_analysis/src/types.rs #` — Metadata struct definition (extractor side)
bat -n canon-utils/upg_analysis/src/emit.rs #` — writes metadata.json
bat -n canon-utils/analysis-engine/src/loader.rs #` — reads metadata.json, parse_edge_kind #1
bat -n canon-utils/analysis-engine/src/augment.rs #` — parse_edge_kind #2, edge_kind_str
bat -n canon-utils/analysis-engine/src/main.rs #` — engine entry point, schema validation goes here
bat -n analysis/metadata.json #` — current schema (no version field)
bat -n AGENT.md #` — invariant protocol, must be followed
