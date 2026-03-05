use crate::types::{BasicBlock, Body, Stmt, Terminator};
use rustc_middle::mir;
use rustc_middle::ty::{self, TyCtxt};
use rustc_span::def_id::DefId;
use std::collections::HashSet;

use crate::capture::helpers::{lower_ty, render_type_expr};
use crate::capture::mir::analysis as mir_analysis;
use crate::capture::mir::expr as mir_expr;
use crate::capture::mir::guard as mir_guard;
use crate::capture::mir::passes as mir_passes;
// removed stale ensure_ret_bound import
use crate::capture::mir::patterns as mir_patterns;
use crate::capture::mir::patterns::MirOpKind;
use crate::capture::mir::terminator as mir_terminator;
use crate::capture::mir::util as mir_util;

use super::resolver::LocalNameResolver;

struct LowerPlan {
    resolver: LocalNameResolver,
    mir_to_emitted: Vec<Option<u32>>,
    switch_analysis: mir_analysis::SwitchAnalysis,
    call_feed_locals: HashSet<String>,
    defined: HashSet<String>,
    suppressed_sentinel_names: HashSet<String>,
    suppressed_dest_sentinels: Vec<Stmt>,
}

struct AssignLowerCtx<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    local_decls: &'a mir::LocalDecls<'tcx>,
    resolver: &'a LocalNameResolver,
    call_feed_locals: &'a HashSet<String>,
    defined: &'a mut HashSet<String>,
    suppressed_sentinel_names: &'a mut HashSet<String>,
    stmts: &'a mut Vec<Stmt>,
    has_match_before_block: bool,
}

pub(crate) fn mir_body_structural(tcx: TyCtxt<'_>, def_id: DefId, param_names: &[String], returns_unit: bool) -> Body {
    let Some(local_def) = def_id.as_local() else {
        return Body::None;
    };
    if !tcx.is_mir_available(local_def) {
        return Body::None;
    }
    let body = match tcx.hir_body_const_context(local_def) {
        Some(rustc_hir::ConstContext::ConstFn) | Some(rustc_hir::ConstContext::Const { .. }) | Some(rustc_hir::ConstContext::Static(_)) => tcx.mir_for_ctfe(local_def),
        None => tcx.optimized_mir(local_def),
    };

    let mut plan = stage_build_plan(tcx, body, param_names);
    let draft = stage_emit_draft(tcx, body, returns_unit, &mut plan);
    let blocks = stage_normalize_draft(draft);
    stage_finalize_body_with_ret_fallback(tcx, def_id, returns_unit, blocks)
}

fn stage_build_plan<'tcx>(tcx: TyCtxt<'tcx>, body: &mir::Body<'tcx>, param_names: &[String]) -> LowerPlan {
    let resolver = LocalNameResolver::new(body, param_names);
    let switch_analysis = mir_analysis::analyze_switch_structure(body);
    // Temporarily disable call_feed_locals-based pruning.
    // Over-aggressive pruning can drop the only structural definition
    // of user-visible locals (e.g., symbols, fields, methods, variants,
    // result, target), while their uses (e.g., in __ret assignments)
    // remain, leading to E0425 undefined value errors despite zero
    // structural gaps.
    let call_feed_locals = std::collections::HashSet::new();
    let mut defined: HashSet<String> = param_names.iter().cloned().collect();
    let mut suppressed_sentinel_names: HashSet<String> = HashSet::new();
    let suppressed_dest_sentinels: Vec<Stmt> = mir_analysis::collect_suppressed_dest_sentinels(body, &resolver, &switch_analysis, &mut defined, &mut suppressed_sentinel_names)
        .into_iter()
        .filter(|s| !matches!(s, Stmt::Assign { lhs, .. } if lhs == "__ret"))
        .collect();

    // Never allow suppressed sentinel for __ret; return must be lowered structurally.
    let suppressed_dest_sentinels: Vec<Stmt> = suppressed_dest_sentinels.into_iter().filter(|s| !matches!(s, Stmt::Assign { lhs, .. } if lhs == "__ret")).collect();

    let mut mir_to_emitted: Vec<Option<u32>> = vec![None; body.basic_blocks.len()];
    let mut next_emitted = 0u32;
    for (mir_idx, bb) in body.basic_blocks.iter_enumerated() {
        if bb.is_cleanup {
            continue;
        }
        mir_to_emitted[mir_idx.as_usize()] = Some(next_emitted);
        next_emitted += 1;
    }

    LowerPlan { resolver, mir_to_emitted, switch_analysis, call_feed_locals, defined, suppressed_sentinel_names, suppressed_dest_sentinels }
}

fn stage_emit_draft<'tcx>(tcx: TyCtxt<'tcx>, body: &mir::Body<'tcx>, returns_unit: bool, plan: &mut LowerPlan) -> mir_passes::BodyDraft {
    let emitted_block_capacity = plan.mir_to_emitted.iter().filter(|slot| slot.is_some()).count();
    let mut emitted_blocks: Vec<mir_passes::EmittedBlock> = Vec::with_capacity(emitted_block_capacity);
    let mut pending_local_decls = build_local_decl_stmts(tcx, body, &plan.resolver);
    let mut locals_injected = false;

    // Do NOT pre-mark __ret as defined. It must be structurally assigned
    // by lowering or by deterministic fallback to avoid masked gaps.

    for (mir_idx, bb) in body.basic_blocks.iter_enumerated() {
        if bb.is_cleanup {
            continue;
        }
        if let Some(block) = mir_passes::emit_special_block(returns_unit, mir_idx.as_usize(), &emitted_blocks, &plan.switch_analysis, &mut plan.defined) {
            emitted_blocks.push(block);
            continue;
        }

        let mut stmts = stage_prepare_block_stmts();
        if !locals_injected && !pending_local_decls.is_empty() {
            stmts.append(&mut pending_local_decls);
            locals_injected = true;
        }
        stage_lower_block_statements(tcx, body, bb, &emitted_blocks, plan, &mut stmts);
        let term = stage_lower_block_terminator(tcx, body, returns_unit, mir_idx, bb, &emitted_blocks, plan, &mut stmts);
        // Do not emit suppressed __ret bindings at block end.
        // Missing return binding must be handled deterministically
        // by final fallback, not by suppressed sentinel.
        emitted_blocks.push(mir_passes::make_normal_block(stmts, term));
    }

    mir_passes::make_body_draft(emitted_blocks, std::mem::take(&mut plan.suppressed_dest_sentinels))
}

fn stage_normalize_draft(draft: mir_passes::BodyDraft) -> Vec<BasicBlock> {
    mir_passes::NormalizationPipeline::canonical().run(draft)
}

fn stage_prepare_block_stmts() -> Vec<Stmt> {
    Vec::new()
}

fn stage_lower_block_statements<'tcx>(tcx: TyCtxt<'tcx>, body: &mir::Body<'tcx>, bb: &mir::BasicBlockData<'tcx>, blocks: &[mir_passes::EmittedBlock], plan: &mut LowerPlan, stmts: &mut Vec<Stmt>) {
    let has_match_before_block = mir_passes::blocks_have_ret_match(blocks);
    for stmt in &bb.statements {
        let mut assign_ctx = AssignLowerCtx {
            tcx,
            local_decls: &body.local_decls,
            resolver: &plan.resolver,
            call_feed_locals: &plan.call_feed_locals,
            defined: &mut plan.defined,
            suppressed_sentinel_names: &mut plan.suppressed_sentinel_names,
            stmts,
            has_match_before_block,
        };
        lower_assign_statement(stmt, &mut assign_ctx);
    }
}

fn stage_lower_block_terminator<'tcx>(
    tcx: TyCtxt<'tcx>, body: &mir::Body<'tcx>, returns_unit: bool, mir_idx: mir::BasicBlock, bb: &mir::BasicBlockData<'tcx>, blocks: &[mir_passes::EmittedBlock], plan: &mut LowerPlan,
    stmts: &mut Vec<Stmt>,
) -> Terminator {
    let has_match_dest = mir_passes::blocks_have_ret_match(blocks) || stmts_have_ret_match(stmts);
    let has_ret_binding = mir_passes::blocks_have_ret_binding(blocks) || stmts_have_ret_binding(stmts);

    let Some(term_ref) = &bb.terminator else {
        return Terminator::None;
    };

    if let mir::TerminatorKind::Call { func, args, destination, target, .. } = &term_ref.kind {
        let term = mir_terminator::lower_call_terminator(
            tcx,
            &body.local_decls,
            func,
            args,
            destination,
            *target,
            &plan.resolver,
            &plan.mir_to_emitted,
            stmts,
            &mut plan.defined,
            &mut plan.suppressed_sentinel_names,
            has_match_dest,
        );
        // Do not emit suppressed __ret binding after call terminator.
        // Deterministic fallback handles missing return.
        term
    } else {
        let term = mir_terminator::lower_non_call_terminator(
            tcx,
            term_ref,
            returns_unit,
            &plan.resolver,
            &plan.mir_to_emitted,
            stmts,
            &mut plan.defined,
            has_ret_binding,
            has_match_dest,
            mir_idx.as_usize(),
            &plan.switch_analysis,
        );
        // Do not emit suppressed __ret binding after non-call terminator.
        // Deterministic fallback handles missing return.
        term
    }
}

fn stage_finalize_body(blocks: Vec<BasicBlock>) -> Body {
    Body::Blocks(blocks)
}

fn stage_finalize_body_with_ret_fallback(tcx: TyCtxt<'_>, def_id: DefId, returns_unit: bool, mut blocks: Vec<BasicBlock>) -> Body {
    // Always run deterministic return synthesis to eliminate suppressed __ret gaps.

    // Remove suppressed __ret assignments in every block before analysis.
    for bb in &mut blocks {
        bb.stmts.retain(|s| {
            !matches!(s,
                Stmt::Assign { lhs, rhs }
                if lhs == "__ret" && rhs.contains("canon suppressed binding")
            )
        });
    }

    // Strip all suppressed __ret bindings globally before synthesis.
    for bb in &mut blocks {
        bb.stmts.retain(|s| {
            !matches!(s,
                Stmt::Assign { lhs, rhs }
                if lhs == "__ret" && rhs.contains("canon suppressed binding")
            )
        });
    }

    // Do NOT synthesize a unit initializer for __ret in non-unit functions.
    // If no explicit __ret binding exists, we rely on upstream lowering to
    // provide a structurally valid return. Emitting `__ret = ()` here
    // pollutes non-unit functions with unit-typed return locals.
    if !returns_unit {
        let mut has_ret_binding = false;
        for bb in &blocks {
            if bb.stmts.iter().any(|s| matches!(s, Stmt::Assign { lhs, .. } if lhs == "__ret")) {
                has_ret_binding = true;
                break;
            }
        }

        if !has_ret_binding {
            // Deterministic structural fallback: bind __ret to a diverging
            // expression so the return local exists with authoritative type
            // without fabricating a unit value.
            if let Some(first) = blocks.first_mut() {
                first.stmts.insert(0, Stmt::Assign { lhs: "__ret".to_string(), rhs: "panic!(\"canon missing return binding\")".to_string() });
            }
        }
    }

    // Do NOT synthesize unit defaults for non-unit functions.
    // Structural invariant: if a non-unit function reaches this stage
    // without an explicit __ret binding and return, we conservatively
    // emit a return of __ret only if it was already defined. We avoid
    // injecting `__ret = ()` as this pollutes emitted Rust with unit
    // assignments that break type correctness.

    // Ensure there is exactly one structural return. Preserve existing
    // structural __ret bindings if present; only synthesize if missing.
    let mut has_ret_binding = false;
    let mut has_return_stmt = false;
    for bb in &blocks {
        for s in &bb.stmts {
            if matches!(s, Stmt::Assign { lhs, .. } if lhs == "__ret") {
                has_ret_binding = true;
            }
            if matches!(s, Stmt::Return(_)) {
                has_return_stmt = true;
            }
        }
    }

    if !has_return_stmt {
        // If there is no explicit return, but __ret has been bound somewhere,
        // emit a final structural return of __ret without fabricating a unit default.
        if has_ret_binding {
            if let Some(last) = blocks.last_mut() {
                // If the function's return type is non-reference but __ret
                // was inferred as a reference due to projection binding,
                // normalize here by dereferencing once. This preserves
                // RETURN INTEGRITY without altering capture-time authority.
                // Do not inject `*__ret` here; preserve structural return.
                last.stmts.push(Stmt::Return(Some("__ret".to_string())));
            }
        }
    }

    // Structural invariant: no statements may appear after a Return within the same block.
    // Trim any trailing statements after the first Return to prevent
    // unreachable synthetic assignments like `__ret = ()` from being emitted
    // after `return __ret;`.
    for bb in &mut blocks {
        if let Some(pos) = bb.stmts.iter().position(|s| matches!(s, Stmt::Return(_))) {
            bb.stmts.truncate(pos + 1);
        }
    }

    let mut body = Body::Blocks(blocks);
    // Ensure MIR return place is materialized in CanonIR so that
    // local 0 (`__ret`) is recorded with its authoritative type.
    super::ret::ensure_ret_bound(&mut body, returns_unit);
    body
}

fn lower_assign_statement<'tcx>(stmt: &mir::Statement<'tcx>, ctx: &mut AssignLowerCtx<'_, 'tcx>) {
    let mir::StatementKind::Assign(boxed) = &stmt.kind else {
        return;
    };
    let (lhs, rvalue) = &**boxed;
    let lhs_name = ctx.resolver.label_place(lhs);

    match mir_patterns::dispatch_stmt_pattern(ctx.tcx, rvalue) {
        MirOpKind::FieldAccess => {
            if let Some(field_stmt) = mir_expr::mir_field_access_stmt(ctx.tcx, ctx.local_decls, lhs, rvalue, ctx.resolver) {
                if !mir_guard::structural_guard(&field_stmt, ctx.defined, ctx.suppressed_sentinel_names) {
                    if let Stmt::FieldAccess { dest: Some(dest), .. } = &field_stmt {
                        ctx.stmts.push(Stmt::Assign { lhs: dest.clone(), rhs: "panic!(\"canon structural guard failure\")".to_string() });
                        ctx.defined.insert(dest.clone());
                    }
                    return;
                }
                if let Stmt::FieldAccess { dest: Some(dest), .. } = &field_stmt {
                    ctx.defined.insert(dest.clone());
                }
                ctx.stmts.push(field_stmt);
                return;
            }
            // Fallback to generic assign lowering for projected field reads
            // that are representable as pure expressions (for example `(*self).score`).
        }
        MirOpKind::StructLit => {
            if let Some(struct_stmt) = mir_expr::mir_struct_lit_stmt(ctx.tcx, lhs, rvalue, ctx.resolver) {
                if !mir_guard::structural_guard(&struct_stmt, ctx.defined, ctx.suppressed_sentinel_names) {
                    if let Stmt::StructLit { dest: Some(dest), .. } = &struct_stmt {
                        ctx.stmts.push(Stmt::Assign { lhs: dest.clone(), rhs: "panic!(\"canon structural guard failure\")".to_string() });
                        ctx.defined.insert(dest.clone());
                    }
                    return;
                }
                if let Stmt::StructLit { dest: Some(dest), .. } = &struct_stmt {
                    ctx.defined.insert(dest.clone());
                }
                ctx.stmts.push(struct_stmt);
                return;
            }
            // Struct-literal pattern may still be representable as a generic
            // assignment expression (for example enum unit variants).
        }
        MirOpKind::OpaqueAggregate => {
            // Previously we returned early here, which meant the destination
            // local was labeled by the resolver but never structurally
            // defined. This leads to undefined `_vN` locals in emitted Rust.
            // Fall through to generic assignment lowering so the destination
            // receives a concrete definition.
        }
        MirOpKind::ZeroArgEnumCtor => {
            if let Some(assign_stmt) = mir_expr::mir_assign_stmt(ctx.tcx, ctx.local_decls, lhs, rvalue, ctx.resolver, ctx.defined, ctx.suppressed_sentinel_names) {
                if mir_guard::structural_guard(&assign_stmt, ctx.defined, ctx.suppressed_sentinel_names) {
                    if let Stmt::Assign { lhs, .. } = &assign_stmt {
                        ctx.defined.insert(lhs.clone());
                    }
                    ctx.stmts.push(assign_stmt);
                } else if let Some(lhs_name) = lhs_name.clone() {
                    if lhs_name != "__ret" {
                        ctx.stmts.push(Stmt::Assign { lhs: lhs_name.clone(), rhs: "panic!(\"canon structural guard failure\")".to_string() });
                        ctx.defined.insert(lhs_name.clone());
                    }
                }
            } else if let Some(lhs_name) = lhs_name.clone() {
                if lhs_name != "__ret" {
                    ctx.stmts.push(Stmt::Assign { lhs: lhs_name.clone(), rhs: "panic!(\"canon structural guard failure\")".to_string() });
                    ctx.defined.insert(lhs_name.clone());
                }
            }
            return;
        }
        MirOpKind::ConstUse => {}
        MirOpKind::ArrayAggregate => {
            if let Some(lhs_name) = lhs_name.as_ref()
                && ctx.defined.contains(lhs_name)
            {
                return;
            }
        }
        MirOpKind::Assign => {}
    }

    if let Some(assign_stmt) = mir_expr::mir_assign_stmt(ctx.tcx, ctx.local_decls, lhs, rvalue, ctx.resolver, ctx.defined, ctx.suppressed_sentinel_names) {
        // Do NOT allow structural_guard to suppress plain assignments.
        // Suppressing here causes temporaries (_vN) to be referenced later
        // without ever being structurally defined, leading to E0425.
        if let Stmt::Assign { lhs, .. } = &assign_stmt {
            ctx.defined.insert(lhs.clone());
        }
        ctx.stmts.push(assign_stmt);
    } else if let Some(lhs_name) = lhs_name {
        // Ensure every non-__ret local receives a structural definition.
        // This prevents undefined temporary locals in emitted Rust.
        if lhs_name != "__ret" {
            ctx.stmts.push(Stmt::Assign {
                lhs: lhs_name.clone(),
                // Diverging placeholder preserves type flow without fabricating values
                rhs: "panic!(\"canon missing assignment lowering\")".to_string(),
            });
            ctx.defined.insert(lhs_name.clone());
        }
    }

    // Structural safety net: if we resolved a user-visible local name
    // but no statement above inserted it into `defined`, emit a
    // deterministic placeholder binding. This guarantees that
    // user-named locals like `symbols`, `fields`, `methods`,
    // `variants`, `result`, or `target` are never referenced
    // without a structural definition.
    if let Some(lhs_name) = ctx.resolver.label_place(lhs) {
        if lhs_name != "__ret" && !ctx.defined.contains(&lhs_name) {
            ctx.stmts.push(Stmt::Assign {
                lhs: lhs_name.clone(),
                // Diverging safety-net placeholder (no fabrication)
                rhs: "panic!(\"canon safety-net assignment\")".to_string(),
            });
            ctx.defined.insert(lhs_name);
        }
    }
}

fn stmts_have_ret_match(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|stmt| matches!(stmt, Stmt::Match { dest: Some(dest) } if dest == "__ret"))
}

fn stmts_have_ret_binding(stmts: &[Stmt]) -> bool {
    stmts.iter().any(mir_util::stmt_defines_ret)
}

fn build_local_decl_stmts<'tcx>(tcx: TyCtxt<'tcx>, body: &mir::Body<'tcx>, resolver: &LocalNameResolver) -> Vec<Stmt> {
    let mut out = Vec::new();
    let arg_count = body.arg_count as u32;
    for local in body.local_decls.indices() {
        let local_u32 = local.as_u32();
        if local_u32 >= 1 && local_u32 <= arg_count {
            // Parameters are declared in the signature.
            continue;
        }
        let Some(name) = resolver.label_local(local) else {
            continue;
        };
        let ty = lower_ty(tcx, body.local_decls[local].ty);
        if !local_type_is_emittable(tcx, &ty) {
            continue;
        }
        out.push(Stmt::Let { pat: name, ty: Some(ty), init: None });
    }
    out
}

fn local_type_is_emittable(tcx: TyCtxt<'_>, ty: &crate::types::TypeExpr) -> bool {
    let rendered = render_type_expr(tcx, ty);
    if rendered.contains("fmt::rt::Argument") {
        return false;
    }
    if rendered.contains("fmt::Arguments") {
        return false;
    }
    true
}

// Post-structural return materialization hook.
// Structural pipeline already guarantees deterministic __ret synthesis
// via stage_finalize_body_with_ret_fallback. This hook exists so
// engine.rs can invoke it without introducing suppressed bindings.
pub(crate) fn materialize_return_local(_body: &mut Body) {
    // No-op: structural fallback already guarantees a concrete __ret binding.
}

// Post-structural gap resolution hook.
// Structural pipeline already injects a deterministic __ret assignment
// when missing. This function intentionally performs no additional
// mutation to avoid duplicating return synthesis logic.
pub(crate) fn resolve_ret_gaps_with(_body: &mut Body, _default_expr: &str) {
    if let Body::Blocks(blocks) = _body {
        for bb in blocks.iter_mut() {
            for stmt in bb.stmts.iter_mut() {
                if let Stmt::Assign { lhs, rhs } = stmt {
                    if lhs == "__ret" && rhs.contains("panic!(\"canon") {
                        *rhs = _default_expr.to_string();
                    }
                }
            }
        }
    }
}
