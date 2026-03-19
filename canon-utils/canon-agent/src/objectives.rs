use crate::goal::{GoalArtifact, GoalType};
use crate::graph_algo;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const BASELINE_PATH: &str = "/workspace/ai_sandbox/canon/state/projections/objective_baseline.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectiveWeights {
    pub branch: f64,
    pub centrality: f64,
    pub redundancy: f64,
    pub hotspot: f64,
    pub deadlock: f64,
    pub depth: f64,
    pub completion_velocity: f64,
}

impl Default for ObjectiveWeights {
    fn default() -> Self {
        Self {
            branch: 0.2,
            centrality: 0.2,
            redundancy: 0.2,
            hotspot: 0.2,
            deadlock: 0.1,
            depth: 0.05,
            completion_velocity: 0.05,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct BranchComplexityEntry {
    symbol: String,
    file: String,
    score: f64,
    #[serde(default)]
    branch_count: u64,
    #[serde(default)]
    duplicate_block_count: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct CallgraphCentralityEntry {
    symbol: String,
    file: String,
    centrality_score: f64,
    #[serde(default)]
    caller_count: u64,
    #[serde(default)]
    callee_count: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct PathRedundancyEntry {
    symbol: String,
    file: String,
    redundancy_ratio: f64,
    #[serde(default)]
    paths_total: u64,
    #[serde(default)]
    paths_unique: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct StructuralHotspotEntry {
    symbol: String,
    file: String,
    score: f64,
    #[serde(default)]
    branch_count: u64,
    #[serde(default)]
    duplicate_blocks: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct MergeCandidateEntry {
    function: String,
    #[serde(default)]
    candidate_blocks: Vec<u64>,
    #[serde(default)]
    successors: Vec<u64>,
    #[serde(default)]
    branch_block: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct DependencyCycleEntry {
    cycle_id: u64,
    cycle_length: u64,
    nodes: Vec<String>,
    files: Vec<String>,
}

#[derive(Debug, Clone)]
struct ObjectiveScore {
    symbol: String,
    file: String,
    priority: f64,
    branch: f64,
    centrality: f64,
    redundancy: f64,
    hotspot: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectiveBaseline {
    pub timestamp: u64,
    pub objective: GoalType,
    pub target_symbol: String,
    pub priority_score: f64,
}

#[derive(Debug, Clone)]
pub struct ObjectiveMetrics {
    pub baseline: f64,
    pub current: f64,
    pub delta: f64,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ObjectiveSelection {
    pub artifact: GoalArtifact,
    pub priority_score: f64,
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<Vec<T>> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn reports_out_root() -> PathBuf {
    if let Ok(p) = std::env::var("CANON_REPORTS_OUT") {
        PathBuf::from(p)
    } else {
        PathBuf::from("/workspace/ai_sandbox/canon/state/reports_out")
    }
}

fn reports_dir() -> PathBuf {
    reports_out_root().join("workspace").join("metrics")
}

fn analysis_dir() -> PathBuf {
    reports_out_root().join("workspace").join("analysis")
}

pub fn reports_last_modified() -> Option<SystemTime> {
    let metrics = reports_dir();
    let analysis = analysis_dir();
    let mut latest: Option<SystemTime> = None;
    let metrics_files = [
        "branch_complexity_report.json",
        "callgraph_centrality_report.json",
        "path_redundancy_report.json",
        "structural_hotspots_report.json",
        "merge_candidates_report.json",
    ];
    for name in metrics_files {
        let modified = std::fs::metadata(metrics.join(name)).ok()?.modified().ok()?;
        latest = Some(latest.map_or(modified, |prev| if modified > prev { modified } else { prev }));
    }
    let cycle_modified = std::fs::metadata(analysis.join("dependency_cycle_report.json")).ok()?.modified().ok()?;
    latest = Some(latest.map_or(cycle_modified, |prev| if cycle_modified > prev { cycle_modified } else { prev }));
    latest
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn normalize_map(map: &HashMap<String, f64>) -> HashMap<String, f64> {
    let max = map.values().cloned().fold(0.0_f64, f64::max);
    if max <= 0.0 {
        return map.iter().map(|(k, _)| (k.clone(), 0.0)).collect();
    }
    map.iter().map(|(k, v)| (k.clone(), v / max)).collect()
}

fn collect_scores(
    branch: &[BranchComplexityEntry],
    centrality: &[CallgraphCentralityEntry],
    redundancy: &[PathRedundancyEntry],
    hotspots: &[StructuralHotspotEntry],
) -> Vec<ObjectiveScore> {
    let mut by_symbol: HashMap<String, ObjectiveScore> = HashMap::new();
    let mut branch_map = HashMap::new();
    let mut centrality_map = HashMap::new();
    let mut redundancy_map = HashMap::new();
    let mut hotspot_map = HashMap::new();

    for entry in branch {
        let enriched = entry.score
            + (entry.branch_count as f64 * 0.01)
            + (entry.duplicate_block_count as f64 * 0.02);
        branch_map.insert(entry.symbol.clone(), enriched);
        let score = by_symbol.entry(entry.symbol.clone()).or_insert(ObjectiveScore {
            symbol: entry.symbol.clone(),
            file: entry.file.clone(),
            priority: 0.0,
            branch: 0.0,
            centrality: 0.0,
            redundancy: 0.0,
            hotspot: 0.0,
        });
        if score.file.is_empty() {
            score.file = entry.file.clone();
        }
    }
    for entry in centrality {
        let enriched = entry.centrality_score
            + (entry.caller_count as f64 * 0.01)
            + (entry.callee_count as f64 * 0.01);
        centrality_map.insert(entry.symbol.clone(), enriched);
        let score = by_symbol.entry(entry.symbol.clone()).or_insert(ObjectiveScore {
            symbol: entry.symbol.clone(),
            file: entry.file.clone(),
            priority: 0.0,
            branch: 0.0,
            centrality: 0.0,
            redundancy: 0.0,
            hotspot: 0.0,
        });
        if score.file.is_empty() {
            score.file = entry.file.clone();
        }
    }
    for entry in redundancy {
        let redundancy_score =
            (1.0 - entry.redundancy_ratio).max(0.0)
            + ((entry.paths_total.saturating_sub(entry.paths_unique)) as f64 * 0.01);
        redundancy_map.insert(entry.symbol.clone(), redundancy_score);
        let score = by_symbol.entry(entry.symbol.clone()).or_insert(ObjectiveScore {
            symbol: entry.symbol.clone(),
            file: entry.file.clone(),
            priority: 0.0,
            branch: 0.0,
            centrality: 0.0,
            redundancy: 0.0,
            hotspot: 0.0,
        });
        if score.file.is_empty() {
            score.file = entry.file.clone();
        }
    }
    for entry in hotspots {
        let enriched = entry.score
            + (entry.branch_count as f64 * 0.01)
            + (entry.duplicate_blocks as f64 * 0.02);
        hotspot_map.insert(entry.symbol.clone(), enriched);
        let score = by_symbol.entry(entry.symbol.clone()).or_insert(ObjectiveScore {
            symbol: entry.symbol.clone(),
            file: entry.file.clone(),
            priority: 0.0,
            branch: 0.0,
            centrality: 0.0,
            redundancy: 0.0,
            hotspot: 0.0,
        });
        if score.file.is_empty() {
            score.file = entry.file.clone();
        }
    }

    let branch_norm = normalize_map(&branch_map);
    let centrality_norm = normalize_map(&centrality_map);
    let redundancy_norm = normalize_map(&redundancy_map);
    let hotspot_norm = normalize_map(&hotspot_map);

    for (symbol, score) in by_symbol.iter_mut() {
        score.branch = *branch_norm.get(symbol).unwrap_or(&0.0);
        score.centrality = *centrality_norm.get(symbol).unwrap_or(&0.0);
        score.redundancy = *redundancy_norm.get(symbol).unwrap_or(&0.0);
        score.hotspot = *hotspot_norm.get(symbol).unwrap_or(&0.0);
    }

    by_symbol.into_values().collect()
}

fn compute_priority(scores: &mut [ObjectiveScore], weights: &ObjectiveWeights) {
    for score in scores.iter_mut() {
        score.priority = weights.branch * score.branch
            + weights.centrality * score.centrality
            + weights.redundancy * score.redundancy
            + weights.hotspot * score.hotspot;
    }
}

fn select_goal_type(score: &ObjectiveScore) -> GoalType {
    let mut best = ("branch", score.branch);
    if score.centrality > best.1 {
        best = ("centrality", score.centrality);
    }
    if score.redundancy > best.1 {
        best = ("redundancy", score.redundancy);
    }
    if score.hotspot > best.1 {
        best = ("hotspot", score.hotspot);
    }
    match best.0 {
        "centrality" => GoalType::SimplifyCallgraph,
        "redundancy" => GoalType::MergePaths,
        "hotspot" => GoalType::ReduceBranching,
        _ => GoalType::ReduceBranching,
    }
}

fn snapshot_features(graph: Option<&crate::task_graph::TaskGraph>) -> Option<graph_algo::GraphFeatureVector> {
    Some(graph_algo::compute_graph_features_parallel(graph?))
}

fn feature_objective_candidates(weights: &ObjectiveWeights, graph: Option<&crate::task_graph::TaskGraph>) -> Vec<ObjectiveSelection> {
    let mut out = Vec::new();
    let features = match snapshot_features(graph) {
        Some(f) => f,
        None => return out,
    };
    if features.deadlock_rate > 0.0 {
        let score = features.deadlock_rate * weights.deadlock;
        out.push(ObjectiveSelection {
            artifact: GoalArtifact {
                target_symbols: Vec::new(),
                target_files: Vec::new(),
                objective_type: GoalType::BreakDeadlock,
                success_criteria: "deadlock_rate ↓".to_string(),
                score,
            },
            priority_score: score,
        });
    }
    if features.nodes > 0 && features.depth > 0 {
        let depth_ratio = features.depth as f64 / features.nodes.max(1) as f64;
        let score = depth_ratio * weights.depth;
        out.push(ObjectiveSelection {
            artifact: GoalArtifact {
                target_symbols: Vec::new(),
                target_files: Vec::new(),
                objective_type: GoalType::ReduceDepth,
                success_criteria: "graph_depth ↓".to_string(),
                score,
            },
            priority_score: score,
        });
    }
    if features.completion_velocity < 1.0 {
        let deficit = (1.0 - features.completion_velocity).max(0.0);
        let score = deficit * weights.completion_velocity;
        out.push(ObjectiveSelection {
            artifact: GoalArtifact {
                target_symbols: Vec::new(),
                target_files: Vec::new(),
                objective_type: GoalType::ImproveCompletionVelocity,
                success_criteria: "completion_velocity ↑".to_string(),
                score,
            },
            priority_score: score,
        });
    }
    out
}

fn feature_objective_score(objective: GoalType, weights: &ObjectiveWeights, graph: Option<&crate::task_graph::TaskGraph>) -> Option<f64> {
    let features = snapshot_features(graph)?;
    let score = match objective {
        GoalType::BreakDeadlock => features.deadlock_rate * weights.deadlock,
        GoalType::ReduceDepth => {
            let ratio = features.depth as f64 / features.nodes.max(1) as f64;
            ratio * weights.depth
        }
        GoalType::ImproveCompletionVelocity => {
            let deficit = (1.0 - features.completion_velocity).max(0.0);
            deficit * weights.completion_velocity
        }
        _ => return None,
    };
    Some(score)
}

fn compute_scores(weights: &ObjectiveWeights) -> Vec<ObjectiveScore> {
    let dir = reports_dir();
    let branch: Vec<BranchComplexityEntry> =
        read_json(&dir.join("branch_complexity_report.json")).unwrap_or_default();
    let centrality: Vec<CallgraphCentralityEntry> =
        read_json(&dir.join("callgraph_centrality_report.json")).unwrap_or_default();
    let redundancy: Vec<PathRedundancyEntry> =
        read_json(&dir.join("path_redundancy_report.json")).unwrap_or_default();
    let hotspots: Vec<StructuralHotspotEntry> =
        read_json(&dir.join("structural_hotspots_report.json")).unwrap_or_default();
    let mut scores = collect_scores(&branch, &centrality, &redundancy, &hotspots);
    compute_priority(&mut scores, weights);
    scores
}

fn priority_for_target(symbol: &str, weights: &ObjectiveWeights) -> Option<f64> {
    let scores = compute_scores(weights);
    scores
        .into_iter()
        .find(|s| s.symbol == symbol)
        .map(|s| s.priority)
}

fn split_targets(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn cycle_targets_present(targets: &[String]) -> Option<bool> {
    let cycles: Vec<DependencyCycleEntry> =
        read_json(&analysis_dir().join("dependency_cycle_report.json"))?;
    if cycles.is_empty() {
        return Some(false);
    }
    for cycle in cycles {
        for node in cycle.nodes {
            if targets.iter().any(|t| t == &node) {
                return Some(true);
            }
        }
    }
    Some(false)
}

pub fn load_goal_from_reports(weights: ObjectiveWeights, graph: Option<&crate::task_graph::TaskGraph>) -> Option<ObjectiveSelection> {
    let cycles: Vec<DependencyCycleEntry> =
        read_json(&analysis_dir().join("dependency_cycle_report.json")).unwrap_or_default();
    if let Some(cycle) = cycles.first() {
        let target_symbols = cycle.nodes.clone();
        let target_files = cycle.files.clone();
        let success = "cycle_count = 0".to_string();
        let artifact = GoalArtifact {
            target_symbols,
            target_files,
            objective_type: GoalType::BreakCycle,
            success_criteria: success,
            score: cycle.cycle_length as f64 + cycle.cycle_id as f64 * 0.001,
        };
        return Some(ObjectiveSelection {
            artifact,
            priority_score: 1.0,
        });
    }

    let merges: Vec<MergeCandidateEntry> =
        read_json(&reports_dir().join("merge_candidates_report.json")).unwrap_or_default();

    let scores = compute_scores(&weights);
    let mut best = scores
        .into_iter()
        .max_by(|a, b| a.priority.partial_cmp(&b.priority).unwrap_or(std::cmp::Ordering::Equal));

    if best.is_none() && !merges.is_empty() {
        let top = merges
            .iter()
            .max_by_key(|m| m.candidate_blocks.len())
            .cloned();
        if let Some(entry) = top {
            let artifact = GoalArtifact {
                target_symbols: vec![entry.function.clone()],
                target_files: Vec::new(),
                objective_type: GoalType::MergePaths,
                success_criteria: "path_redundancy ↓".to_string(),
                score: entry.candidate_blocks.len() as f64
                    + entry.successors.len() as f64 * 0.5
                    + entry.branch_block as f64 * 0.01,
            };
            let priority_score = artifact.score;
            return Some(ObjectiveSelection {
                artifact,
                priority_score,
            });
        }
    }

    let mut candidates: Vec<ObjectiveSelection> = Vec::new();
    if let Some(best) = best.take() {
        let goal_type = select_goal_type(&best);
        let success = match goal_type {
            GoalType::ReduceBranching => "branch complexity ↓",
            GoalType::MergePaths => "path redundancy ↓",
            GoalType::SimplifyCallgraph => "callgraph centrality ↓",
            GoalType::BreakCycle => "cycle_count = 0",
            GoalType::RemoveDeadCode => "dead code ↓",
            GoalType::BreakDeadlock => "deadlock_rate ↓",
            GoalType::ReduceDepth => "graph_depth ↓",
            GoalType::ImproveCompletionVelocity => "completion_velocity ↑",
        }
        .to_string();
        let artifact = GoalArtifact {
            target_symbols: vec![best.symbol.clone()],
            target_files: vec![best.file.clone()],
            objective_type: goal_type,
            success_criteria: success,
            score: best.priority,
        };
        candidates.push(ObjectiveSelection {
            artifact,
            priority_score: best.priority,
        });
    }
    candidates.extend(feature_objective_candidates(&weights, graph));
    candidates
        .into_iter()
        .max_by(|a, b| a.priority_score.partial_cmp(&b.priority_score).unwrap_or(std::cmp::Ordering::Equal))
}

pub fn goal_raw_with_artifact(base: &str, artifact: &GoalArtifact) -> String {
    let mut out = String::new();
    let base = base.trim();
    if !base.is_empty() {
        out.push_str(base);
        out.push('\n');
        out.push('\n');
    }
    out.push_str("OBJECTIVE:\n");
    out.push_str(&format!("Type: {}\n", artifact.objective_type));
    out.push_str(&format!(
        "Targets: {}\n",
        artifact.target_symbols.join(", ")
    ));
    if !artifact.target_files.is_empty() {
        out.push_str(&format!("Files: {}\n", artifact.target_files.join(", ")));
    }
    out.push_str(&format!("Success: {}\n", artifact.success_criteria));
    out.push_str(&format!("Priority score: {:.4}\n", artifact.score));
    out
}

/// Read the report most relevant to `artifact` and return a formatted text
/// block suitable for inclusion in an LLM prompt (top N entries by score).
pub fn report_context_for_artifact(artifact: &GoalArtifact) -> String {
    const TOP_N: usize = 10;
    let metrics = reports_dir();
    let analysis = analysis_dir();
    let mut out = String::new();

    match artifact.objective_type {
        GoalType::ReduceBranching => {
            let entries: Vec<BranchComplexityEntry> =
                read_json(&metrics.join("branch_complexity_report.json")).unwrap_or_default();
            if entries.is_empty() { return out; }
            out.push_str("REPORT: branch_complexity (top by score)\n");
            let mut sorted = entries;
            sorted.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
            for e in sorted.iter().take(TOP_N) {
                out.push_str(&format!(
                    "  symbol={} file={} score={:.3} branches={} dups={}\n",
                    e.symbol, e.file, e.score, e.branch_count, e.duplicate_block_count,
                ));
            }
        }
        GoalType::SimplifyCallgraph => {
            let entries: Vec<CallgraphCentralityEntry> =
                read_json(&metrics.join("callgraph_centrality_report.json")).unwrap_or_default();
            if entries.is_empty() { return out; }
            out.push_str("REPORT: callgraph_centrality (top by score)\n");
            let mut sorted = entries;
            sorted.sort_by(|a, b| b.centrality_score.partial_cmp(&a.centrality_score).unwrap_or(std::cmp::Ordering::Equal));
            for e in sorted.iter().take(TOP_N) {
                out.push_str(&format!(
                    "  symbol={} file={} centrality={:.3} callers={} callees={}\n",
                    e.symbol, e.file, e.centrality_score, e.caller_count, e.callee_count,
                ));
            }
        }
        GoalType::MergePaths => {
            let entries: Vec<PathRedundancyEntry> =
                read_json(&metrics.join("path_redundancy_report.json")).unwrap_or_default();
            let merges: Vec<MergeCandidateEntry> =
                read_json(&metrics.join("merge_candidates_report.json")).unwrap_or_default();
            if !entries.is_empty() {
                out.push_str("REPORT: path_redundancy (top by redundancy)\n");
                let mut sorted = entries;
                sorted.sort_by(|a, b| b.redundancy_ratio.partial_cmp(&a.redundancy_ratio).unwrap_or(std::cmp::Ordering::Equal));
                for e in sorted.iter().take(TOP_N) {
                    out.push_str(&format!(
                        "  symbol={} file={} redundancy={:.3} paths_total={} paths_unique={}\n",
                        e.symbol, e.file, e.redundancy_ratio, e.paths_total, e.paths_unique,
                    ));
                }
            }
            if !merges.is_empty() {
                out.push_str("REPORT: merge_candidates (top by candidate blocks)\n");
                let mut sorted = merges;
                sorted.sort_by_key(|m| Reverse(m.candidate_blocks.len()));
                for m in sorted.iter().take(TOP_N) {
                    out.push_str(&format!(
                        "  function={} candidate_blocks={} successors={}\n",
                        m.function, m.candidate_blocks.len(), m.successors.len(),
                    ));
                }
            }
        }
        GoalType::BreakCycle => {
            let cycles: Vec<DependencyCycleEntry> =
                read_json(&analysis.join("dependency_cycle_report.json")).unwrap_or_default();
            if cycles.is_empty() { return out; }
            out.push_str("REPORT: dependency_cycles\n");
            for c in cycles.iter().take(TOP_N) {
                out.push_str(&format!(
                    "  cycle_id={} length={} nodes=[{}] files=[{}]\n",
                    c.cycle_id, c.cycle_length,
                    c.nodes.join(", "), c.files.join(", "),
                ));
            }
        }
        _ => {}
    }
    out
}

pub fn objective_context(artifact: &GoalArtifact) -> String {
    let mut out = String::new();
    out.push_str("OBJECTIVE CONTEXT\n");
    out.push_str(&format!("TARGET_SYMBOL: {}\n", artifact.target_symbols.join(", ")));
    out.push_str(&format!("OBJECTIVE: {}\n", artifact.objective_type));
    out.push_str(&format!("CURRENT_SCORE: {:.4}\n", artifact.score));
    if !artifact.target_files.is_empty() {
        out.push_str(&format!("TARGET_FILES: {}\n", artifact.target_files.join(", ")));
    }
    out.push_str(&format!("SUCCESS: {}\n", artifact.success_criteria));
    out
}

pub fn objective_task_hints(artifact: &GoalArtifact) -> Vec<String> {
    match artifact.objective_type {
        GoalType::MergePaths => vec![
            "Analyze duplicate branches and identify merge candidates".to_string(),
            "Propose and apply refactor to merge paths".to_string(),
            "Run compile/verification check".to_string(),
        ],
        GoalType::BreakCycle => vec![
            "Locate cycle edges and propose cut points".to_string(),
            "Apply dependency rewrite to sever cycle".to_string(),
            "Validate graph is acyclic".to_string(),
        ],
        GoalType::SimplifyCallgraph => vec![
            "Identify central hub responsibilities to split".to_string(),
            "Extract helper functions to reduce fan-in/out".to_string(),
            "Verify behavior with compile check".to_string(),
        ],
        GoalType::ReduceBranching => vec![
            "Locate duplicated branches".to_string(),
            "Refactor to reduce branching/duplicates".to_string(),
            "Compile to verify".to_string(),
        ],
        GoalType::RemoveDeadCode => vec![
            "Identify unused code paths".to_string(),
            "Remove dead code and simplify".to_string(),
            "Compile to verify".to_string(),
        ],
        GoalType::BreakDeadlock => vec![
            "Identify deadlocked nodes and dependencies".to_string(),
            "Rewrite graph to unblock execution".to_string(),
            "Verify deadlock_rate reduced".to_string(),
        ],
        GoalType::ReduceDepth => vec![
            "Identify long dependency chains".to_string(),
            "Refactor to reduce graph depth".to_string(),
            "Verify depth reduction".to_string(),
        ],
        GoalType::ImproveCompletionVelocity => vec![
            "Identify bottleneck nodes".to_string(),
            "Rebalance priorities or dependencies".to_string(),
            "Verify completion_velocity improves".to_string(),
        ],
    }
}

pub fn load_baseline() -> Option<ObjectiveBaseline> {
    let text = std::fs::read_to_string(BASELINE_PATH).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn maybe_write_baseline(selection: &ObjectiveSelection) {
    let mut write = true;
    if let Some(existing) = load_baseline() {
        if existing.target_symbol == selection.artifact.target_symbols.join(",")
            && existing.objective == selection.artifact.objective_type
        {
            write = false;
        }
    }
    if write {
        let baseline = ObjectiveBaseline {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            objective: selection.artifact.objective_type,
            target_symbol: selection.artifact.target_symbols.join(","),
            priority_score: selection.priority_score,
        };
        if let Ok(payload) = serde_json::to_string_pretty(&baseline) {
            let _ = std::fs::write(BASELINE_PATH, payload);
        }
    }
}

pub fn objective_reward_delta() -> f64 {
    let baseline = match load_baseline() {
        Some(b) => b,
        None => return 0.0,
    };
    let targets = split_targets(&baseline.target_symbol);
    if baseline.objective == GoalType::BreakCycle {
        if let Some(present) = cycle_targets_present(&targets) {
            return if present { 0.0 } else { baseline.priority_score.max(0.0) };
        }
    }
    if matches!(
        baseline.objective,
        GoalType::BreakDeadlock | GoalType::ReduceDepth | GoalType::ImproveCompletionVelocity
    ) {
        if let Some(current_score) = feature_objective_score(baseline.objective, &ObjectiveWeights::default(), None) {
            let delta = baseline.priority_score - current_score;
            return if delta > 0.0 { delta } else { 0.0 };
        }
        return 0.0;
    }
    let mut current: Option<f64> = None;
    for target in targets {
        let score = priority_for_target(&target, &ObjectiveWeights::default());
        if let Some(score) = score {
            current = Some(match current {
                Some(prev) => prev.max(score),
                None => score,
            });
        }
    }
    match current {
        Some(current_score) => {
            let delta = baseline.priority_score - current_score;
            if delta > 0.0 { delta } else { 0.0 }
        }
        None => baseline.priority_score.max(0.0),
    }
}

pub fn objective_metrics_for_goal(goal: &crate::goal::GoalSpec) -> Option<ObjectiveMetrics> {
    let artifact = goal.artifact.as_ref()?;
    let baseline = load_baseline()?;
    if baseline.objective != artifact.objective_type {
        return None;
    }
    if baseline.target_symbol != artifact.target_symbols.join(",") {
        return None;
    }
    let targets = split_targets(&baseline.target_symbol);
    let mut current = 0.0;
    if baseline.objective == GoalType::BreakCycle {
        if let Some(present) = cycle_targets_present(&targets) {
            current = if present { baseline.priority_score } else { 0.0 };
        }
    } else if matches!(
        baseline.objective,
        GoalType::BreakDeadlock | GoalType::ReduceDepth | GoalType::ImproveCompletionVelocity
    ) {
        if let Some(score) = feature_objective_score(baseline.objective, &ObjectiveWeights::default(), None) {
            current = score;
        }
    } else {
        for target in &targets {
            if let Some(score) = priority_for_target(target, &ObjectiveWeights::default()) {
                if score > current {
                    current = score;
                }
            }
        }
    }
    let delta = baseline.priority_score - current;
    Some(ObjectiveMetrics {
        baseline: baseline.priority_score,
        current,
        delta,
        targets: artifact.target_symbols.clone(),
    })
}

pub fn objective_metrics_context(goal: &crate::goal::GoalSpec) -> String {
    let Some(metrics) = objective_metrics_for_goal(goal) else {
        return String::new();
    };
    format!(
        "OBJECTIVE_METRIC_BASELINE: {:.4}\nOBJECTIVE_METRIC_CURRENT: {:.4}\nOBJECTIVE_METRIC_DELTA: {:.4}\nOBJECTIVE_TARGET_SYMBOLS: {}\n",
        metrics.baseline,
        metrics.current,
        metrics.delta,
        metrics.targets.join(", ")
    )
}

pub fn maybe_regenerate_reports_if_stale() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, SystemTime};
    static IN_FLIGHT: AtomicBool = AtomicBool::new(false);
    static LAST_START: std::sync::OnceLock<Mutex<Option<SystemTime>>> = std::sync::OnceLock::new();

    if std::env::var("CANON_REPORTS_DISABLE").ok().as_deref() == Some("1") {
        return false;
    }

    let reports_dir = reports_dir();
    let lock_path = reports_dir.join(".regen.lock");
    let tlog_base = std::env::var("CANON_REPORTS_TLOG")
        .unwrap_or_else(|_| "/workspace/ai_sandbox/canon/state/event_log/event.tlog".to_string());
    let tlog = Path::new(&tlog_base);
    let tlog_idx_buf;
    let tlog_idx = if tlog.extension().is_none() {
        // it's a directory (.tlog.d); use the dir mtime directly
        tlog
    } else {
        tlog_idx_buf = format!("{}.idx", tlog_base);
        Path::new(&tlog_idx_buf)
    };
    if !tlog.exists() || !tlog_idx.exists() {
        return false;
    }
    let reports_mtime = reports_last_modified();
    let tlog_mtime = std::fs::metadata(tlog_idx).and_then(|m| m.modified()).ok();
    let should_run = match (reports_mtime, tlog_mtime) {
        (Some(r), Some(t)) => t > r,
        (None, Some(_)) => true,
        _ => false,
    };
    if !should_run {
        return false;
    }
    let last_start = LAST_START.get_or_init(|| Mutex::new(None));
    let now = SystemTime::now();
    if let Ok(mut guard) = last_start.lock() {
        if let Some(prev) = *guard {
            if now.duration_since(prev).unwrap_or_default() < Duration::from_secs(30) {
                return false;
            }
        }
        *guard = Some(now);
    }
    if IN_FLIGHT.swap(true, Ordering::SeqCst) {
        return false;
    }
    // Cross-process lock to avoid spawning multiple report generators.
    let lock_fh = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .ok();
    if lock_fh.is_none() {
        IN_FLIGHT.store(false, Ordering::SeqCst);
        return false;
    }
    let tlog = tlog.to_path_buf();
    let reports_dir = reports_dir.to_path_buf();
    let lock_path = lock_path.to_path_buf();
    std::thread::spawn(move || {
        if let Some(mut fh) = lock_fh {
            let _ = writeln!(
                fh,
                "pid={} started_at={}",
                std::process::id(),
                current_timestamp()
            );
        }
        let _ = (tlog, reports_dir);
        let _ = std::fs::remove_file(&lock_path);
        IN_FLIGHT.store(false, Ordering::SeqCst);
    });
    true
}
