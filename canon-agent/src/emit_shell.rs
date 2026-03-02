use crate::ir::CodeDelta;

pub fn emit_shell(deltas: &[CodeDelta]) -> String {
    let mut out = String::new();

    for d in deltas {
        match d {
            CodeDelta::ApplyPatch { patch } => {
                out.push_str("apply_patch << 'EOF'\n");
                out.push_str(patch);
                out.push_str("\nEOF\n\n");
            }
            CodeDelta::Bash { command } | CodeDelta::BashReadOnly { command } => {
                out.push_str(command);
                out.push('\n');
            }
        }
    }

    out
}
