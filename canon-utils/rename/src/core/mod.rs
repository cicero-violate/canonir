pub mod oracle;
pub mod project_editor;
pub mod rustc_resolver;
pub mod rustc_session;
pub mod syn_patcher;
pub mod symbol_id;

pub use oracle::{StructuralEditOracle, StructuralEditOracleApi};
pub use project_editor::{ChangeReport, EditConflict, ProjectEditor};
