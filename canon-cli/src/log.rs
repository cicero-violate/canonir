use serde::Serialize;
use std::fs::{create_dir_all, File};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
pub struct Entry {
    pub task: String,
    pub status: String,
    pub exit_code: i32,
    pub duration_ms: u128,
    pub stdout: String,
    pub stderr: String,
}

pub fn write(entries: &[Entry]) -> Result<(), String> {
    create_dir_all(".canon-cli").map_err(|e| e.to_string())?;
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let path = format!(".canon-cli/run_{}.json", ts);
    let mut f = File::create(path).map_err(|e| e.to_string())?;
    let data = serde_json::to_string_pretty(entries).unwrap();
    f.write_all(data.as_bytes()).unwrap();
    Ok(())
}

