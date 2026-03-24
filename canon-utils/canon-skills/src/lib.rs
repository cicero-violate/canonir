use anyhow::Result;
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
    pub prompt: String,
}

#[derive(Debug, Deserialize, Default)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    effort: Option<LlmEffort>,
    includes: Option<Vec<String>>,
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
        let fm: SkillFrontmatter = if let Some(raw) = front {
            serde_yaml::from_str(raw)?
        } else {
            SkillFrontmatter::default()
        };
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
            prompt,
        });
        self.cache.write().insert(skill_path.to_string(), skill.clone());
        Ok(skill)
    }
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
        let dir = std::env::var("CANON_SKILLS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/workspace/ai_sandbox/canon/canon-agent-prompts/skills"));
        SkillRegistry::new(dir)
    });
    &REG
}
