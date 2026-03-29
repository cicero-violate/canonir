# Implementation Plan: CapabilityConfig::from_toml_str

## Problem

`CapabilityConfig::snapshot_store_load()` hardcodes the config file path
(`/workspace/ai_sandbox/canon/canon-agent-prompts/capability_config.toml`).
There is no way to construct a `CapabilityConfig` from an in-memory TOML
string, which makes unit tests that need a config fixture impossible without
touching the filesystem.

The relay tests in `canon-llm-runtime/src/relay.rs` call:

```rust
CapabilityConfig::from_toml_str(harness_config_toml())
```

and fail to compile because this method does not exist.

---

## Fix — `canon-utils/canon-llm-runtime/src/config.rs`

### Add `from_toml_str` and `from_toml_path` to `impl CapabilityConfig`

Insert the following immediately after `snapshot_store_load`:

```rust
/// Parse a `CapabilityConfig` from a TOML string.
/// Used in tests and tooling that need an in-memory fixture.
pub fn from_toml_str(toml: &str) -> Result<Self> {
    let raw: CapabilityConfigRawConfig =
        toml::from_str(toml).context("cannot parse capability config TOML")?;
    Self::from_raw(raw)
}

/// Parse a `CapabilityConfig` from an arbitrary file path.
pub fn from_toml_path(path: &std::path::Path) -> Result<Self> {
    let raw_toml = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    Self::from_toml_str(&raw_toml)
}
```

### Extract shared construction logic into `from_raw`

`snapshot_store_load` currently inlines the construction logic.  Extract it:

```rust
fn from_raw(raw: CapabilityConfigRawConfig) -> Result<Self> {
    let (llm_endpoints, planner_endpoint) = match raw.llm.endpoints {
        CapabilityConfigRawEndpoints::List(list) => {
            let planner = list.iter().find(|e| e.role.as_deref() == Some("planner")).cloned();
            (list, planner)
        }
        CapabilityConfigRawEndpoints::Map(map) => {
            let mut list = Vec::new();
            let mut planner = None;
            for (key, mut ep) in map {
                if ep.id.is_empty() { ep.id = key.clone(); }
                if ep.role.is_none() && key == "planner" {
                    ep.role = Some("planner".to_string());
                }
                if ep.role.as_deref() == Some("planner") {
                    planner = Some(ep.clone());
                }
                list.push(ep);
            }
            (list, planner)
        }
    };
    Ok(Self {
        exit_check_command: raw.system.exit_check_command,
        // … all other fields exactly as they appear today in snapshot_store_load …
        llm_endpoints,
        planner_endpoint,
        llm_roles: raw.llm.roles,
        tab_cooldown_ms: raw.llm.tab_cooldown_ms,
    })
}
```

Update `snapshot_store_load` to delegate:

```rust
pub fn snapshot_store_load() -> Result<Self> {
    let raw_toml = std::fs::read_to_string(CAPABILITY_CONFIG_TOML)
        .with_context(|| format!("cannot read {}", CAPABILITY_CONFIG_TOML))?;
    Self::from_toml_str(&raw_toml)
}
```

---

## Files changed

| File | Change |
|---|---|
| `canon-utils/canon-llm-runtime/src/config.rs` | Add `from_toml_str`, `from_toml_path`, `from_raw`; refactor `snapshot_store_load` to delegate |

---

## Expected result

`cargo test -p canon-llm-runtime` compiles.  The relay tests that call
`CapabilityConfig::from_toml_str(harness_config_toml())` reach the point of
actually running, at which point the `relay_server_start`/`relay_client_call`
stubs return `Err` and the tests fail on assertions — giving the harness agent
concrete runtime failures to repair.
