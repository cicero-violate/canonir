/// Persistent repair job server — allows harness_suite (and other callers) to
/// submit repair jobs to a long-running canon-harness-repair daemon instead of
/// spawning a new subprocess for every failing test.
///
/// Architecture
/// ============
///
///   canon-repair-daemon process  (started once, alongside the main loop)
///   ├── relay_client_call(9101) → supervisor WsBridge → ChatGPT tab
///   └── RepairJobServer          (listens on 127.0.0.1:9102)
///                ↑ TCP JSON job request
///   harness_suite  │
///   └── repair_client_submit()──┘  (sends job, blocks until done)
///
/// The daemon processes one job at a time (the ChatGPT tab is stateful and
/// sequential). The conversation accumulates across jobs — the repair agent
/// builds context about what it has already tried.

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Default address the repair job server binds to.
pub const REPAIR_SERVER_ADDR: &str = "127.0.0.1:9102";

/// A repair job submitted by the harness suite.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepairJobRequest {
    /// Name of the crate containing the failing test.
    pub crate_name: String,
    /// Name of the failing test.
    pub test_name: String,
    /// Combined stdout/stderr from `cargo test`, used as failure context.
    pub failure_output: String,
    /// Optional structured incident context (e.g. from eventlog analysis).
    pub incident_context: Option<String>,
    /// Maximum repair steps the agent may take.
    pub max_steps: usize,
    /// Absolute path to the workspace root.
    pub workspace: String,
}

/// Result returned to the harness suite after a repair attempt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepairJobResult {
    /// `true` when the target test passes after the repair.
    pub success: bool,
    /// Number of steps taken.
    pub steps_taken: usize,
    /// Error message when `success == false`.
    pub error: Option<String>,
}

/// Handle to a running repair job server — drop to shut it down.
pub struct RepairServerHandle {
    pub addr: std::net::SocketAddr,
    _shutdown: tokio::sync::oneshot::Sender<()>,
}

impl RepairServerHandle {
    pub fn local_addr(&self) -> std::net::SocketAddr {
        self.addr
    }
}

/// Start the repair job server.
///
/// `job_fn` is called for each inbound `RepairJobRequest` and must return a
/// `RepairJobResult`.  It runs the actual repair loop (compile-test-patch cycle)
/// using `relay_client_call` for LLM access.
///
/// Jobs are processed **sequentially** — the server accepts the next connection
/// only after the current job completes.  This is intentional: the stateful
/// ChatGPT tab processes one conversation at a time.
pub async fn repair_server_start(
    addr: &str,
    job_fn: impl Fn(RepairJobRequest) -> std::pin::Pin<Box<dyn std::future::Future<Output = RepairJobResult> + Send>>
        + Send
        + Sync
        + 'static,
) -> Result<RepairServerHandle> {
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    let listener = TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    let job_fn = Arc::new(job_fn);
    let (tx, mut rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut rx => break,
                Ok((mut socket, _)) = listener.accept() => {
                    let job_fn = job_fn.clone();
                    let peer = socket.peer_addr().ok();
                    eprintln!(
                        "[repair_server] connection accepted peer={}",
                        peer.map(|p| p.to_string()).unwrap_or_else(|| "unknown".to_string())
                    );
                    // Sequential: await the job before accepting the next connection.
                    let mut buf = Vec::new();
                    if socket.read_to_end(&mut buf).await.is_ok() {
                        eprintln!(
                            "[repair_server] request bytes={} peer={}",
                            buf.len(),
                            peer.map(|p| p.to_string()).unwrap_or_else(|| "unknown".to_string())
                        );
                        let result = if let Ok(req) = serde_json::from_slice::<RepairJobRequest>(&buf) {
                            eprintln!(
                                "[repair_server] decoded job crate={} test={} max_steps={} workspace={}",
                                req.crate_name,
                                req.test_name,
                                req.max_steps,
                                req.workspace
                            );
                            job_fn(req).await
                        } else {
                            eprintln!("[repair_server] invalid repair job request");
                            RepairJobResult {
                                success: false,
                                steps_taken: 0,
                                error: Some("invalid repair job request".to_string()),
                            }
                        };
                        eprintln!(
                            "[repair_server] result success={} steps_taken={} error={}",
                            result.success,
                            result.steps_taken,
                            result.error.as_deref().unwrap_or("none")
                        );
                        if let Ok(bytes) = serde_json::to_vec(&result) {
                            let _ = socket.write_all(&bytes).await;
                        }
                    }
                }
            }
        }
    });

    Ok(RepairServerHandle { addr: local_addr, _shutdown: tx })
}

/// Submit a repair job to a running daemon and block until it completes.
/// Returns the `RepairJobResult` from the daemon.
pub fn repair_client_submit(server_addr: &str, req: &RepairJobRequest) -> Result<RepairJobResult> {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let mut stream = TcpStream::connect(server_addr)?;
    let bytes = serde_json::to_vec(req)?;
    stream.write_all(&bytes)?;
    stream.shutdown(std::net::Shutdown::Write)?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf)?;

    let result: RepairJobResult = serde_json::from_slice(&buf)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_job() -> RepairJobRequest {
        RepairJobRequest {
            crate_name: "canon-loop".to_string(),
            test_name: "synthetic_timeout_recovery".to_string(),
            failure_output: "test synthetic_timeout_recovery ... FAILED\nthread panicked".to_string(),
            incident_context: None,
            max_steps: 8,
            workspace: "/workspace/ai_sandbox/canon".to_string(),
        }
    }

    // ── serde ────────────────────────────────────────────────────────────────

    #[test]
    fn test_repair_job_request_serde_roundtrip() {
        let req = sample_job();
        let json = serde_json::to_string(&req).expect("serialize");
        let decoded: RepairJobRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(req, decoded);
    }

    #[test]
    fn test_repair_job_result_serde_roundtrip() {
        for result in [
            RepairJobResult { success: true, steps_taken: 3, error: None },
            RepairJobResult { success: false, steps_taken: 8, error: Some("compile failed".to_string()) },
        ] {
            let json = serde_json::to_string(&result).expect("serialize");
            let decoded: RepairJobResult = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(result, decoded);
        }
    }

    // ── server lifecycle ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_repair_server_binds_and_accepts() {
        let handle = repair_server_start("127.0.0.1:0", |_req| {
            Box::pin(async { RepairJobResult { success: true, steps_taken: 1, error: None } })
        })
        .await
        .expect("repair server must start");

        let addr = handle.local_addr();
        let stream = tokio::net::TcpStream::connect(addr).await;
        assert!(stream.is_ok(), "server must accept TCP connections on {addr}");
    }

    #[tokio::test]
    async fn test_repair_client_submit_success() {
        let handle = repair_server_start("127.0.0.1:0", |_req| {
            Box::pin(async {
                RepairJobResult { success: true, steps_taken: 2, error: None }
            })
        })
        .await
        .expect("start");

        let addr = handle.local_addr().to_string();
        let req = sample_job();

        let result = tokio::task::spawn_blocking(move || repair_client_submit(&addr, &req))
            .await
            .expect("no panic")
            .expect("submit must succeed");

        assert!(result.success);
        assert_eq!(result.steps_taken, 2);
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn test_repair_client_submit_failure_result() {
        let handle = repair_server_start("127.0.0.1:0", |req| {
            let msg = format!("repair exhausted {} steps for {}", req.max_steps, req.test_name);
            Box::pin(async move {
                RepairJobResult { success: false, steps_taken: 8, error: Some(msg) }
            })
        })
        .await
        .expect("start");

        let addr = handle.local_addr().to_string();
        let result = tokio::task::spawn_blocking(move || repair_client_submit(&addr, &sample_job()))
            .await
            .expect("no panic")
            .expect("submit must not Err");

        assert!(!result.success);
        assert!(result.error.as_deref().unwrap_or("").contains("synthetic_timeout_recovery"));
    }

    /// The job function receives the full request including incident_context.
    #[tokio::test]
    async fn test_repair_server_receives_incident_context() {
        let handle = repair_server_start("127.0.0.1:0", |req| {
            let had_context = req.incident_context.is_some();
            Box::pin(async move {
                RepairJobResult {
                    success: had_context,
                    steps_taken: 0,
                    error: if had_context { None } else { Some("no context".to_string()) },
                }
            })
        })
        .await
        .expect("start");

        let addr = handle.local_addr().to_string();
        let mut req = sample_job();
        req.incident_context = Some("incident_kind=llm_timeout_plan_loop".to_string());

        let result = tokio::task::spawn_blocking(move || repair_client_submit(&addr, &req))
            .await
            .expect("no panic")
            .expect("submit");

        assert!(result.success, "job_fn must see the incident_context: {:?}", result.error);
    }

    /// Sequential processing: a second job submitted while the first is running
    /// must wait and complete successfully.
    #[tokio::test]
    async fn test_repair_server_processes_jobs_sequentially() {
        use std::sync::{Arc, Mutex};

        let counter = Arc::new(Mutex::new(0usize));
        let counter_fn = counter.clone();

        let handle = repair_server_start("127.0.0.1:0", move |_req| {
            let c = counter_fn.clone();
            Box::pin(async move {
                let mut guard = c.lock().unwrap();
                *guard += 1;
                let n = *guard;
                drop(guard);
                RepairJobResult { success: true, steps_taken: n, error: None }
            })
        })
        .await
        .expect("start");

        let addr = handle.local_addr().to_string();

        // Submit two sequential jobs.
        let addr1 = addr.clone();
        let r1 = tokio::task::spawn_blocking(move || repair_client_submit(&addr1, &sample_job()))
            .await
            .expect("no panic")
            .expect("job1");

        let addr2 = addr.clone();
        let r2 = tokio::task::spawn_blocking(move || repair_client_submit(&addr2, &sample_job()))
            .await
            .expect("no panic")
            .expect("job2");

        assert!(r1.success);
        assert!(r2.success);
        // Sequential counter increments: 1 then 2.
        assert_eq!(r1.steps_taken + r2.steps_taken, 3, "counter must be 1+2=3: {:?} {:?}", r1, r2);
    }

    /// Dropping the handle shuts down the server.
    #[tokio::test]
    async fn test_repair_server_shuts_down_on_handle_drop() {
        let handle = repair_server_start("127.0.0.1:0", |_req| {
            Box::pin(async { RepairJobResult { success: true, steps_taken: 0, error: None } })
        })
        .await
        .expect("start");

        let addr = handle.local_addr();
        drop(handle);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let result = tokio::net::TcpStream::connect(addr).await;
        assert!(result.is_err(), "server must stop accepting after handle drop");
    }
}
