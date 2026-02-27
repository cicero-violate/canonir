use canon::ir::CanonIR;
use canon::node::{NameId, PathId};

pub fn emit_macro_call(ir: &CanonIR, path_id: PathId, tokens_id: NameId, pad: &str) -> String {
    format!("{}{}!({});\n", pad, ir.lookup_path(path_id), ir.lookup_body(tokens_id))
}
