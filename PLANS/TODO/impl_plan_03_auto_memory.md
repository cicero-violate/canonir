# Implementation Plan 03 — Auto Memory System

## Goal

Give Canon's LLM agents persistent cross-restart memory. After each analyst session,
key findings are written to typed memory files. On the next invocation, the memory
index is injected into the system prompt so the analyst does not re-diagnose
already-identified root causes.

---

## New crate: `canon-utils/canon-memory`

### `Cargo.toml`

```toml
[package]
name = "canon-memory"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true, features = ["derive"] }
serde_json.workspace = true
anyhow.workspace = true
chrono.workspace = true
```

### `src/lib.rs`

```
pub mod memory;
pub mod index;
pub use memory::{MemoryEntry, MemoryType};
pub use index::{MemoryIndex, load_context_block};
```

### `src/memory.rs`

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    User,
    Feedback,
    Project,
    Reference,
}

pub struct MemoryEntry {
    pub name: String,
    pub description: String,
    pub memory_type: MemoryType,
    pub body: String,
}

impl MemoryEntry {
    /// Renders to the on-disk format:
    /// ---
    /// name: <name>
    /// description: <description>
    /// type: <type>
    /// ---
    /// <body>
    pub fn to_file_content(&self) -> String;

    /// Parses a file written by `to_file_content`.
    pub fn from_file_content(raw: &str) -> anyhow::Result<Self>;
}
```

### `src/index.rs`

The memory directory is `state/memory/` (configurable via env `CANON_MEMORY_DIR`,
default `/workspace/ai_sandbox/canon/state/memory`).

```rust
pub struct MemoryIndex {
    dir: PathBuf,
}

impl MemoryIndex {
    pub fn new(dir: PathBuf) -> Self;

    /// Writes `entry` to `{dir}/{slug}.md` where slug = entry.name with spaces→`_`.
    /// Updates `{dir}/MEMORY.md` index: appends a line
    /// `- [{name}]({slug}.md) — {description}` if not already present.
    pub fn save(&self, entry: &MemoryEntry) -> anyhow::Result<()>;

    /// Returns the first 200 lines of `{dir}/MEMORY.md`, or empty string if
    /// it doesn't exist.
    pub fn load_index_text(&self) -> String;

    /// Loads the full body of a specific memory file by slug.
    pub fn load_entry(&self, slug: &str) -> anyhow::Result<MemoryEntry>;

    /// Returns all entries whose description or body contains any of the keywords.
    pub fn search(&self, keywords: &[&str]) -> Vec<MemoryEntry>;
}

/// Builds a context block suitable for injection into a system prompt:
///
/// ## Memory
/// <MEMORY.md first 200 lines>
///
/// Returns empty string if no MEMORY.md exists.
pub fn load_context_block(dir: &Path) -> String;
```

---

## Modify: `canon-utils/canon-runtime/src/consumers/analyst_consumer.rs`

### Add dependency

In `canon-runtime/Cargo.toml`: add `canon-memory = { path = "../canon-memory" }`.

### Add `MemoryIndex` to `AnalystConsumer`

```rust
pub struct AnalystConsumer {
    tlog_path: PathBuf,
    memory: canon_memory::MemoryIndex,
    state: State,
}
```

Construct with `MemoryIndex::new(PathBuf::from(CANON_MEMORY_DIR))`.

### Inject memory into `start_session`

Before building `first_prompt`, call `canon_memory::load_context_block(&memory_dir)`.
If non-empty, prepend it to `first_prompt`:

```
{memory_context_block}

---

{SYSTEM_PROMPT}

{question}
```

This gives the analyst full prior-session context on every invocation.

### Write memory after `finish_session`

In `finish_session`, after `write_report(&report)`, extract structured memory from
the report:

1. Look for a line starting with `**Root cause**` — everything after the colon up
   to the next blank line becomes the `body` of a `MemoryType::Project` entry.
2. Save:
   ```
   MemoryEntry {
       name: format!("analyst_finding_{unix_ts}"),
       description: first 120 chars of root cause,
       memory_type: MemoryType::Project,
       body: format!("Root cause: {root_cause}\nTimestamp: {ts}\nReport: {report_path}"),
   }
   ```
3. Call `self.memory.save(&entry)`.

If no `**Root cause**` line is found, skip — the report was incomplete.

---

## Analyst system prompt amendment

After the memory injection block, add one line to `SYSTEM_PROMPT`:

```
If the Memory section above mentions a previously identified root cause, confirm
whether it still applies before spending turns re-deriving it.
```

---

## Verification

```
cargo check --workspace
```

Run the runtime until the analyst fires and produces a report. Confirm:
- `state/memory/MEMORY.md` is created
- `state/memory/analyst_finding_<ts>.md` contains root cause text
- On next analyst invocation, the memory block appears in the outgoing LLM prompt
  (visible in `state/event_log` Llm event)
