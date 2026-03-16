# Running Canon

This guide shows you how to run the Canon system after the unification.

## Architecture Overview

The Canon system now consists of 5 unified crates:

```
canon-event-store → canon-event → canon-capability-engine → canon-kernel → canon-planner
```

## Available Binaries

### 1. canon-kernel (Main Runtime)

The main event runtime that processes events and executes capabilities.

**Location:** `target/debug/canon-kernel`

**Usage:**
```bash
./target/debug/canon-kernel --tlog <path-to-tlog-directory>
```

**Options:**
- `--tlog <path>` - Path to the TLOG event log directory (required)
- `--poll-ms <milliseconds>` - Polling interval (default: 500ms)
- `--once` - Process events once and exit (for testing)

**Environment Variables:**
- `CANON_EVENT_EXECUTION` - Enable capability execution (default: false)
  - Set to `1` or `true` to enable
- `CANON_EVENT_RUNTIME_START_AT_TAIL` - Start processing from end of log (default: false)
- `CANON_EVENT_RUNTIME_CURSOR` - Path to cursor file (default: `state/event_runtime.cursor.json`)
- `CANON_EVENT_RUNTIME_LOCK` - Path to lock file (default: `state/event_runtime.lock`)
- `CANON_REPORTS_TLOG` - Path to reports TLOG
- `CANON_REPORTS_OUT` - Path to reports output directory
- `CANON_VERIFY_TLOG_EQUIV` - Verify TLOG equivalence (set to `1`)

**Example:**
```bash
# Create directories
mkdir -p state/kernel_logs state/reports_out

# Run with capability execution enabled
CANON_EVENT_EXECUTION=1 \
CANON_REPORTS_TLOG=state/kernel_logs/kernel.tlog.d \
CANON_REPORTS_OUT=state/reports_out \
./target/debug/canon-kernel --tlog state/kernel_logs/kernel.tlog.d
```

### 2. canon-supervisor (Process Orchestration)

Watches for file changes and manages processes.

**Location:** `target/debug/canon-supervisor`

**Usage:**
```bash
./target/debug/canon-supervisor
```

**Configuration:** Uses `supervisor.toml` in the project root

**What it does:**
- Watches for file changes in configured directories
- Automatically restarts processes on changes
- Monitors event stream for capability events
- Emits supervisor events to TLOG

**Configuration Example (`supervisor.toml`):**
```toml
[watcher]
debounce_ms = 300
watch_dirs = ["canon-utils", "canon-agent-prompts"]

[[process]]
name = "canon-kernel"
bin  = "target/debug/canon-kernel"
args = ["--tlog", "/workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog.d"]
restart = "kill"
crate_name = "canon-kernel"
depends_on = ["canon-event", "canon-planner"]

[process.env]
CANON_REPORTS_TLOG = "/workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog.d"
CANON_REPORTS_OUT  = "/workspace/ai_sandbox/canon/state/reports_out"
```

## Quick Start

### Option 1: Run canon-kernel directly

```bash
# Build everything
cargo build --workspace

# Create state directories
mkdir -p state/kernel_logs state/reports_out

# Run the kernel
CANON_EVENT_EXECUTION=1 \
CANON_REPORTS_TLOG=state/kernel_logs/kernel.tlog.d \
CANON_REPORTS_OUT=state/reports_out \
./target/debug/canon-kernel --tlog state/kernel_logs/kernel.tlog.d
```

### Option 2: Run with supervisor (recommended for development)

```bash
# Build everything
cargo build --workspace

# Make sure supervisor.toml is configured (already done)

# Run the supervisor (it will manage canon-kernel)
./target/debug/canon-supervisor
```

The supervisor will:
1. Start `canon-kernel` automatically
2. Watch for code changes
3. Restart processes when dependencies change
4. Monitor the event stream

## Utility Binaries

### Analysis Tools

```bash
# Generate reports from TLOG
cargo run -p canon-analysis --bin reports_from_tlog -- <tlog-path>

# Aggregate workspace
cargo run -p canon-analysis --bin workspace_aggregate
```

### Editor Tools

```bash
# Test editor capabilities
cargo run -p canon-editor --bin editor_capability_smoke_test
```

### TLOG Tools

```bash
# Emit capability event
cargo run -p canon-tlog-writer --bin emit_capability_event

# Emit kernel event
cargo run -p canon-tlog-writer --bin emit_kernel_event

# Verify TLOG equivalence
cargo run -p canon-tlog-replay --bin verify_tlog_equivalence
```

### Kernel Tools

```bash
# Test capabilities
cargo run -p canon-kernel --bin capability_smoke_test

# Test LLM integration
cargo run -p canon-kernel --bin llm_smoke_test
```

## Development Workflow

### 1. Make code changes

Edit files in `canon-utils/`

### 2. Let supervisor handle rebuilds

If running with supervisor, it will detect changes and rebuild/restart automatically.

### 3. Manual rebuild and test

```bash
# Build specific crate
cargo build -p canon-kernel

# Run tests
cargo test -p canon-kernel

# Check entire workspace
cargo check --workspace
```

## Stopping the System

### If running canon-kernel directly:
Press `Ctrl+C`

### If running with supervisor:
Press `Ctrl+C` - supervisor will gracefully shutdown all managed processes

## Logs and State

- **TLOG events:** `state/kernel_logs/kernel.tlog.d/`
- **Reports:** `state/reports_out/`
- **Runtime cursor:** `state/event_runtime.cursor.json`
- **Runtime lock:** `state/event_runtime.lock`

## Troubleshooting

### "another instance is running"

The kernel uses a lock file to prevent multiple instances.

```bash
# Remove stale lock
rm state/event_runtime.lock
```

### Missing directories

Create required directories:
```bash
mkdir -p state/kernel_logs state/reports_out
```

### Capability execution not working

Make sure `CANON_EVENT_EXECUTION=1` is set:
```bash
export CANON_EVENT_EXECUTION=1
./target/debug/canon-kernel --tlog state/kernel_logs/kernel.tlog.d
```

## Architecture Components

### canon-event-store
Event persistence layer - handles TLOG reading/writing

### canon-event
Event schema and emission - defines RuntimeEvent types

### canon-capability-engine
Capability system - registry, executor, process orchestration

### canon-kernel
Runtime kernel - event loop, dispatch, capability execution
- `EventRuntime` - main runtime struct
- `EventBus` - event dispatching
- Consumers - agent, capability executor, event loop, LLM

### canon-planner
Planning and analysis - graph building, planning, scoring, SMT
- `graph` module - from canon-graph
- `planner` module - from canon-agent-v3
- `analysis` module - from canon-analysis

## Next Steps

- Emit events to TLOG
- Watch canon-kernel process them
- Capabilities execute and produce new events
- Planner analyzes state and generates plans
- System converges toward goals

The complete event-capability loop: **E_t → C → E_{t+1}**
