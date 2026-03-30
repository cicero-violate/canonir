use anyhow::Result;
use canon_semantic_state::{DevelopmentObjectiveKind, DevelopmentStrategyKind};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Deserialize)]
pub enum LlmEffort {
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
}

#[derive(Debug)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub effort: Option<LlmEffort>,
    pub objectives: Vec<DevelopmentObjectiveKind>,
    pub strategies: Vec<DevelopmentStrategyKind>,
    pub tools: Vec<String>,
    pub preconditions: Vec<String>,
    pub prompt: String,
}

impl Skill {
    pub fn supports(&self, objective: Option<DevelopmentObjectiveKind>, strategy: Option<DevelopmentStrategyKind>) -> bool {
        let objective_ok = objective.is_none_or(|objective| self.objectives.is_empty() || self.objectives.contains(&objective));
        let strategy_ok = strategy.is_none_or(|strategy| self.strategies.is_empty() || self.strategies.contains(&strategy));
        objective_ok && strategy_ok
    }
}

#[derive(Debug, Deserialize, Default)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    effort: Option<LlmEffort>,
    includes: Option<Vec<String>>,
    objectives: Option<Vec<SkillObjective>>,
    strategies: Option<Vec<SkillStrategy>>,
    tools: Option<Vec<String>>,
    preconditions: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SkillObjective {
    ReduceCompilerFailures,
    ReduceContradictionRate,
    IncreaseTestCoverage,
    DecreaseInvalidPlanRate,
    ReduceStalledLoopFrequency,
    ImproveModuleCohesion,
}

impl From<SkillObjective> for DevelopmentObjectiveKind {
    fn from(value: SkillObjective) -> Self {
        match value {
            SkillObjective::ReduceCompilerFailures => Self::ReduceCompilerFailures,
            SkillObjective::ReduceContradictionRate => Self::ReduceContradictionRate,
            SkillObjective::IncreaseTestCoverage => Self::IncreaseTestCoverage,
            SkillObjective::DecreaseInvalidPlanRate => Self::DecreaseInvalidPlanRate,
            SkillObjective::ReduceStalledLoopFrequency => Self::ReduceStalledLoopFrequency,
            SkillObjective::ImproveModuleCohesion => Self::ImproveModuleCohesion,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SkillStrategy {
    FixConfigLintPolicy,
    ApplyTargetedCompilerRepair,
    DiscoverTestSurface,
    AddRegressionTest,
    SimplifyPlanBatch,
    RealignObjectiveFlow,
    RefreshContextBeforeRetry,
    CreateMissingModules,
    RestructureModules,
}

impl From<SkillStrategy> for DevelopmentStrategyKind {
    fn from(value: SkillStrategy) -> Self {
        match value {
            SkillStrategy::FixConfigLintPolicy => Self::FixConfigLintPolicy,
            SkillStrategy::ApplyTargetedCompilerRepair => Self::ApplyTargetedCompilerRepair,
            SkillStrategy::DiscoverTestSurface => Self::DiscoverTestSurface,
            SkillStrategy::AddRegressionTest => Self::AddRegressionTest,
            SkillStrategy::SimplifyPlanBatch => Self::SimplifyPlanBatch,
            SkillStrategy::RealignObjectiveFlow => Self::RealignObjectiveFlow,
            SkillStrategy::RefreshContextBeforeRetry => Self::RefreshContextBeforeRetry,
            SkillStrategy::CreateMissingModules => Self::CreateMissingModules,
            SkillStrategy::RestructureModules => Self::RestructureModules,
        }
    }
}

pub struct SkillRegistry {
    skills_dir: PathBuf,
    cache: RwLock<HashMap<String, Arc<Skill>>>,
}

impl SkillRegistry {
    pub fn new(skills_dir: PathBuf) -> Self {
        Self { skills_dir, cache: RwLock::new(HashMap::new()) }
    }

    pub fn load(&self, skill_path: &str) -> Result<Arc<Skill>> {
        self.load_inner(skill_path, &mut HashSet::new())
    }

    pub fn invalidate(&self, skill_path: &str) {
        self.cache.write().remove(skill_path);
    }

    pub fn invalidate_all(&self) {
        self.cache.write().clear();
    }

    fn load_inner(&self, skill_path: &str, seen: &mut HashSet<String>) -> Result<Arc<Skill>> {
        if let Some(cached) = self.cache.read().get(skill_path) {
            return Ok(cached.clone());
        }
        if !seen.insert(skill_path.to_string()) {
            anyhow::bail!("cycle detected while loading skill {}", skill_path);
        }
        let path = self.skills_dir.join(format!("{skill_path}.md"));
        let content = fs::read_to_string(&path)?;
        let (front, body) = split_frontmatter(&content);
        let fm: SkillFrontmatter = if let Some(raw) = front { serde_yaml::from_str(raw)? } else { SkillFrontmatter::default() };
        let mut included_prompts = Vec::new();
        if let Some(includes) = fm.includes.as_ref() {
            for inc in includes {
                let child = self.load_inner(inc, seen)?;
                included_prompts.push(child.prompt.clone());
            }
        }
        seen.remove(skill_path);
        let mut prompt_parts = included_prompts;
        prompt_parts.push(body.trim().to_string());
        let prompt = prompt_parts.join("\n\n");
        let skill = Arc::new(Skill {
            name: fm.name.unwrap_or_else(|| skill_path.to_string()),
            description: fm.description.unwrap_or_default(),
            effort: fm.effort,
            objectives: fm.objectives.unwrap_or_default().into_iter().map(Into::into).collect(),
            strategies: fm.strategies.unwrap_or_default().into_iter().map(Into::into).collect(),
            tools: fm.tools.unwrap_or_default(),
            preconditions: fm.preconditions.unwrap_or_default(),
            prompt,
        });
        self.cache.write().insert(skill_path.to_string(), skill.clone());
        Ok(skill)
    }

    pub fn select_for(&self, objective: DevelopmentObjectiveKind, strategy: DevelopmentStrategyKind) -> Result<Vec<Arc<Skill>>> {
        self.select_for_scope("", objective, strategy)
    }

    pub fn select_for_scope(&self, scope: &str, objective: DevelopmentObjectiveKind, strategy: DevelopmentStrategyKind) -> Result<Vec<Arc<Skill>>> {
        let mut out = Vec::new();
        for skill_path in collect_skill_paths(&self.skills_dir, scope)? {
            let skill = self.load(&skill_path)?;
            if skill.supports(Some(objective), Some(strategy)) {
                out.push(skill);
            }
        }
        Ok(out)
    }
}

fn collect_skill_paths(root: &PathBuf, scope: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let normalized_scope = scope.trim_matches('/');
    collect_skill_paths_inner(root, root, normalized_scope, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_skill_paths_inner(root: &PathBuf, current: &PathBuf, scope: &str, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_skill_paths_inner(root, &path, scope, out)?;
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let mut skill_path = relative.to_string_lossy().replace('\\', "/");
        if let Some(stripped) = skill_path.strip_suffix(".md") {
            skill_path = stripped.to_string();
        }
        if !scope.is_empty() && !skill_path.starts_with(&format!("{scope}/")) {
            continue;
        }
        out.push(skill_path);
    }
    Ok(())
}

fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
    let trimmed = content.trim_start();
    if let Some(rest) = trimmed.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let fm = &rest[..end];
            let body = &rest[end + 4..];
            return (Some(fm), body);
        }
    }
    (None, content)
}

pub fn global_registry() -> &'static SkillRegistry {
    static REG: Lazy<SkillRegistry> = Lazy::new(|| {
        let dir = std::env::var("CANON_SKILLS_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/workspace/ai_sandbox/canon/canon-agent-prompts/skills"));
        SkillRegistry::new(dir)
    });
    &REG
}
