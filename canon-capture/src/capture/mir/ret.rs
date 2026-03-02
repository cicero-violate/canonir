use crate::types::{Body, Expr, ExprKind};

/// Deterministically bind `__ret` to the MIR return place for
/// non-unit functions when structural lowering produced no
/// explicit return expression.
pub fn ensure_ret_bound(body: &mut Body, returns_unit: bool) {
    if returns_unit {
        return;
    }

    if body.has_explicit_return() {
        return;
    }

    body.push_stmt(Expr {
        kind: ExprKind::ReturnPlace,
        span: None,
    });
}

