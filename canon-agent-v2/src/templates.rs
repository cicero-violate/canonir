use super::capability::capability_model_assert_class_disjoint;
use super::dag::ExecutionGraph;
use super::goal_embedding;
use super::goal::GoalSpec;
use super::graph_algo;
use super::planner_update::{apply_graph_patch, GraphPatch};
use super::template_index;
use super::TEMPLATE_ROOT;
use anyhow::Result;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
pub struct GraphTemplateStore {
    root: PathBuf,
    index: template_index::GraphTemplateIndex,
    embedding_dim: usize,
}
impl GraphTemplateStore {
    pub fn new(root: PathBuf, embedding_dim: usize) -> Self {
        let index = template_index::GraphTemplateIndex::snapshot_store_load(&root);
        Self { root, index, embedding_dim }
    }
    pub fn path_for(&self, name: &str) -> PathBuf {
        self.root.join(format!("{}.json", self.hash_for(name)))
    }
    pub fn hash_for(&self, name: &str) -> String {
        let mut h = DefaultHasher::new();
        name.hash(&mut h);
        format!("{:016x}", h.finish())
    }
    fn reward_path(&self, name: &str) -> PathBuf {
        self.path_for(name).with_extension("reward")
    }
    fn history_path(&self, name: &str) -> PathBuf {
        self.path_for(name).with_extension("history")
    }
    pub fn snapshot_store_load(&self, name: &str) -> Result<ExecutionGraph> {
        let path = self.path_for(name);
        let data = fs::read_to_string(&path)?;
        let mut graph: ExecutionGraph = serde_json::from_str(&data)?;
        graph.rebuild_index();
        graph.reset_for_execution();
        Ok(graph)
    }
    pub fn snapshot_store_save(&mut self, name: &str, graph: &ExecutionGraph) -> Result<()> {
        let structural_hash = graph_algo::hash_graph_structure(graph);
        if let Some(existing) = self.index.get_by_structural_hash(&structural_hash) {
            if existing.hash != self.hash_for(name) {
                return Ok(());
            }
        }
        fs::create_dir_all(&self.root)?;
        let json = serde_json::to_string_pretty(graph)?;
        fs::write(self.path_for(name), json)?;
        let hash = self.hash_for(name);
        let mut entry = template_index::template_index_entry_from_graph(&hash, name, graph, self.stored_reward(name));
        let mut cache = goal_embedding::goal_embedding_load_cache();
        let g_hash = goal_embedding::goal_embedding_goal_hash(name);
        let (emb, cache_hit) = if let Some(existing) = cache.get(&g_hash) {
            (existing.clone(), true)
        } else {
            let emb = goal_embedding::goal_embedding_embed_goal(name, self.embedding_dim);
            cache.insert(g_hash.clone(), emb.vector.clone());
            (emb.vector, false)
        };
        if !cache_hit {
            goal_embedding::goal_embedding_save_cache(&cache);
        }
        entry.goal_embedding = emb;
        if let Some(existing) = self.index.get(&hash) {
            entry.failure_count = existing.failure_count;
        }
        self.index.upsert(entry);
        self.index.snapshot_store_save();
        Ok(())
    }
    pub fn stored_reward(&self, name: &str) -> f64 {
        fs::read_to_string(self.reward_path(name)).ok().and_then(|s| s.trim().parse::<f64>().ok()).unwrap_or(f64::NEG_INFINITY)
    }
    pub fn record_reward(&self, name: &str, reward: f64) {
        let path = self.history_path(name);
        let line = format!("{}\n", reward);
        let _ = fs::OpenOptions::new().create(true).append(true).open(&path).and_then(|mut f| f.write_all(line.as_bytes()));
    }
    pub fn recent_rewards(&self, name: &str, n: usize) -> Vec<f64> {
        fs::read_to_string(self.history_path(name))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.trim().parse::<f64>().ok())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .take(n)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
    pub fn is_plateaued(&self, name: &str, window: usize, threshold: f64) -> bool {
        let rewards = self.recent_rewards(name, window);
        if rewards.len() < window {
            return false;
        }
        let baseline = rewards[0];
        let best_recent = rewards.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        (best_recent - baseline) < threshold
    }
    pub fn save_with_reward(&mut self, name: &str, graph: &ExecutionGraph, reward: f64) -> Result<()> {
        if reward <= self.stored_reward(name) {
            return Ok(());
        }
        let structural_hash = graph_algo::hash_graph_structure(graph);
        if let Some(existing) = self.index.get_by_structural_hash(&structural_hash) {
            if existing.hash != self.hash_for(name) {
                return Ok(());
            }
        }
        self.snapshot_store_save(name, graph)?;
        fs::write(self.reward_path(name), reward.to_string())?;
        let hash = self.hash_for(name);
        let mut entry = template_index::template_index_entry_from_graph(&hash, name, graph, reward);
        let mut cache = goal_embedding::goal_embedding_load_cache();
        let g_hash = goal_embedding::goal_embedding_goal_hash(name);
        let (emb, cache_hit) = if let Some(existing) = cache.get(&g_hash) {
            (existing.clone(), true)
        } else {
            let emb = goal_embedding::goal_embedding_embed_goal(name, self.embedding_dim);
            cache.insert(g_hash.clone(), emb.vector.clone());
            (emb.vector, false)
        };
        if !cache_hit {
            goal_embedding::goal_embedding_save_cache(&cache);
        }
        entry.goal_embedding = emb;
        if let Some(existing) = self.index.get(&hash) {
            entry.failure_count = existing.failure_count;
        }
        self.index.upsert(entry);
        self.index.snapshot_store_save();
        Ok(())
    }
    pub fn update(&mut self, name: &str, update: GraphPatch) -> Result<()> {
        let mut graph = self.snapshot_store_load(name)?;
        apply_graph_patch(&mut graph, update)?;
        graph.validate().map_err(|e| anyhow::anyhow!(e))?;
        self.snapshot_store_save(name, &graph)?;
        Ok(())
    }
    pub fn exists(&self, name: &str) -> bool {
        self.path_for(name).exists()
    }
    pub fn evict(&mut self, name: &str) {
        let hash = self.hash_for(name);
        let _ = fs::remove_file(self.path_for(name));
        let _ = fs::remove_file(self.reward_path(name));
        let _ = fs::remove_file(self.history_path(name));
        self.index.remove(&hash);
        self.index.snapshot_store_save();
    }
    pub fn find_similar(&self, goal: &GoalSpec, graph: &ExecutionGraph, top_k: usize, goal_w: f64, struct_w: f64, failure_hard_ban: usize) -> template_index::TemplateSearchResult {
        self.index.find_similar(goal, graph, top_k, goal_w, struct_w, failure_hard_ban)
    }
    pub fn record_failure(&mut self, template_hash: &str) {
        self.index.bump_failure_count(template_hash);
        self.index.snapshot_store_save();
    }
    pub fn failure_count(&self, template_hash: &str) -> usize {
        self.index.get(template_hash).map(|e| e.failure_count).unwrap_or(0)
    }
    pub fn record_failure_and_maybe_evict(&mut self, name: &str, threshold: usize) {
        let hash = self.hash_for(name);
        self.record_failure(&hash);
        if threshold > 0 && self.failure_count(&hash) >= threshold {
            self.evict(name);
            return;
        }
        if threshold > 1 {
            let failures = self.failure_count(&hash);
            let reward = self.stored_reward(name);
            if failures >= threshold.saturating_sub(1) && reward <= 0.0 {
                self.evict(name);
            }
        }
    }
    pub fn record_revision(&self, template_name: &str, graph: &ExecutionGraph, reward: f64, rewrites: usize, iter: u64) {
        #[derive(serde::Serialize)]
        struct TemplateRevisionLog {
            template_hash: String,
            reward: f64,
            nodes: usize,
            edges: usize,
            rewrites: usize,
        }
        let revision = TemplateRevisionLog { template_hash: self.hash_for(template_name), reward, nodes: graph.nodes.len(), edges: graph_algo::graph_analysis_edge_count(graph), rewrites };
        let path = PathBuf::from(TEMPLATE_ROOT).join(format!("template_revision_{:04}.json", iter));
        if let Ok(pretty) = serde_json::to_string_pretty(&revision) {
            let _ = std::fs::create_dir_all(Path::new(TEMPLATE_ROOT));
            let _ = std::fs::write(path, pretty);
        }
    }
}
