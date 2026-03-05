use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct SymbolLocation {
    pub module: String,
}

#[derive(Default, Debug)]
pub struct SymbolResolver {
    table: BTreeMap<String, SymbolLocation>,
}

impl SymbolResolver {
    pub fn register(&mut self, name: impl Into<String>, module: impl Into<String>) {
        self.table.insert(name.into(), SymbolLocation { module: module.into() });
    }

    pub fn resolve(&self, name: &str) -> Option<&SymbolLocation> {
        self.table.get(name)
    }
}
