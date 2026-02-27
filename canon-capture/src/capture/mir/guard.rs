use crate::types::Stmt;
use std::collections::HashSet;

pub fn structural_guard(
    stmt: &Stmt,
    defined: &HashSet<String>,
    suppressed_sentinel_names: &HashSet<String>,
) -> bool {
    match stmt {
        Stmt::Assign { rhs, .. } => value_known(rhs, defined, suppressed_sentinel_names),
        Stmt::Call { args, .. } => args
            .iter()
            .all(|a| value_known(a, defined, suppressed_sentinel_names)),
        Stmt::FieldAccess { base, .. } => value_known(base, defined, suppressed_sentinel_names),
        Stmt::MethodCall {
            receiver, args, ..
        } => {
            value_known(receiver, defined, suppressed_sentinel_names)
                && args
                    .iter()
                    .all(|a| value_known(a, defined, suppressed_sentinel_names))
        }
        Stmt::StructLit { fields, .. } => fields
            .iter()
            .all(|(_, v)| value_known(v, defined, suppressed_sentinel_names)),
        Stmt::Match { .. } => true,
        _ => true,
    }
}

pub fn value_known(
    value: &str,
    defined: &HashSet<String>,
    suppressed_sentinel_names: &HashSet<String>,
) -> bool {
    if expr_uses_suppressed_sentinel(value, suppressed_sentinel_names) {
        return false;
    }
    if suppressed_sentinel_names.contains(value) {
        return false;
    }
    if is_synthetic_name(value) {
        return false;
    }
    defined.contains(value) || value == "__ret" || is_structural_expr(value)
}

pub fn emit_suppressed_binding(
    lhs: &str,
    defined: &mut HashSet<String>,
    suppressed_sentinel_names: &mut HashSet<String>,
    stmts: &mut Vec<Stmt>,
) -> bool {
    if lhs == "__ret" || defined.contains(lhs) {
        return false;
    }
    let lhs = lhs.to_string();
    defined.insert(lhs.clone());
    suppressed_sentinel_names.insert(lhs.clone());
    stmts.push(Stmt::Assign {
        lhs,
        rhs: "__canon_suppressed__".to_string(),
    });
    true
}

fn expr_uses_suppressed_sentinel(value: &str, suppressed_sentinel_names: &HashSet<String>) -> bool {
    value
        .split(|c: char| !(c == '_' || c.is_ascii_alphanumeric()))
        .any(|tok| !tok.is_empty() && suppressed_sentinel_names.contains(tok))
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
        || value
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit() || c == '-')
}
