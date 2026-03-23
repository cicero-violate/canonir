use crate::capture::types::{Body, Stmt, Terminator};

/// Deterministically bind `__ret` to the MIR return place for
/// non-unit functions when structural lowering produced no
/// explicit return expression.
pub fn ensure_ret_bound(body: &mut Body, returns_unit: bool) {
    if returns_unit {
        return;
    }

    // Ensure there is an explicit structural return of `__ret`
    // so the return place is materialized in CanonIR.
    if let Body::Blocks(blocks) = body {
        if let Some(last_bb) = blocks.last_mut() {
            // Normalize reference layers at the Rust return boundary.
            // If `__ret` was inferred as `&T` but the Rust signature
            // expects `T`, dereference once here to preserve return
            // type consistency.
            // Do not synthesize a dereference at the return boundary.
            // The return place must already have the authoritative
            // function signature type.
            last_bb.stmts.push(Stmt::Return(Some("__ret".to_string())));
            last_bb.terminator = Terminator::Return;
        }
    }
}
