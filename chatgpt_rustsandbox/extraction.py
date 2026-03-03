#!/usr/bin/env python3
import json
import time
import urllib.request
import importlib.util
import sys
from pathlib import Path

BASE = "http://127.0.0.1:1384"
MNT = Path("/mnt/data")
HOME = Path.home()

RUST_TAR = MNT / "rust-nightly-x86_64-unknown-linux-gnu.tar.gz"
INSTALL_ROOT = HOME / "rust-nightly"
TOOLCHAIN_PREFIX = INSTALL_ROOT / "toolchain"
RUSTC_PATH = TOOLCHAIN_PREFIX / "bin" / "rustc"

AGENT_MAIN = HOME / "autonomous_agent_upgrade" / "main.py"

def post_json(path, data):
    req = urllib.request.Request(
        BASE + path,
        data=json.dumps(data).encode(),
        headers={"Content-Type": "application/json"},
        method="POST"
    )
    with urllib.request.urlopen(req, timeout=10) as r:
        return r.read()

def post_raw(path, data_bytes):
    req = urllib.request.Request(BASE + path, data=data_bytes, method="POST")
    with urllib.request.urlopen(req, timeout=10) as r:
        return r.read()

def start_bash():
    return int(post_json("/open", {
        "cmd": ["/bin/bash"],
        "env": {},
        "cwd": str(HOME),
        "user": ""
    }).decode())

def run_and_wait(pid, cmd, marker, timeout=1800):
    post_raw(f"/write/{pid}", (cmd + "\n").encode())
    output = ""
    start = time.time()
    while time.time() - start < timeout:
        chunk = post_raw(f"/read/{pid}", b"8192").decode(errors="ignore")
        output += chunk
        if marker in output:
            return True, output
        time.sleep(1)
    return False, output

def wait_for_rustc(pid, timeout=600):
    start = time.time()
    while time.time() - start < timeout:
        post_raw(f"/write/{pid}", f"{RUSTC_PATH} --version || echo __RUSTC_FAIL__\n".encode())
        time.sleep(2)
        out = post_raw(f"/read/{pid}", b"4096").decode(errors="ignore")
        if "rustc" in out and "nightly" in out:
            return True, out
        time.sleep(3)
    return False, out

def import_main(path):
    result = {"loaded": False, "error": None}
    if not path.exists():
        result["error"] = "main.py not found"
        return result
    try:
        spec = importlib.util.spec_from_file_location("autonomous_agent_main", str(path))
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        result["loaded"] = True
        return result
    except Exception as e:
        result["error"] = str(e)
        return result

def main():
    report = {}
    pid = start_bash()
    report["pty_pid"] = pid

    run_and_wait(pid, f"mkdir -p {INSTALL_ROOT}", marker="")

    ok_extract, _ = run_and_wait(
        pid,
        f"tar -xzf {RUST_TAR} -C {INSTALL_ROOT} && echo __EXTRACT_DONE__",
        "__EXTRACT_DONE__"
    )
    report["extract_completed"] = ok_extract

    ok_install, _ = run_and_wait(
        pid,
        f"cd {INSTALL_ROOT}/*nightly* && ./install.sh --prefix={TOOLCHAIN_PREFIX} --disable-ldconfig && echo __INSTALL_DONE__",
        "__INSTALL_DONE__"
    )
    report["install_completed"] = ok_install

    ok_rustc, rustc_out = wait_for_rustc(pid)
    report["rustc_ready"] = ok_rustc
    report["rustc_output_sample"] = rustc_out[-500:] if rustc_out else None

    report["main_import"] = import_main(AGENT_MAIN)

    print(json.dumps(report, indent=2))

if __name__ == "__main__":
    main()
