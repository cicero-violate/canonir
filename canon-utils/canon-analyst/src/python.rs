use anyhow::Result;
use std::io::Write;
use std::process::{Command, Stdio};

/// Execute a Python snippet and return (stdout, stderr, exit_code).
/// The snippet receives the tlog path via the CANON_TLOG environment variable.
pub fn run(code: &str, tlog_path: &str) -> Result<PythonResult> {
    // Write the code to a temp file so multi-line scripts work cleanly.
    let mut tmp = tempfile::NamedTempFile::new()?;
    tmp.write_all(code.as_bytes())?;
    tmp.flush()?;

    let output = Command::new("python3").arg(tmp.path()).env("CANON_TLOG", tlog_path).stdout(Stdio::piped()).stderr(Stdio::piped()).output()?;

    Ok(PythonResult { stdout: String::from_utf8_lossy(&output.stdout).into_owned(), stderr: String::from_utf8_lossy(&output.stderr).into_owned(), exit_code: output.status.code().unwrap_or(-1) })
}

pub struct PythonResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl PythonResult {
    /// Format as a terse block for inclusion in the next LLM prompt.
    pub fn to_context_block(&self) -> String {
        let status = if self.exit_code == 0 { "ok" } else { "error" };
        let mut s = format!("exit={} ({})\n", self.exit_code, status);
        if !self.stdout.is_empty() {
            let out = truncate(&self.stdout, 4000);
            s.push_str(&format!("stdout:\n{out}\n"));
        }
        if !self.stderr.is_empty() {
            let err = truncate(&self.stderr, 1000);
            s.push_str(&format!("stderr:\n{err}\n"));
        }
        s
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
