use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use anyhow::Result;
use ignore::WalkBuilder;
use nucleo_matcher::{
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
    Config, Matcher, Utf32Str,
};

#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Path relative to the search root.
    pub path: PathBuf,
    /// Absolute path to the match.
    pub full_path: PathBuf,
    pub score: u32,
}

/// Synchronous fuzzy search over files under `root` using nucleo-matcher.
/// Returns top `limit` matches ordered by descending score then path.
pub fn search_files(query: &str, root: &Path, limit: usize) -> Result<Vec<SearchResult>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let pattern = Pattern::new(query, CaseMatching::Smart, Normalization::Smart, AtomKind::Fuzzy);

    let walker = WalkBuilder::new(root)
        .hidden(false)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    let mut results = Vec::new();
    for entry in walker.flatten() {
        let file_type = match entry.file_type() {
            Some(ft) => ft,
            None => continue,
        };
        if !file_type.is_file() {
            continue;
        }
        let full_path = entry.path().to_path_buf();
        let rel = match full_path.strip_prefix(root) {
            Ok(r) => r.to_path_buf(),
            Err(_) => continue,
        };
        let rel_str = match rel.to_str() {
            Some(s) => s,
            None => continue,
        };
        let mut buf = Vec::new();
        let haystack = Utf32Str::new(rel_str, &mut buf);
        let Some(score) = pattern.score(haystack, &mut matcher) else {
            continue;
        };
        results.push(SearchResult {
            path: rel,
            full_path,
            score,
        });
    }

    results.sort_by(|a, b| match b.score.cmp(&a.score) {
        Ordering::Equal => a.path.cmp(&b.path),
        other => other,
    });
    if results.len() > limit {
        results.truncate(limit);
    }
    Ok(results)
}

