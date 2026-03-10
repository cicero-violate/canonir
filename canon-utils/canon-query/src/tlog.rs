use crate::{query_file, QueryError, QueryOptions, TlogQueryResult};
use canon_types::{parse_edge_kind, parse_node_kind, EdgeKind, NodeKind};
use std::path::Path;

#[derive(Debug, Clone)]
pub enum TlogRecord {
    Session { ts: u64, project: String },
    Node {
        sym: String,
        kind: NodeKind,
        file: String,
        line: u32,
        col: u32,
        lo: u32,
        hi: u32,
    },
    Edge { src: String, dst: String, kind: EdgeKind },
    File { path: String },
}

impl TlogRecord {
    fn from_line(line: &str) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        match v.get("t")?.as_str()? {
            "SESSION" => Some(TlogRecord::Session {
                ts: v["ts"].as_u64().unwrap_or(0),
                project: v["project"].as_str().unwrap_or("").to_string(),
            }),
            "N" => {
                let kind_raw = v["kind"].as_str().unwrap_or("");
                let kind = parse_node_kind(kind_raw).ok()?;
                Some(TlogRecord::Node {
                    sym: v["sym"].as_str()?.to_string(),
                    kind,
                file: v["file"].as_str().unwrap_or("").to_string(),
                line: v["line"].as_u64().unwrap_or(0) as u32,
                col: v["col"].as_u64().unwrap_or(0) as u32,
                lo: v["lo"].as_u64().unwrap_or(0) as u32,
                hi: v["hi"].as_u64().unwrap_or(0) as u32,
                })
            }
            "E" => {
                let kind_raw = v["kind"].as_str().unwrap_or("");
                let kind = parse_edge_kind(kind_raw).ok()?;
                Some(TlogRecord::Edge {
                    src: v["src"].as_str()?.to_string(),
                    dst: v["dst"].as_str()?.to_string(),
                    kind,
                })
            }
            "F" => Some(TlogRecord::File {
                path: v["path"].as_str()?.to_string(),
            }),
            _ => None,
        }
    }
}

pub struct TlogReader;

impl TlogReader {
    pub fn load_session(path: &Path) -> Result<Vec<TlogRecord>, QueryError> {
        let content = std::fs::read_to_string(path).map_err(QueryError::Io)?;
        let mut records = Vec::new();
        for line in content.lines() {
            if let Some(rec) = TlogRecord::from_line(line) {
                records.push(rec);
            }
        }
        Ok(records)
    }

    pub fn query_by_kind(
        path: &Path,
        kind: &str,
        offset: u64,
    ) -> Result<Vec<TlogRecord>, QueryError> {
        let _ = offset;
        let query = format!("$[?(@.kind == \"{kind}\")]");
        let results = query_file(path, &[query], QueryOptions::default())?;
        Self::collect_from_result(path, &results[0])
    }

    pub fn query_callers(
        path: &Path,
        sym: &str,
        offset: u64,
    ) -> Result<Vec<TlogRecord>, QueryError> {
        let _ = offset;
        let query = format!("$[?(@.dst == \"{sym}\")]");
        let results = query_file(path, &[query], QueryOptions::default())?;
        let all = Self::collect_from_result(path, &results[0])?;
        Ok(all
            .into_iter()
            .filter(|r| matches!(r, TlogRecord::Edge { kind, .. } if *kind == EdgeKind::Call))
            .collect())
    }

    pub fn last_session_offset(idx_path: &Path) -> Result<u64, QueryError> {
        let content = std::fs::read_to_string(idx_path).map_err(QueryError::Io)?;
        let v: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| QueryError::InvalidQueryInput(e.to_string()))?;
        Ok(v["last_session_offset"].as_u64().unwrap_or(0))
    }

    fn collect_from_result(
        path: &Path,
        result: &TlogQueryResult,
    ) -> Result<Vec<TlogRecord>, QueryError> {
        let file_bytes = std::fs::read(path).map_err(QueryError::Io)?;
        let mut records = Vec::new();
        for line_idx in 0..result.number_of_lines {
            if let Some(raw) = result.value(line_idx, 0) {
                if let Some(rec) = TlogRecord::from_line(&raw) {
                    records.push(rec);
                }
            }
            let _ = file_bytes.as_slice();
        }
        Ok(records)
    }
}
