use std::process::Command;
use std::time::Instant;
use crate::config::Task;
use crate::log::Entry;

pub fn run(tasks: &[Task], order: &[String]) -> Vec<Entry> {
    let mut res = vec![];
    for name in order {
        let t = tasks.iter().find(|x| &x.name == name).unwrap();
        let start = Instant::now();

        let out = Command::new("sh").arg("-c").arg(&t.cmd).output();

        match out {
            Ok(o) => {
                let ok = o.status.success();
                res.push(Entry {
                    task: name.clone(),
                    status: if ok { "ok" } else { "failed" }.into(),
                    exit_code: o.status.code().unwrap_or(-1),
                    duration_ms: start.elapsed().as_millis(),
                    stdout: String::from_utf8_lossy(&o.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&o.stderr).to_string(),
                });
            }
            Err(e) => {
                res.push(Entry {
                    task: name.clone(),
                    status: "failed".into(),
                    exit_code: -1,
                    duration_ms: 0,
                    stdout: "".into(),
                    stderr: e.to_string(),
                });
            }
        }
    }
    res
}

