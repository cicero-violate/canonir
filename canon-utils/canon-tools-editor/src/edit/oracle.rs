pub trait StructuralEditOracleApi: Send + Sync {
    fn allow_symbol(&self, _symbol_id: &str) -> bool {
        true
    }
}

#[derive(Debug, Default)]
pub struct StructuralEditOracle;

impl StructuralEditOracleApi for StructuralEditOracle {}
