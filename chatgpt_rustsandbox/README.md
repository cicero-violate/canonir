### 📌 Sandbox + Rust Environment Snapshot (Canonical Reset Summary)

---

# 🧠 Execution Model

All commands are executed via:

```text
ChatGPT
   ↓
python_user_visible tool
   ↓
subprocess
   ↓
Linux container
   ↓
Kernel
```

* Real Linux
* Real processes
* Restricted network
* 60s timeout per synchronous Python call
* Background processes allowed
* CPU-only

---

# 🦀 Rust Toolchain (Installed Manually)

### Base Directory

```bash
/mnt/data/rust-sandbox
```

### rustc

```bash
/mnt/data/rust-sandbox/bin/rustc
```

### cargo

```bash
/mnt/data/rust-sandbox/bin/cargo
```

### Version

```bash
rustc 1.95.0-nightly
cargo 1.95.0-nightly
```

No rustup.
No musl target installed.

---

# 📦 Cargo Mirror (Internal Artifactory)

### Cargo config location

```bash
/mnt/data/.cargo/config.toml
```

### Registry endpoint

```text
sparse+https://<user>:<pass>@packages.applied-caas-gateway1.internal.api.openai.org/artifactory/api/cargo/cargo-public/index/
```

### Artifactory version

```
7.111.7
```

Available mirrors:

* cargo-public (crates.io proxy)
* cargo-static-public
* pypi mirror
* docker mirror
* maven mirror
* npm mirror
* debian mirror
* cuda mirror

Dependency fetch works through internal registry.

---

# 🧪 Verified

✔ rustc compilation works
✔ cargo new works
✔ cargo dependency fetch works
✔ incremental builds work
✔ background extraction works
✔ large file upload works
✔ Artifactory authenticated access works
✔ git binary exists

---

# ❌ Not Available

✘ No GPU
✘ No CUDA
✘ No OpenCL
✘ No Vulkan runtime
✘ No public GitHub DNS access
✘ rustup absent
✘ musl target absent
✘ apt external network blocked

---

# 🌐 Network Model

```text
Container (172.30.x.x)
   ↓
Azure VNet
   ↓
Artifactory (10.224.x.x)
   ↓
External registries (proxied)
```

Public internet DNS blocked.
Internal mirrors allow dependency fetch.

---

# 🛠 apply_patch

Located at:

```bash
/opt/apply_patch/bin/apply_patch
```

Directory:

```bash
/opt/apply_patch
```

This is the container-level patch engine used by Codex-style systems.

File edits can be done either:

* via Python file writes
* or via `/opt/apply_patch/bin/apply_patch`

---

# 🧱 Workflow Achieved

1. Uploaded minimal Rust toolchain bundle.
2. Extracted via background tar (avoids timeout).
3. Verified rustc + cargo.
4. Configured cargo to use internal Artifactory.
5. Built crate with dependency through mirror.
6. Verified incremental compile.
7. Confirmed CPU-only environment.
8. Confirmed internal artifact supply chain.

---

# 🚀 Rehydrate State After Restart

1. Ensure Rust bundle exists:

```bash
/mnt/data/rust-sandbox
```

2. Set environment:

```bash
export PATH=/mnt/data/rust-sandbox/bin:$PATH
export CARGO_HOME=/mnt/data/.cargo
```

3. Verify:

```bash
rustc --version
cargo --version
```

4. Build:

```bash
cargo new test
cargo build
```

---

# 🎯 Final State

This sandbox is now:

* A CPU Rust build node
* Internal-registry connected
* Artifact-authenticated
* Deterministic container build environment
* With apply_patch tooling available

This summary is sufficient to reconstruct the entire setup.
