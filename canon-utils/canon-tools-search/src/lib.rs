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
    /// Optional text snippet for content-based searches.
    pub snippet: Option<String>,
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
            snippet: None,
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

/// BM25-style content search (lightweight, in-memory per call).
/// Tokenises files (.rs, .toml, .md, .txt) and ranks by BM25 using term frequencies.
pub fn search_files_bm25(query: &str, root: &Path, limit: usize) -> Result<Vec<SearchResult>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let query_terms: Vec<String> = tokenize(query);
    if query_terms.is_empty() {
        return Ok(Vec::new());
    }

    let mut documents: Vec<(PathBuf, PathBuf, Vec<String>)> = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

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
        // Filter to text-like files to keep cost bounded.
        if !is_text_candidate(&rel) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&full_path) else {
            continue;
        };
        let tokens = tokenize(&content);
        if tokens.is_empty() {
            continue;
        }
        documents.push((rel, full_path, tokens));
    }

    if documents.is_empty() {
        return Ok(Vec::new());
    }

    // Build DF map.
    use std::collections::HashMap;
    let mut doc_freq: HashMap<String, usize> = HashMap::new();
    for (_, _, tokens) in &documents {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        for t in tokens {
            if seen.insert(t) {
                *doc_freq.entry(t.clone()).or_insert(0) += 1;
            }
        }
    }
    let doc_count = documents.len() as f32;
    let k1 = 1.5_f32;
    let b = 0.75_f32;

    let mut scored: Vec<SearchResult> = Vec::new();
    for (rel, full, tokens) in documents {
        let doc_len = tokens.len() as f32;
        let mut tf: HashMap<&str, usize> = HashMap::new();
        for t in &tokens {
            *tf.entry(t).or_insert(0) += 1;
        }
        let avg_dl = doc_len; // per-call; adequate for small sets
        let mut score = 0.0_f32;
        for term in &query_terms {
            let df = *doc_freq.get(term).unwrap_or(&0) as f32;
            if df == 0.0 {
                continue;
            }
            let idf = ((doc_count - df + 0.5) / (df + 0.5) + 1.0).ln();
            let f = *tf.get(term.as_str()).unwrap_or(&0) as f32;
            let numerator = f * (k1 + 1.0);
            let denom = f + k1 * (1.0 - b + b * (doc_len / avg_dl.max(1.0)));
            score += idf * (numerator / denom);
        }
        if score > 0.0 {
            let snippet = make_snippet(&tokens, &query_terms);
            scored.push(SearchResult {
                path: rel,
                full_path: full,
                score: (score * 1000.0) as u32,
                snippet,
            });
        }
    }

    scored.sort_by(|a, b| match b.score.cmp(&a.score) {
        Ordering::Equal => a.path.cmp(&b.path),
        other => other,
    });
    if scored.len() > limit {
        scored.truncate(limit);
    }
    Ok(scored)
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '/' && c != '.')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect()
}

fn is_text_candidate(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("rs" | "toml" | "md" | "txt" | "json" | "yaml" | "yml")
    )
}

fn make_snippet(tokens: &[String], terms: &[String]) -> Option<String> {
    let window = 30;
    for i in 0..tokens.len() {
        if terms.contains(&tokens[i]) {
            let start = i.saturating_sub(3);
            let end = (i + window).min(tokens.len());
            return Some(tokens[start..end].join(" "));
        }
    }
    None
}

