# Implementation Plan 05 — Skill Registry

## Goal

Move all hardcoded agent system prompts out of Rust source into named skill files
under `canon-agent-prompts/skills/`. Skills are loaded at runtime, hot-reloadable,
composable (a skill can `@include` another skill), and versionable in git.
No recompile needed when a prompt changes.

---

## Directory structure

```
canon-agent-prompts/
  skills/
    analyst/
      full_analysis.md       ← current SYSTEM_PROMPT from analyst_consumer.rs
      compaction.md          ← compaction summary prompt (plan 04)
    goal_gen/
      generate_goal.md       ← current GOAL_GEN_PROMPT from goal_gen_consumer.rs
    planner/
      loop_plan.md           ← planner system prompt
    router/
      route_select.md        ← router system prompt
    shared/
      canon_context.md       ← injected into all skills (project overview)
```

---

## New crate: `canon-utils/canon-skills`

### `Cargo.toml`

```toml
[package]
name = "canon-skills"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow.workspace = true
once_cell.workspace = true
parking_lot.workspace = true
```

### Skill file format

A skill file is plain markdown. Optional YAML frontmatter (between `---` markers):

```yaml
---
name: full_analysis
description: 8-phase analyst system prompt
effort: high
includes:
  - shared/canon_context
---
```

`includes` lists other skill names to prepend (recursively, deduplicated, cycle-safe).

### `src/lib.rs`

```rust
pub struct SkillRegistry {
    skills_dir: PathBuf,
    cache: parking_lot::RwLock<HashMap<String, Arc<Skill>>>,
}

pub struct Skill {
    pub name: String,
    pub description: String,
    pub effort: Option<LlmEffort>,
    /// Fully resolved prompt text (includes expanded).
    pub prompt: String,
}

impl SkillRegistry {
    /// Constructs a registry pointing at `skills_dir`.
    pub fn new(skills_dir: PathBuf) -> Self;

    /// Loads skill by path relative to skills_dir (e.g. "analyst/full_analysis").
    /// Result is cached. Call `invalidate` to force reload.
    pub fn load(&self, skill_path: &str) -> anyhow::Result<Arc<Skill>>;

    /// Clears the cache entry for `skill_path`, forcing next `load` to re-read disk.
    pub fn invalidate(&self, skill_path: &str);

    /// Clears the entire cache.
    pub fn invalidate_all(&self);
}

/// Global singleton registry pointing at the default skills dir.
/// Initialized on first access from env `CANON_SKILLS_DIR`, falling back to
/// `/workspace/ai_sandbox/canon/canon-agent-prompts/skills`.
pub fn global_registry() -> &'static SkillRegistry;
```

### `@include` resolution

In `load()`, after reading the raw file:
1. Parse frontmatter YAML.
2. For each name in `includes`, recursively call `load()` and collect resolved prompts.
3. Concatenate: `{included_prompts_joined_by_blank_line}\n\n{body_without_frontmatter}`.
4. Store in `Skill::prompt`.

---

## Migrate prompts

### Move `SYSTEM_PROMPT` from `analyst_consumer.rs`

1. Create `canon-agent-prompts/skills/analyst/full_analysis.md`.
2. Copy the full contents of `SYSTEM_PROMPT` const as the file body.
3. Add frontmatter: `effort: high`, `includes: [shared/canon_context]`.
4. In `analyst_consumer.rs`:
   - Remove `const SYSTEM_PROMPT`.
   - In `start_session`: replace `SYSTEM_PROMPT` with
     `canon_skills::global_registry().load("analyst/full_analysis")?.prompt`.
   - If load fails, log error and return `EventOutcome::NoOp("analyst_skill_load_failed")`.

### Move `GOAL_GEN_PROMPT` from `goal_gen_consumer.rs`

1. Create `canon-agent-prompts/skills/goal_gen/generate_goal.md`.
2. Copy `GOAL_GEN_PROMPT` as body.
3. In `goal_gen_consumer.rs`:
   - Remove `const GOAL_GEN_PROMPT`.
   - Load from registry on first `Tick` transition to `Pending`.

---

## Add `canon-skills` dependency

In `canon-runtime/Cargo.toml`:
```toml
canon-skills = { path = "../canon-skills" }
```

---

## `shared/canon_context.md`

Create this file with:

```markdown
## Canon Runtime Context

Canon is a multi-agent Rust runtime. The event bus dispatches `RuntimeEvent`
variants to consumer threads. Consumers return `EventOutcome` (Emit/EmitMany/NoOp/Error).
The capability pipeline processes LlmCall, Cargo, File, and Bash events.
The main loop stages: LoopObserved → LoopPlanned → LoopActed → LoopVerified → LoopRewarded.

Working directory: /workspace/ai_sandbox/canon
Tlog: $CANON_REPORTS_TLOG
Reports output: $CANON_REPORTS_OUT
```

All skills that include `shared/canon_context` get this injected automatically.

---

## Hot-reload on SIGHUP

In `event_runtime.rs`, add a signal handler for `SIGHUP` (use `signal_hook` crate,
already a dependency of `canon-runtime-supervisor`):

```rust
// When SIGHUP received:
canon_skills::global_registry().invalidate_all();
eprintln!("[event_runtime] skill cache invalidated");
```

Next LLM call re-reads prompts from disk.

---

## Verification

```
cargo check --workspace
```

1. Edit `canon-agent-prompts/skills/analyst/full_analysis.md` — add a test phrase.
2. Send SIGHUP to the event runtime process.
3. Trigger an analyst invocation.
4. Confirm the test phrase appears in the outgoing `Llm` event prompt in the tlog.
