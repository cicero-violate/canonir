# Repair Plan: Bash Execution Must Not Block the Event Loop

## Status

`CapabilityExecutor::on_event` calls `BashInvoke::execute()` synchronously on the
event loop thread. `execute()` calls `Command::output()?` which blocks until the
subprocess exits. Long-running commands (`cargo check`, `cargo build`, `cargo init`)
freeze the entire event loop for their duration — no `Tick`, no watchdog, no
`LoopActed` fires until the process exits.

The LLM executor already uses the correct pattern: a static worker thread receives
work via an `mpsc::Sender`, executes the blocking call, and emits the result back
via `EventEmitterHandle`. `LlmCall::execute()` sends to the channel and immediately
returns `ExecutionResult::Deferred`. The bus sees `Deferred` and moves on.

**Fix:** Apply the same worker-thread pattern to `BashInvoke`.

---

## Fix — `canon-utils/canon-exec/src/exec/bash.rs`

### Step 1 — Add a static worker channel (mirrors `LLM_WORKER_TX`)

```rust
struct BashWork {
    request_id: String,
    cmd: String,
    cwd: String,
    emitter: EventEmitterHandle,
}

static BASH_WORKER_TX: std::sync::RwLock<Option<std::sync::mpsc::Sender<BashWork>>> =
    std::sync::RwLock::new(None);
```

### Step 2 — Add `init_bash_worker` / `shutdown_bash_worker`

```rust
pub fn init_bash_worker() {
    let (tx, rx) = std::sync::mpsc::channel::<BashWork>();
    *BASH_WORKER_TX.write().unwrap() = Some(tx);

    std::thread::Builder::new()
        .name("bash_executor_worker".to_string())
        .spawn(move || {
            for BashWork { request_id, cmd, cwd, emitter } in rx {
                std::fs::create_dir_all(&cwd).ok();
                let result = Command::new("bash")
                    .arg("-lc")
                    .arg(&cmd)
                    .current_dir(&cwd)
                    .output();
                match result {
                    Ok(output) => {
                        emitter.emit(RuntimeEvent::CapabilityCompleted(CapabilityCompleted {
                            request_id,
                            capability: "bash",
                            result: CapabilityResult::Process(ProcessResult {
                                status: output.status.code().unwrap_or(-1),
                                success: output.status.success(),
                                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                            }),
                        }));
                    }
                    Err(err) => {
                        emitter.emit(RuntimeEvent::CapabilityFailed(canon_event::CapabilityFailed {
                            request_id,
                            capability: "bash",
                            error: err.to_string(),
                        }));
                    }
                }
            }
        })
        .expect("bash worker thread spawn failed");
}

pub fn shutdown_bash_worker() {
    *BASH_WORKER_TX.write().unwrap() = None;
}
```

### Step 3 — Replace `BashInvoke::execute` with non-blocking send

Replace the current blocking `execute`:

```rust
impl Executable for BashInvoke {
    fn execute(self, ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        let cwd = self.cwd.clone().unwrap_or_else(|| ".".to_string());
        std::fs::create_dir_all(&cwd).ok();
        let output = Command::new("bash").arg("-lc").arg(&self.cmd).current_dir(&cwd).output()?;
        Ok(ExecutionResult::Emit(RuntimeEvent::CapabilityCompleted(...)))
    }
}
```

With:

```rust
impl Executable for BashInvoke {
    fn execute(self, ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        let cwd = self.cwd.unwrap_or_else(|| ".".to_string());
        let guard = BASH_WORKER_TX.read().unwrap();
        if let Some(tx) = guard.as_ref() {
            tx.send(BashWork {
                request_id: self.request_id,
                cmd: self.cmd,
                cwd,
                emitter: ctx.emitter,
            }).ok();
            Ok(ExecutionResult::Deferred)
        } else {
            // Worker not initialized — fall back to inline execution so behavior
            // degrades gracefully rather than silently dropping the command.
            std::fs::create_dir_all(&cwd).ok();
            let output = Command::new("bash").arg("-lc").arg(&self.cmd).current_dir(&cwd).output()?;
            Ok(ExecutionResult::Emit(RuntimeEvent::CapabilityCompleted(CapabilityCompleted {
                request_id: self.request_id,
                capability: "bash",
                result: CapabilityResult::Process(ProcessResult {
                    status: output.status.code().unwrap_or(-1),
                    success: output.status.success(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                }),
            })))
        }
    }
}
```

---

## Fix — `canon-utils/canon-exec/src/lib.rs`

Export the new functions alongside `init_llm_worker`:

```rust
pub use exec::bash::{init_bash_worker, shutdown_bash_worker};
```

---

## Fix — `canon-utils/canon-runtime/src/bin/event_runtime.rs`

Add next to `init_llm_worker()` (line 326):

```rust
canon_exec::init_bash_worker();
```

And in the shutdown path next to `shutdown_llm_worker()`:

```rust
canon_exec::shutdown_bash_worker();
```

---

## Verification

```
cargo check -p canon-exec
cargo check -p canon-runtime
```

Then run the runtime and confirm in the tlog:

1. After `RouteSelected("act")`, `Tick` events continue to fire every second —
   confirming the event loop is not blocked.
2. `CapabilityCompleted` for `cargo check` arrives after the compilation finishes
   (1–5 minutes), not before.
3. `LoopActed` fires after `CapabilityCompleted`, not before.
4. Multiple sequential `run_command` actions in a plan batch all complete and emit
   `LoopActed` in order.
5. No heartbeat gap longer than 2 seconds occurs during a `cargo check` run.
