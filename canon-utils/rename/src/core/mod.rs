pub mod oracle;
pub mod project_editor;
#[cfg(feature = "rustc_frontend")]
pub mod rustc_resolver;
pub mod symbol_id;

pub use oracle::{StructuralEditOracle, StructuralEditOracleApi};
pub use project_editor::{ChangeReport, EditConflict, ProjectEditor};
