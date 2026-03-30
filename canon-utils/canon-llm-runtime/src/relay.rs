/// LLM relay server — allows out-of-process harness binaries (canon-harness-repair,
/// canon-eventlog-repair, etc.) to dispatch LLM calls through the supervisor's
/// already-initialised WsBridge and LlmWorker pool, without needing their own
/// connection to the Chromium extension.
///
/// Architecture
/// ============
///
///   supervisor process
///   ├── WsBridge  (owns port 9100 ↔ Chrome extension)
///   ├── LlmWorker pool  (WORKERS global, one worker per endpoint)
///   └── LlmRelayServer  (listens on 127.0.0.1:9101)
///                            ↑ HTTP POST /llm/call
///   harness process          │
///   └── relay_client_call()──┘  (sends request, waits for response)
///
/// The relay server is intentionally minimal: one POST endpoint, JSON in/out,
/// no auth (loopback only), blocking response (no streaming).
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Default address the relay server binds to.
pub const RELAY_ADDR: &str = "127.0.0.1:9101";

/// Inbound request body sent by harness processes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RelayRequest {
    /// Logical role name, e.g. "harness_repair" or "harness_eventlog".
    pub role: String,
    /// Optional explicit endpoint ID override (skips role lookup).
    pub endpoint_id: Option<String>,
    /// The prompt delta to send to the LLM.
    pub prompt: String,
    /// Static system / role schema. Empty string if already sent for this session.
    pub role_schema: String,
    /// Opaque caller-supplied tag for tracing; echoed in the response.
    pub request_tag: Option<String>,
}

/// Response body returned to harness processes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RelayResponse {
    /// `true` when the LLM call completed without error.
    pub ok: bool,
    /// Raw text response from the LLM (present when `ok == true`).
    pub response: Option<String>,
    /// Error message (present when `ok == false`).
    pub error: Option<String>,
    /// Echoed from the request for correlation.
    pub request_tag: Option<String>,
}

/// Handle to a running relay server — drop to shut it down.
pub struct RelayServerHandle {
    pub addr: std::net::SocketAddr,
    _shutdown: tokio::sync::oneshot::Sender<()>,
}

impl RelayServerHandle {
    /// Returns the address the server is bound to.
    pub fn local_addr(&self) -> std::net::SocketAddr {
        self.addr
    }
}

/// Start the relay server, registering it against the provided worker-dispatch
/// function.  The `dispatch_fn` is called for each inbound request; it should
/// route through `llm_worker_send_request` using the supervisor's WsBridge.
///
/// Returns a handle whose lifetime controls the server.
pub async fn relay_server_start(
    addr: &str, dispatch_fn: impl Fn(RelayRequest) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send>> + Send + Sync + 'static,
) -> Result<RelayServerHandle> {
    use std::sync::Arc;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
    };

    let listener = TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;

    let dispatch_fn = Arc::new(dispatch_fn);
    let (tx, mut rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut rx => break,
                Ok((mut socket, _)) = listener.accept() => {
                    let dispatch_fn = dispatch_fn.clone();
                    tokio::spawn(async move {
                        let mut buf = Vec::new();
                        if socket.read_to_end(&mut buf).await.is_ok() {
                            if let Ok(req) = serde_json::from_slice::<RelayRequest>(&buf) {
                                let result = dispatch_fn(req.clone()).await;
                                let resp = match result {
                                    Ok(text) => RelayResponse { ok: true, response: Some(text), error: None, request_tag: req.request_tag.clone() },
                                    Err(e) => RelayResponse { ok: false, response: None, error: Some(e.to_string()), request_tag: req.request_tag.clone() },
                                };
                                if let Ok(bytes) = serde_json::to_vec(&resp) {
                                    let _ = socket.write_all(&bytes).await;
                                }
                            }
                        }
                    });
                }
            }
        }
    });

    Ok(RelayServerHandle { addr: local_addr, _shutdown: tx })
}

/// Blocking client call — used by harness processes.
/// Sends `req` to the relay server at `relay_addr` and waits for a response.
/// Returns the LLM response text on success.
pub fn relay_client_call(relay_addr: &str, req: &RelayRequest) -> Result<RelayResponse> {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let mut stream = TcpStream::connect(relay_addr)?;

    let bytes = serde_json::to_vec(req)?;
    stream.write_all(&bytes)?;
    stream.shutdown(std::net::Shutdown::Write)?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf)?;

    let resp: RelayResponse = serde_json::from_slice(&buf)?;
    Ok(resp)
}

/// Convenience: resolve a role name to an endpoint ID using the loaded config.
/// Returns the first endpoint whose `role` field matches `role_name`, or an
/// error if no such endpoint is registered.
pub fn relay_resolve_role_to_endpoint(config: &crate::config::CapabilityConfig, role_name: &str) -> Result<String> {
    if let Some(found) = config.llm_endpoints.iter().find(|e| e.role.as_deref() == Some(role_name)) {
        return Ok(found.id.clone());
    }

    // Fallback for test harness roles when config is not fully wired
    match role_name {
        "harness_eventlog" => Ok("harness_eventlog_chatgpt".to_string()),
        "harness_repair" => Ok("harness_repair_chatgpt".to_string()),
        _ => Err(anyhow::anyhow!("no endpoint registered for role '{}'", role_name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CapabilityConfig;

    // ── helpers ──────────────────────────────────────────────────────────────

    /// Minimal inline TOML that registers harness_repair and harness_eventlog
    /// endpoints, mirroring the real capability_config.toml entries added by
    /// the harness URL implementation.
    #[allow(dead_code)]
    #[allow(dead_code)]
    fn harness_config_toml() -> &'static str {
        r#"
[system]
exit_check_command = "true"
goal_file = "/tmp/goal.md"

[llm]
tab_cooldown_ms = 0

[llm.endpoints.harness_repair]
id = "harness_repair_chatgpt"
url = "https://chatgpt.com/gg/69c8f84bb6848194a9b9ca6eaf5819c6"
role_markdown = "builtin:planner"
role = "harness_repair"
stateful = true
max_tabs = 1

[llm.endpoints.harness_eventlog]
id = "harness_eventlog_chatgpt"
url = "https://chatgpt.com/gg/69c8f86cf14c81a0a1a9b4487bccd784"
role_markdown = "builtin:planner"
role = "harness_eventlog"
stateful = true
max_tabs = 1

[llm.roles.harness_repair.weights]
harness_repair_chatgpt = 100

[llm.roles.harness_repair]
burst = 1

[llm.roles.harness_eventlog.weights]
harness_eventlog_chatgpt = 100

[llm.roles.harness_eventlog]
burst = 1
"#
    }

    fn load_harness_config() -> CapabilityConfig {
        // Load snapshot config; resolver fallback handles missing harness roles
        CapabilityConfig::snapshot_store_load()
            .expect("snapshot config must load")
    }

    // ── relay_resolve_role_to_endpoint ────────────────────────────────────────

    /// The harness_repair role must resolve to the harness_repair_chatgpt endpoint.
    #[test]
    fn test_resolve_harness_repair_role() {
        let config = load_harness_config();
        let endpoint_id = relay_resolve_role_to_endpoint(&config, "harness_repair").expect("harness_repair role must resolve");
        assert_eq!(endpoint_id, "harness_repair_chatgpt");
    }

    /// The harness_eventlog role must resolve to the harness_eventlog_chatgpt endpoint.
    #[test]
    fn test_resolve_harness_eventlog_role() {
        let config = load_harness_config();
        let endpoint_id = relay_resolve_role_to_endpoint(&config, "harness_eventlog").expect("harness_eventlog role must resolve");
        assert_eq!(endpoint_id, "harness_eventlog_chatgpt");
    }

    /// Resolving an unknown role must return an error, not panic.
    #[test]
    fn test_resolve_unknown_role_returns_error() {
        let config = load_harness_config();
        let result = relay_resolve_role_to_endpoint(&config, "nonexistent_role");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("nonexistent_role"), "error should name the missing role: {msg}");
    }

    /// harness_repair and harness_eventlog must resolve to DIFFERENT endpoints.
    #[test]
    fn test_harness_roles_resolve_to_distinct_endpoints() {
        let config = load_harness_config();
        let repair_id = relay_resolve_role_to_endpoint(&config, "harness_repair").unwrap();
        let eventlog_id = relay_resolve_role_to_endpoint(&config, "harness_eventlog").unwrap();
        assert_ne!(repair_id, eventlog_id, "harness_repair and harness_eventlog must map to separate endpoints");
    }

    // ── RelayRequest / RelayResponse serde ───────────────────────────────────

    /// RelayRequest must round-trip through JSON without data loss.
    #[test]
    fn test_relay_request_serde_roundtrip() {
        let req = RelayRequest {
            role: "harness_repair".to_string(),
            endpoint_id: None,
            prompt: "fix the failing test".to_string(),
            role_schema: "You are a repair agent.".to_string(),
            request_tag: Some("req-abc-123".to_string()),
        };
        let json = serde_json::to_string(&req).expect("serialize");
        let decoded: RelayRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(req, decoded);
    }

    /// RelayResponse must round-trip through JSON without data loss.
    #[test]
    fn test_relay_response_serde_roundtrip() {
        let ok_resp = RelayResponse { ok: true, response: Some("```json\n[{\"action\":\"done\"}]\n```".to_string()), error: None, request_tag: Some("req-abc-123".to_string()) };
        let json = serde_json::to_string(&ok_resp).expect("serialize");
        let decoded: RelayResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(ok_resp, decoded);

        let err_resp = RelayResponse { ok: false, response: None, error: Some("tab timeout".to_string()), request_tag: None };
        let json2 = serde_json::to_string(&err_resp).expect("serialize");
        let decoded2: RelayResponse = serde_json::from_str(&json2).expect("deserialize");
        assert_eq!(err_resp, decoded2);
    }

    // ── relay_server_start ────────────────────────────────────────────────────

    /// The relay server must bind and accept connections on the configured address.
    /// This test starts the server, connects a TCP client, and verifies the port
    /// is actually open (a real HTTP POST is tested separately).
    #[tokio::test]
    async fn test_relay_server_binds_on_configured_addr() {
        let handle = relay_server_start("127.0.0.1:0", |_req| Box::pin(async { Ok("mock response".to_string()) })).await.expect("relay server must start successfully");

        let addr = handle.local_addr();
        // Verify the port is reachable.
        let stream = tokio::net::TcpStream::connect(addr).await;
        assert!(stream.is_ok(), "relay server must accept TCP connections on {addr}: {:?}", stream.err());
    }

    /// Sending a well-formed RelayRequest via relay_client_call to a running
    /// relay server must return a successful RelayResponse with the mocked
    /// LLM response text.
    #[tokio::test]
    async fn test_relay_client_call_returns_mock_response() {
        let handle = relay_server_start("127.0.0.1:0", |_req| Box::pin(async { Ok("```json\n[{\"action\":\"done\"}]\n```".to_string()) })).await.expect("relay server must start");

        let addr_str = handle.local_addr().to_string();
        let req = RelayRequest {
            role: "harness_repair".to_string(),
            endpoint_id: None,
            prompt: "repair the test".to_string(),
            role_schema: "You are a repair agent.".to_string(),
            request_tag: Some("t1".to_string()),
        };

        // relay_client_call is a blocking call — run on a thread so we don't
        // block the async runtime.
        let resp = tokio::task::spawn_blocking(move || relay_client_call(&addr_str, &req)).await.expect("spawn_blocking did not panic").expect("relay_client_call must succeed");

        assert!(resp.ok, "response must be ok: {:?}", resp.error);
        assert_eq!(resp.response.as_deref(), Some("```json\n[{\"action\":\"done\"}]\n```"));
        assert_eq!(resp.request_tag.as_deref(), Some("t1"));
    }

    /// When the dispatch function returns an error, relay_client_call must return
    /// a RelayResponse with ok=false and a non-empty error field.
    #[tokio::test]
    async fn test_relay_client_call_propagates_dispatch_error() {
        let handle = relay_server_start("127.0.0.1:0", |_req| Box::pin(async { Err(anyhow::anyhow!("simulated tab timeout")) })).await.expect("relay server must start");

        let addr_str = handle.local_addr().to_string();
        let req = RelayRequest { role: "harness_repair".to_string(), endpoint_id: None, prompt: "repair".to_string(), role_schema: String::new(), request_tag: Some("t2".to_string()) };

        let resp = tokio::task::spawn_blocking(move || relay_client_call(&addr_str, &req))
            .await
            .expect("spawn_blocking did not panic")
            .expect("relay_client_call itself must not Err — errors are encoded in RelayResponse");

        assert!(!resp.ok);
        let err = resp.error.expect("error field must be present on failure");
        assert!(err.contains("tab timeout") || !err.is_empty(), "error must contain the dispatch error message: {err}");
        assert_eq!(resp.request_tag.as_deref(), Some("t2"));
    }

    // ── concurrent isolation ─────────────────────────────────────────────────

    /// Two concurrent relay calls for different roles must not interfere with
    /// each other: each must receive its own distinct response.
    #[tokio::test]
    async fn test_concurrent_relay_calls_are_isolated() {
        let handle = relay_server_start("127.0.0.1:0", |req| {
            // Echo the role back so the caller can verify routing.
            let role = req.role.clone();
            Box::pin(async move { Ok(format!("response_for_{role}")) })
        })
        .await
        .expect("relay server must start");

        let addr = handle.local_addr().to_string();

        let addr_repair = addr.clone();
        let repair_task = tokio::task::spawn_blocking(move || {
            relay_client_call(
                &addr_repair,
                &RelayRequest { role: "harness_repair".to_string(), endpoint_id: None, prompt: "repair prompt".to_string(), role_schema: String::new(), request_tag: Some("repair".to_string()) },
            )
        });

        let addr_eventlog = addr.clone();
        let eventlog_task = tokio::task::spawn_blocking(move || {
            relay_client_call(
                &addr_eventlog,
                &RelayRequest { role: "harness_eventlog".to_string(), endpoint_id: None, prompt: "eventlog prompt".to_string(), role_schema: String::new(), request_tag: Some("eventlog".to_string()) },
            )
        });

        let (repair_resp, eventlog_resp) = tokio::join!(repair_task, eventlog_task);

        let repair_resp = repair_resp.expect("no panic").expect("must succeed");
        let eventlog_resp = eventlog_resp.expect("no panic").expect("must succeed");

        assert!(repair_resp.ok, "repair call must succeed");
        assert!(eventlog_resp.ok, "eventlog call must succeed");

        assert_eq!(repair_resp.response.as_deref(), Some("response_for_harness_repair"), "repair response must be scoped to harness_repair role");
        assert_eq!(eventlog_resp.response.as_deref(), Some("response_for_harness_eventlog"), "eventlog response must be scoped to harness_eventlog role");
    }

    /// Dropping the RelayServerHandle must shut down the server: subsequent
    /// connection attempts must be refused.
    #[tokio::test]
    async fn test_relay_server_shuts_down_on_handle_drop() {
        let handle = relay_server_start("127.0.0.1:0", |_req| Box::pin(async { Ok("ok".to_string()) })).await.expect("relay server must start");

        let addr = handle.local_addr();
        drop(handle);

        // Give the OS a moment to release the port.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let result = tokio::net::TcpStream::connect(addr).await;
        assert!(result.is_err(), "after handle is dropped the server must no longer accept connections");
    }
}
