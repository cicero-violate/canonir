// pub mod oracle;
// pub mod project_editor;
// pub mod rustc_session;
// pub mod syn_patcher;
// pub mod symbol_id;

// pub use oracle::{StructuralEditOracle, StructuralEditOracleApi};
// pub use project_editor::{ChangeReport, EditConflict, ProjectEditor};

pub mod oracle;
pub mod project_editor;

mod project_editor_helpers;
mod project_editor_load;
mod project_editor_ops;
mod project_editor_rewrite;
mod project_editor_types;

pub mod rustc_session;
pub mod symbol_id;
pub mod syn_patcher;
