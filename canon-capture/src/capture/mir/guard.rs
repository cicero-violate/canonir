use crate::types::Stmt;
use std::collections::HashSet;

pub fn structural_guard(stmt: &Stmt, defined: &HashSet<String>, suppressed_sentinel_names: &HashSet<String>) -> bool {
    match stmt {
        Stmt::Assign { rhs, .. } => value_known(rhs, defined, suppressed_sentinel_names),
        Stmt::Call { args, dest, .. } => {
            let allow_suppressed_inputs = matches!(dest, Some(dest) if dest == "__ret");
            args.iter().all(|a| value_known_with_mode(a, defined, suppressed_sentinel_names, allow_suppressed_inputs))
        }
        Stmt::FieldAccess { base, .. } => value_known(base, defined, suppressed_sentinel_names),
        Stmt::MethodCall { receiver, args, .. } => value_known(receiver, defined, suppressed_sentinel_names) && args.iter().all(|a| value_known(a, defined, suppressed_sentinel_names)),
        Stmt::StructLit { fields, .. } => fields.iter().all(|(_, v)| value_known(v, defined, suppressed_sentinel_names)),
        Stmt::Match { .. } => true,
        _ => true,
    }
}

pub fn value_known(value: &str, defined: &HashSet<String>, suppressed_sentinel_names: &HashSet<String>) -> bool {
    value_known_with_mode(value, defined, suppressed_sentinel_names, false)
}

fn value_known_with_mode(value: &str, defined: &HashSet<String>, suppressed_sentinel_names: &HashSet<String>, allow_suppressed_inputs: bool) -> bool {
    if expr_uses_suppressed_sentinel(value, suppressed_sentinel_names) {
        return allow_suppressed_inputs;
    }
    if suppressed_sentinel_names.contains(value) {
        return allow_suppressed_inputs;
    }
    if is_synthetic_name(value) {
        return defined.contains(value);
    }
    defined.contains(value) || value == "__ret" || is_structural_expr(value)
}

pub fn emit_suppressed_binding(lhs: &str, defined: &mut HashSet<String>, suppressed_sentinel_names: &mut HashSet<String>, stmts: &mut Vec<Stmt>) -> bool {
    // Suppressed bindings are forbidden by invariant.
    // Do not introduce any synthetic sentinel assignments.
    // Structural lowering and deterministic return fallback
    // must handle incomplete paths instead.
    let _ = (lhs, defined, suppressed_sentinel_names, stmts);
    false
}

fn expr_uses_suppressed_sentinel(value: &str, suppressed_sentinel_names: &HashSet<String>) -> bool {
    value.split(|c: char| !(c == '_' || c.is_ascii_alphanumeric())).any(|tok| !tok.is_empty() && suppressed_sentinel_names.contains(tok))
}

fn is_synthetic_name(s: &str) -> bool {
    let s = s.strip_prefix('_').unwrap_or(s);
    let Some(rest) = s.strip_prefix('v') else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

fn is_structural_expr(value: &str) -> bool {
    value.contains("::")
        || value.starts_with('*')
        || value.contains('(')
        || value.contains(')')
        || value.contains('[')
        || value.contains(']')
        || value.contains('&')
        || value.contains(' ')
        || value.starts_with('"')
        || value.starts_with('\'')
        || value == "true"
        || value == "false"
        || value.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '-')
}
