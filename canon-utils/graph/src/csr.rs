use canon_kernel::types::Edge;
use anyhow::{anyhow, Result};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CsrGraph {
    pub row_ptr: Vec<u32>,
    pub col_idx: Vec<u32>,
}

pub fn build_csr(node_count: u32, edges: &[Edge]) -> CsrGraph {
    let mut unique: BTreeSet<(u32, u32)> = BTreeSet::new();
    for edge in edges {
        unique.insert((edge.src, edge.dst));
    }
    let mut row_ptr = vec![0u32; node_count as usize + 1];
    let mut col_idx: Vec<u32> = Vec::with_capacity(unique.len());
    let mut cursor = 0usize;
    for node in 0..node_count {
        row_ptr[node as usize] = cursor as u32;
        for &(src, dst) in unique.range((node, 0)..=(node, u32::MAX)) {
            if src != node {
                break;
            }
            col_idx.push(dst);
            cursor += 1;
        }
    }
    row_ptr[node_count as usize] = cursor as u32;
    CsrGraph { row_ptr, col_idx }
}

pub fn load_csr(output_dir: &Path) -> Result<CsrGraph> {
    let row_ptr = read_bin_u32(output_dir.join("csr_row_ptr.bin"))?;
    let col_idx = read_bin_u32(output_dir.join("csr_col_idx.bin"))?;
    Ok(CsrGraph { row_ptr, col_idx })
}

pub fn find_path(csr: &CsrGraph, start: u32, goal: u32) -> Option<Vec<u32>> {
    let node_count = csr.row_ptr.len().saturating_sub(1);
    if start as usize >= node_count || goal as usize >= node_count {
        return None;
    }
    let mut prev: Vec<Option<u32>> = vec![None; node_count];
    let mut queue: std::collections::VecDeque<u32> = std::collections::VecDeque::new();
    queue.push_back(start);
    prev[start as usize] = Some(start);
    while let Some(node) = queue.pop_front() {
        if node == goal {
            return Some(reconstruct_path(&prev, start, goal));
        }
        let start_idx = csr.row_ptr[node as usize] as usize;
        let end_idx = csr.row_ptr[node as usize + 1] as usize;
        for &next in &csr.col_idx[start_idx..end_idx] {
            if prev[next as usize].is_none() {
                prev[next as usize] = Some(node);
                queue.push_back(next);
            }
        }
    }
    None
}

fn reconstruct_path(prev: &[Option<u32>], start: u32, goal: u32) -> Vec<u32> {
    let mut path = Vec::new();
    let mut cursor = goal;
    loop {
        path.push(cursor);
        if cursor == start {
            break;
        }
        cursor = match prev[cursor as usize] {
            Some(p) => p,
            None => break,
        };
    }
    path.reverse();
    path
}

fn read_bin_u32(path: PathBuf) -> Result<Vec<u32>> {
    let mut file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if bytes.len() % 4 != 0 {
        return Err(anyhow!("invalid u32 binary length"));
    }
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        out.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(out)
}
