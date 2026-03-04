use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{PathBuf};

use anyhow::Result;

use super::dag::TaskGraph;
use super::planner_session::PlannerUpdate;
use super::scheduler::apply_planner_update;
use super::template_index;

pub struct TemplateStore {
    root: PathBuf,
    index: template_index::TemplateIndex,
}

impl TemplateStore {
    pub fn new(root: PathBuf) -> Self {
        let index = template_index::TemplateIndex::load(&root);
        Self { root, index }
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

    pub fn load(&self, name: &str) -> Result<TaskGraph> {
        let path = self.path_for(name);
        let data = fs::read_to_string(&path)?;
        let mut graph: TaskGraph = serde_json::from_str(&data)?;
        graph.rebuild_index();
        graph.reset_for_execution();
        Ok(graph)
    }

    pub fn save(&self, name: &str, graph: &TaskGraph) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        let json = serde_json::to_string_pretty(graph)?;
        fs::write(self.path_for(name), json)?;
        Ok(())
    }

    pub fn stored_reward(&self, name: &str) -> f64 {
        fs::read_to_string(self.reward_path(name))
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok())
            .unwrap_or(f64::NEG_INFINITY)
    }

    pub fn record_reward(&self, name: &str, reward: f64) {
        let path = self.history_path(name);
        let line = format!("{}\n", reward);
        let _ = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(line.as_bytes()));
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

    pub fn save_with_reward(&mut self, name: &str, graph: &TaskGraph, reward: f64) -> Result<()> {
        if reward <= self.stored_reward(name) {
            return Ok(());
        }
        self.save(name, graph)?;
        fs::write(self.reward_path(name), reward.to_string())?;
        let hash = self.hash_for(name);
        let mut entry = template_index::entry_from_graph(&hash, name, graph, reward);
        if let Some(existing) = self.index.get(&hash) {
            entry.failure_count = existing.failure_count;
        }
        self.index.upsert(entry);
        self.index.save();
        Ok(())
    }

    pub fn update(&self, name: &str, update: PlannerUpdate) -> Result<()> {
        let mut graph = self.load(name)?;
        apply_planner_update(&mut graph, update)?;
        graph.validate().map_err(|e| anyhow::anyhow!(e))?;
        self.save(name, &graph)?;
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
        self.index.save();
    }

    pub fn find_similar(
        &self,
        goal: &str,
        graph: &TaskGraph,
        top_k: usize,
    ) -> Vec<template_index::SimilarTemplate> {
        self.index.find_similar(goal, graph, top_k)
    }

    pub fn record_failure(&mut self, template_hash: &str) {
        self.index.bump_failure_count(template_hash);
        self.index.save();
    }
}
