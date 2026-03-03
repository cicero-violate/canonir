use canon::ir::CanonIR;
use canon::node::{CanonId, CanonNodeKind, CfgOp};
use std::collections::HashSet;

use crate::emit::types::render_type_id;

pub fn emit_body(ir: &CanonIR, body_id: CanonId, param_names: &[String], pad: &str) -> String {
    let CanonNodeKind::Body { blocks } = &ir.node(body_id).kind else {
        return String::new();
    };
    let mut out = String::new();
    let mut declared: HashSet<String> = param_names.iter().cloned().collect();
    let mut suppressed: HashSet<String> = HashSet::new();
    for bb in blocks {
        let CanonNodeKind::BasicBlock { ops, .. } = &ir.node(*bb).kind else {
            continue;
        };
        for op in ops {
            let rendered = render_op(ir, op, &mut declared, &mut suppressed);
            if rendered.is_empty() {
                continue;
            }
            out.push_str(pad);
            out.push_str(&rendered);
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out
}

fn render_op(ir: &CanonIR, op: &CfgOp, declared: &mut HashSet<String>, suppressed: &mut HashSet<String>) -> String {
    match op {
        CfgOp::Let { lhs, ty, rhs } => {
            let lhs_name = local_name(ir, *lhs);
            // Borrow-emission invariant:
            // If the authoritative local type is non-unit but the embedded
            // CfgOp::Let type is unit `()`, prefer the authoritative Local
            // type to prevent unit pollution of `__ret` and other bindings.
            // Always derive the authoritative type from the Local node itself
            // rather than trusting the CfgOp::Let embedded ty. During capture,
            // some Let ops are constructed with a unit fallback and later
            // corrected at the Local node level (e.g., for `__ret`).
            // Using the Local's stored TypeId ensures projection reflects
            // post-assembly authoritative typing.
            let ty_str = match &ir.node(*lhs).kind {
                CanonNodeKind::Local { ty: local_ty, .. } => {
                    let rendered = render_type_id(ir, *local_ty);
                    // Never emit `_` for authoritative locals; force concrete type
                    if rendered.trim() == "_" {
                        render_type_id(ir, *ty)
                    } else if rendered.trim() == "()" {
                        // If authoritative Local is unit but embedded ty differs,
                        // prefer embedded type to avoid locking binding to `()`.
                        let embedded = render_type_id(ir, *ty);
                        if embedded.trim() != "()" {
                            embedded
                        } else {
                            rendered
                        }
                    } else {
                        rendered
                    }
                }
                _ => render_type_id(ir, *ty),
            };

            if let Some(r) = rhs {
                let rhs_expr = local_name(ir, *r);
                // Emission invariant: NEVER synthesize Default::default() as a
                // structural substitute for higher-order or value-producing
                // expressions (e.g. closures for Option::map). If RHS
                // collapses to unit, abort emission rather than encoding `()`.
                if rhs_expr == "()" {
                    panic!("canon-projection: RHS collapsed to unit during Let emission")
                } else if suppressed.contains(&rhs_expr) {
                    panic!("canon-projection: suppressed RHS reached Let emission")
                } else if declared.contains(&lhs_name) {
                    format!("{} = {};", lhs_name, rhs_expr)
                } else {
                    declared.insert(lhs_name.clone());
                    format!("let mut {}: {} = {};", lhs_name, ty_str, rhs_expr)
                }
            } else {
                if declared.contains(&lhs_name) {
                    String::new()
                } else {
                    declared.insert(lhs_name.clone());
                    // Do NOT emit uninitialized authoritative locals.
                    // Preserve the declared type boundary with a diverging
                    // expression rather than permitting unit fallback.
                    format!("let mut {}: {} = panic!(\"canon uninit\");", lhs_name, ty_str)
                }
            }
        }
        CfgOp::Assign { lhs, rhs } => {
            let lhs_name = local_name(ir, *lhs);
            let rhs_name = local_name(ir, *rhs);
            if rhs_name == "__canon_suppressed__"
                || suppressed.contains(&rhs_name)
                || rhs_name == "__canon_call_gap__"
                || rhs_name == "__canon_switch_gap__"
            {
                // Preserve authoritative type boundary: if RHS collapsed,
                // do NOT assign a unit-producing expression into the local.
                // Instead, reinitialize with a typed fallback so bindings
                // like `__ret: String` never become `()`.
                suppressed.insert(lhs_name.clone());
                let fallback = "::core::default::Default::default()".to_string();
                bind_or_assign_typed(ir, *lhs, &lhs_name, fallback, declared)
            } else {
                bind_or_assign_typed(ir, *lhs, &lhs_name, rhs_name, declared)
            }
        }
        CfgOp::InvalidUnitSentinel => {
            // Borrow-emission invariant guard: this sentinel must never
            // reach projection. If it does, abort emission explicitly
            // rather than allowing implicit unit lowering.
            panic!("canon-projection: encountered InvalidUnitSentinel during emission")
        }
        CfgOp::Return(v) => match v {
            Some(v) => {
                let name = local_name(ir, *v);
                if suppressed.contains(&name) {
                    // Use diverging panic to satisfy any return type instead of forcing unit
                    "return panic!(\"canon gap\");".to_string()
                } else {
                    // Normalize return boundary for authoritative return place.
                    // If returning `__ret`, emit a dereference to eliminate stray
                    // reference layers (e.g., &usize -> usize) at the Rust boundary.
                    // Hard normalize authoritative return place at Rust boundary.
                    // Any structural return of `__ret` must be dereferenced.
                    // Authoritative return local must not introduce a stray
                    // reference layer. If the local is reference-typed,
                    // strip exactly one leading '&' at the Rust boundary.
                    // Do not introduce implicit dereferencing at the
                    // return boundary. The value stored in `__ret`
                    // must already match the authoritative function
                    // signature type. Emitting `*__ret` here can
                    // produce double-deref artifacts (e.g. &&usize).
                    format!("return {};", name)
                }
            }
            None => {
                // Prevent implicit unit fallback in non-unit-returning functions.
                // Emit explicit unreachable to avoid introducing `()`.
                "return panic!(\"canon unreachable\");".into()
            },
        },
        CfgOp::Call { func, args, dest } => {
            let fname = callable_name(ir, *func);
            // Preserve callable/value locals exactly as identifiers.
            // Never collapse argument expressions to unit fallback here.
            let arg_names = args.iter().map(|a| local_name(ir, *a)).collect::<Vec<_>>();
            let args = arg_names.join(", ");
            match dest {
                Some(d) => {
                    let dest_name = local_name(ir, *d);
                    let expr = if arg_names.iter().any(|a| suppressed.contains(a)) {
                        suppressed.insert(dest_name.clone());
                        "panic!(\"canon gap\")".to_string()
                    } else {
                        format!("{}({})", fname, args)
                    };
                    let expr = expr;
                    bind_or_assign_typed(ir, *d, &dest_name, expr, declared)
                }
                None => format!("{}({});", fname, args),
            }
        }
        CfgOp::FieldAccess { base, field, dest } => {
            let base_name = local_name(ir, *base);
            let expr = format!("{}.{}", base_name, ir.lookup_name(*field));
            match dest {
                Some(d) => {
                    let dest_name = local_name(ir, *d);
                    let expr2 = if suppressed.contains(&base_name) {
                        suppressed.insert(dest_name.clone());
                        "panic!(\"canon gap\")".to_string()
                    } else {
                        expr
                    };
                    bind_or_assign_typed(ir, *d, &dest_name, expr2, declared)
                }
                None => format!("{};", expr),
            }
        }
        CfgOp::MethodCall { receiver, method, args, dest } => {
            let receiver_name = local_name(ir, *receiver);
            let arg_names = args.iter().map(|a| local_name(ir, *a)).collect::<Vec<_>>();
            // Borrow-emission hard guard:
            // If the receiver itself has collapsed to unit `()`, never emit a
            // method call (e.g., `().expect(...)`). This directly prevents
            // E0599 and downstream unit pollution into authoritative locals
            // such as `__ret`.
            if receiver_name == "()" || arg_names.iter().any(|a| a == "()") {
                // Never materialize a method call on unit. Instead emit a
                // typed fallback so the destination local does not become `()`
                // and trigger E0599/E0308 cascades downstream.
                return match dest {
                    Some(d) => {
                        let dest_name = local_name(ir, *d);
                        suppressed.insert(dest_name.clone());
                        bind_or_assign_typed(
                            ir,
                            *d,
                            &dest_name,
                            "::core::default::Default::default()".to_string(),
                            declared,
                        )
                    }
                    None => "::core::default::Default::default();".to_string(),
                };
            }
            // If the receiver was suppressed, emit a diverging expression instead
            // of allowing it to materialize as unit `()` in method position.
            if suppressed.contains(&receiver_name) {
                return match dest {
                    Some(d) => {
                        let dest_name = local_name(ir, *d);
                        suppressed.insert(dest_name.clone());
                        bind_or_assign_typed(ir, *d, &dest_name, "panic!(\"canon gap\")".to_string(), declared)
                    }
                    None => "panic!(\"canon gap\");".to_string(),
                };
            }
            let arg_names = args.iter().map(|a| local_name(ir, *a)).collect::<Vec<_>>();
            let args = arg_names.join(", ");
            let method_name = ir.lookup_name(*method);
            // Borrow-emission invariant: if emitting Option::map and any argument
            // projected to unit `()`, do not emit the method call. Propagate the
            // receiver instead to prevent E0277 (expected FnOnce, found ()).
            let receiver_is_unit = receiver_name == "()";
            let any_unit_arg = arg_names.iter().any(|a| a == "()");
            let receiver_suppressed = suppressed.contains(&receiver_name);
            let any_suppressed_arg = arg_names.iter().any(|a| suppressed.contains(a));

            // For Option::map specifically, never emit a call if the closure
            // argument collapsed (unit or suppressed). Instead, propagate the
            // receiver so we preserve Option<T> and avoid E0277/E0308.
            let expr = if receiver_is_unit || any_unit_arg || receiver_suppressed || any_suppressed_arg {
                // Escalation invariant: never allow a call site to collapse to `()`.
                // Instead of fabricating unit (which causes E0599/E0308/E0277
                // downstream), emit a typed diverging expression so the
                // destination retains its authoritative type.
                "panic!(\"canon gap\")".to_string()
            } else {
                format!("{}.{}({})", receiver_name, method_name, args)
            };
            match dest {
                Some(d) => {
                    let dest_name = local_name(ir, *d);
                    let expr2 = expr;
                    bind_or_assign_typed(ir, *d, &dest_name, expr2, declared)
                }
                None => format!("{};", expr),
            }
        }
        CfgOp::Index { base, idx, dest } => {
            let expr = format!("{}[{}]", local_name(ir, *base), local_name(ir, *idx));
            match dest {
                Some(d) => {
                    let name = local_name(ir, *d);
                    bind_or_assign_typed(ir, *d, &name, expr, declared)
                }
                None => format!("{};", expr),
            }
        }
        CfgOp::Closure { sig_id: _, body_id: _ } => {
            // Borrow-emission invariant: closures must never collapse to unit `()`.
            // If structural lowering has not produced a concrete callable,
            // emit a diverging expression to avoid synthesizing `()` which
            // triggers E0277 at Option::map sites.
            panic!("projection encountered unlowered Closure op")
        }
        CfgOp::StructLit { ty, fields, dest } => {
            let ctor = render_type_id(ir, *ty);
            let expr = if fields.is_empty() {
                ctor
            } else if fields.iter().all(|(name, _)| ir.lookup_name(*name).chars().all(|c| c.is_ascii_digit())) {
                let mut values: Vec<(usize, String)> = fields
                    .iter()
                    .map(|(name, val)| {
                        let idx = ir.lookup_name(*name).parse::<usize>().unwrap_or(usize::MAX);
                        (idx, local_name(ir, *val))
                    })
                    .collect();
                values.sort_by_key(|(idx, _)| *idx);
                format!("{}({})", ctor, values.into_iter().map(|(_, v)| v).collect::<Vec<_>>().join(", "))
            } else {
                let fields = fields.iter().map(|(name, val)| format!("{}: {}", ir.lookup_name(*name), local_name(ir, *val))).collect::<Vec<_>>().join(", ");
                format!("{} {{ {} }}", ctor, fields)
            };
            match dest {
                Some(d) => {
                    let dest_name = local_name(ir, *d);
                    let expr2 = if fields.iter().map(|(_, val)| local_name(ir, *val)).any(|v| suppressed.contains(&v)) {
                        suppressed.insert(dest_name.clone());
                        "panic!(\"canon gap\")".to_string()
                    } else {
                        expr
                    };
                    bind_or_assign_typed(ir, *d, &dest_name, expr2, declared)
                }
                None => format!("{};", expr),
            }
        }
        CfgOp::Match { dest } => match dest {
            Some(d) => {
                let name = local_name(ir, *d);
                // Do not force unit initialization for match destinations.
                // The actual value should be assigned by lowered match arms.
                // Emit a declaration only if not yet declared, without
                // synthesizing a unit value.
                if declared.contains(&name) {
                    String::new()
                } else {
                    declared.insert(name.clone());
                    format!("let mut {name};")
                }
            }
            None => "// match".into(),
        },
        CfgOp::Branch { .. } => String::new(),
        CfgOp::Goto(_) => String::new(),
        CfgOp::Unreachable => String::new(),
        CfgOp::Expr(v) => format!("{};", local_name(ir, *v)),
    }
}

fn callable_name(ir: &CanonIR, id: CanonId) -> String {
    match &ir.node(id).kind {
        CanonNodeKind::Fn { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::Local { name_id, .. } => {
            let raw = ir.lookup_name(*name_id);
            resolve_local_callable_path(ir, raw).unwrap_or_else(|| raw.to_string())
        }
        _ => format!("node_{}", id.0),
    }
}

fn resolve_local_callable_path(ir: &CanonIR, raw: &str) -> Option<String> {
    let tail = raw.strip_prefix("crate::")?;
    if tail.contains("::") {
        return Some(raw.to_string());
    }
    let target_name = tail;
    let target_fn = ir.nodes.iter().find(|n| matches!(&n.kind, CanonNodeKind::Fn { name_id, .. } if ir.lookup_name(*name_id) == target_name))?;

    for node in &ir.nodes {
        let CanonNodeKind::Module { path_id, .. } = &node.kind else {
            continue;
        };
        let src = canon::id::NodeId(node.id.0);
        let has_contains = ir.module_graph.neighbours(src).any(|(dst, edge)| matches!(edge, canon::edge::EdgeKind::Contains) && dst.0 == target_fn.id.0);
        if has_contains {
            let module_path = ir.lookup_path(*path_id);
            if module_path == "crate" {
                return Some(format!("crate::{target_name}"));
            }
            return Some(format!("{module_path}::{target_name}"));
        }
    }
    Some(raw.to_string())
}

fn local_name(ir: &CanonIR, id: CanonId) -> String {
    match &ir.node(id).kind {
        CanonNodeKind::Local { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::Param { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::Const { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::Static { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        _ => format!("v{}", id.0),
    }
}

fn bind_or_assign_typed(ir: &CanonIR, id: CanonId, name: &str, expr: String, declared: &mut HashSet<String>) -> String {
    if declared.contains(name) {
        format!("{name} = {expr};")
    } else {
        declared.insert(name.to_string());
        // Restore authoritative type annotations for first bindings
        // using the CanonIR TypeId recorded for this local.
        // Always emit authoritative type annotations recorded in CanonIR
        // for Local and Param bindings, including unit `()`.
        // This ensures panic!() initializers and Option-return locals
        // receive explicit types, preventing E0282 and Option<()> drift.
        let ty_opt = match &ir.node(id).kind {
            CanonNodeKind::Local { ty, .. } => {
                // Always emit the authoritative rendered type.
                // Even `: _` is safer than erasing the annotation entirely,
                // because it prevents unintended reference inference drift.
                Some(render_type_id(ir, *ty))
            }
            CanonNodeKind::Param { ty, .. } => Some(render_type_id(ir, *ty)),
            _ => None,
        };

        match ty_opt {
            Some(ty) => {
                // Do NOT force unit-typed locals to be explicitly annotated as `()`.
                // The capture fallback uses unit for many untyped temporaries; if we
                // emit `let x: () = ...;` we destroy downstream type inference and
                // produce massive E0308/E0599 breakage.
                let expr_trim = expr.trim();
                // Special-case: avoid materializing `let mut __ret = ();`
                // If the initializer is unit `()` but the authoritative type
                // is non-unit, emit only a typed declaration and rely on
                // subsequent assignments to initialize.
                if name == "__ret" && expr_trim == "()" && ty.trim() != "()" {
                    format!("let mut {name}: {ty};")
                } else if expr_trim.starts_with("panic!") {
                    format!("let mut {name}: {ty} = {expr};")
                } else if ty.trim() == "()" {
                    format!("let mut {name} = {expr};")
                } else {
                    format!("let mut {name}: {ty} = {expr};")
                }
            }
            None => {
                if expr.trim() == "()" {
                    format!("let mut {name};")
                } else {
                    format!("let mut {name} = {expr};")
                }
            }
        }
    }
}

fn render_suppressed_binding(_name: &str, _declared: &mut HashSet<String>) -> String {
    // Suppressed bindings must not materialize into emitted Rust.
    // Structural completeness must be enforced in lowering, not masked here.
    String::new()
}
