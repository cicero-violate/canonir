use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct ModuleNode {
    pub name: String,
    pub children: BTreeMap<String, ModuleNode>,
}

impl ModuleNode {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), children: BTreeMap::new() }
    }

    pub fn ensure_child(&mut self, name: impl Into<String>) -> &mut ModuleNode {
        let key = name.into();
        self.children.entry(key.clone()).or_insert_with(|| ModuleNode::new(key))
    }
}
