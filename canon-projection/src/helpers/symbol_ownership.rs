use std::collections::BTreeMap;

/// Tracks symbol -> module ownership during emission planning.
#[derive(Default, Debug)]
pub struct SymbolOwnership {
    map: BTreeMap<String, String>,
}

impl SymbolOwnership {
    pub fn register(&mut self, symbol: impl Into<String>, module: impl Into<String>) {
        self.map.insert(symbol.into(), module.into());
    }

    pub fn owner(&self, symbol: &str) -> Option<&str> {
        self.map.get(symbol).map(|s| s.as_str())
    }
}
