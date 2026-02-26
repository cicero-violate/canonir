// CONTRACT:
// - No sorting
// - No graph traversal
// - No mutation
// - Pure string rendering of Plan

use crate::emit::helpers::Emit;

pub struct MacroCallEmitter<'a> {
    pub path: &'a str,
    pub tokens: &'a str,
}

impl Emit for MacroCallEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
        let helper = format!("__macro_call_{}", self.path.replace("::", "_"));
        format!(
            "{pad}#[allow(dead_code)]\n{pad}fn {helper}() {{\n{pad}    {path}!({tokens});\n{pad}}}\n",
            pad = pad,
            helper = helper,
            path = self.path,
            tokens = self.tokens
        )
    }
}
