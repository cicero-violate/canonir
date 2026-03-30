use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::time::Instant;

use canon_event::LoopPlanned;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

#[derive(Debug, Clone)]
pub struct ScheduledTask {
    pub priority: TaskPriority,
    pub enqueued_at: Instant,
    pub seq: u64,
    pub agent_id: Option<String>,
    pub plan: LoopPlanned,
}

#[derive(Default)]
pub struct Scheduler {
    heap: BinaryHeap<Queued>,
    seq: u64,
    pub agent_capacity: HashMap<String, usize>,
    pub agent_active: HashMap<String, usize>,
}

#[derive(Debug, Clone)]
struct Queued {
    priority: TaskPriority,
    enqueued_at: Instant,
    seq: u64,
    agent_id: Option<String>,
    plan: LoopPlanned,
}

impl Ord for Queued {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher priority first; earlier enqueue first for stability.
        self.priority.cmp(&other.priority).then_with(|| other.enqueued_at.cmp(&self.enqueued_at)).then_with(|| other.seq.cmp(&self.seq))
    }
}

impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Queued {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.seq == other.seq
    }
}

impl Eq for Queued {}

impl Scheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn clear(&mut self) {
        self.heap.clear();
        self.agent_active.clear();
    }

    pub fn push(&mut self, mut task: ScheduledTask) {
        task.seq = self.seq;
        self.seq = self.seq.saturating_add(1);
        self.heap.push(Queued { priority: task.priority, enqueued_at: task.enqueued_at, seq: task.seq, agent_id: task.agent_id.clone(), plan: task.plan });
    }

    pub fn pop_any(&mut self) -> Option<ScheduledTask> {
        let q = self.heap.pop()?;
        if let Some(agent) = q.agent_id.as_deref() {
            if !self.has_capacity(agent) {
                self.heap.push(q);
                return None;
            }
            *self.agent_active.entry(agent.to_string()).or_insert(0) += 1;
        }
        Some(self.to_task(q))
    }

    pub fn pop_for_llm(&mut self, llm_request_id: Option<&str>) -> Option<ScheduledTask> {
        if llm_request_id.is_none() {
            return self.pop_any();
        }
        let target = llm_request_id.unwrap();
        let mut stash: Vec<Queued> = Vec::new();
        let mut found: Option<Queued> = None;
        while let Some(q) = self.heap.pop() {
            if q.plan.llm_request_id.as_deref() == Some(target) {
                found = Some(q);
                break;
            }
            stash.push(q);
        }
        // push back stashed items
        for q in stash {
            self.heap.push(q);
        }
        if let Some(q) = found {
            if let Some(agent) = q.agent_id.as_deref() {
                if !self.has_capacity(agent) {
                    self.heap.push(q);
                    return None;
                }
                *self.agent_active.entry(agent.to_string()).or_insert(0) += 1;
            }
            return Some(self.to_task(q));
        }
        None
    }

    pub fn complete(&mut self, agent_id: Option<&str>) {
        if let Some(agent) = agent_id {
            if let Some(active) = self.agent_active.get_mut(agent) {
                *active = active.saturating_sub(1);
            }
        }
    }

    pub fn peek_llm_request_id(&self) -> Option<String> {
        self.heap.peek().and_then(|q| q.plan.llm_request_id.clone())
    }

    pub fn has_capacity(&self, agent_id: &str) -> bool {
        let cap = self.agent_capacity.get(agent_id).copied().unwrap_or(usize::MAX);
        let active = self.agent_active.get(agent_id).copied().unwrap_or(0);
        active < cap
    }

    fn to_task(&self, q: Queued) -> ScheduledTask {
        ScheduledTask { priority: q.priority, enqueued_at: q.enqueued_at, seq: q.seq, agent_id: q.agent_id, plan: q.plan }
    }
}

pub fn infer_priority(_plan: &LoopPlanned, goodness: Option<f32>, delta_g: Option<f32>) -> TaskPriority {
    if let Some(g) = goodness {
        if g < 0.0 {
            return TaskPriority::High;
        }
    }
    if let Some(d) = delta_g {
        if d < -0.1 {
            return TaskPriority::High;
        }
    }
    if _plan.action_kind == "done" {
        return TaskPriority::Critical;
    }
    TaskPriority::Normal
}

#[derive(Default)]
pub struct DependencyTracker {
    waiting: HashMap<String, HashSet<String>>,
    unblocks: HashMap<String, Vec<ScheduledTask>>,
}

impl DependencyTracker {
    pub fn add(&mut self, task: ScheduledTask) {
        let deps = task.plan.depends_on.clone();
        if deps.is_empty() {
            return;
        }
        if let Some(action_id) = task.plan.action_id.clone() {
            self.waiting.insert(action_id, deps.iter().cloned().collect());
        }
        for dep in deps {
            self.unblocks.entry(dep).or_default().push(task.clone());
        }
    }

    pub fn complete(&mut self, action_id: &str) -> Vec<ScheduledTask> {
        let mut ready = Vec::new();
        if let Some(list) = self.unblocks.remove(action_id) {
            for task in list {
                if let Some(aid) = task.plan.action_id.clone() {
                    if let Some(wait) = self.waiting.get_mut(&aid) {
                        wait.remove(action_id);
                        if wait.is_empty() {
                            self.waiting.remove(&aid);
                            ready.push(task.clone());
                        } else {
                            for dep in wait.clone() {
                                self.unblocks.entry(dep).or_default().push(task.clone());
                            }
                        }
                    } else {
                        ready.push(task.clone());
                    }
                } else {
                    ready.push(task.clone());
                }
            }
        }
        ready
    }

    pub fn clear(&mut self) {
        self.waiting.clear();
        self.unblocks.clear();
    }
}
