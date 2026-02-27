use crate::types::{BasicBlock, Body, Stmt, Terminator};
use rustc_middle::mir;
use rustc_middle::ty::{self, TyCtxt};
use rustc_span::def_id::DefId;
use std::collections::HashSet;

use crate::capture::mir::analysis as mir_analysis;
use crate::capture::mir::expr as mir_expr;
use crate::capture::mir::guard as mir_guard;
use crate::capture::mir::passes as mir_passes;
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

pub(crate) fn mir_body_structural(
    tcx: TyCtxt<'_>,
    def_id: DefId,
    param_names: &[String],
    returns_unit: bool,
) -> Body {
    let Some(local_def) = def_id.as_local() else {
        return Body::None;
    };
    if !tcx.is_mir_available(local_def) {
        return Body::None;
    }
    let body = match tcx.hir_body_const_context(local_def) {
        Some(rustc_hir::ConstContext::ConstFn)
        | Some(rustc_hir::ConstContext::Const { .. })
        | Some(rustc_hir::ConstContext::Static(_)) => tcx.mir_for_ctfe(local_def),
        None => tcx.optimized_mir(local_def),
    };

    let mut plan = stage_build_plan(tcx, body, param_names);
    let draft = stage_emit_draft(tcx, body, returns_unit, &mut plan);
    let blocks = stage_normalize_draft(draft);
    stage_finalize_body(blocks)
}

fn stage_build_plan<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    param_names: &[String],
) -> LowerPlan {
    let resolver = LocalNameResolver::new(body, param_names);
    let switch_analysis = mir_analysis::analyze_switch_structure(body);
    let call_feed_locals = mir_analysis::compute_call_feed_locals(tcx, body, &resolver);
    let mut defined: HashSet<String> = param_names.iter().cloned().collect();
    defined.insert("__ret".to_string());
    let mut suppressed_sentinel_names: HashSet<String> = HashSet::new();
    let suppressed_dest_sentinels = mir_analysis::collect_suppressed_dest_sentinels(
        body,
        &resolver,
        &switch_analysis,
        &mut defined,
        &mut suppressed_sentinel_names,
    );

    let mut mir_to_emitted: Vec<Option<u32>> = vec![None; body.basic_blocks.len()];
    let mut next_emitted = 0u32;
    for (mir_idx, bb) in body.basic_blocks.iter_enumerated() {
        if bb.is_cleanup {
            continue;
        }
        mir_to_emitted[mir_idx.as_usize()] = Some(next_emitted);
        next_emitted += 1;
    }

    LowerPlan {
        resolver,
        mir_to_emitted,
        switch_analysis,
        call_feed_locals,
        defined,
        suppressed_sentinel_names,
        suppressed_dest_sentinels,
    }
}

fn stage_emit_draft<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    returns_unit: bool,
    plan: &mut LowerPlan,
) -> mir_passes::BodyDraft {
    let emitted_block_capacity = plan
        .mir_to_emitted
        .iter()
        .filter(|slot| slot.is_some())
        .count();
    let mut emitted_blocks: Vec<mir_passes::EmittedBlock> =
        Vec::with_capacity(emitted_block_capacity);

    for (mir_idx, bb) in body.basic_blocks.iter_enumerated() {
        if bb.is_cleanup {
            continue;
        }
        if let Some(block) = mir_passes::emit_special_block(
            returns_unit,
            mir_idx.as_usize(),
            &emitted_blocks,
            &plan.switch_analysis,
            &mut plan.defined,
        ) {
            emitted_blocks.push(block);
            continue;
        }

        let mut stmts = stage_prepare_block_stmts();
        stage_lower_block_statements(tcx, body, bb, &emitted_blocks, plan, &mut stmts);
        let term = stage_lower_block_terminator(
            tcx,
            returns_unit,
            bb,
            &emitted_blocks,
            plan,
            &mut stmts,
        );
        emitted_blocks.push(mir_passes::make_normal_block(stmts, term));
    }

    mir_passes::make_body_draft(
        emitted_blocks,
        std::mem::take(&mut plan.suppressed_dest_sentinels),
    )
}

fn stage_normalize_draft(draft: mir_passes::BodyDraft) -> Vec<BasicBlock> {
    mir_passes::NormalizationPipeline::canonical().run(draft)
}

fn stage_prepare_block_stmts() -> Vec<Stmt> {
    Vec::new()
}

fn stage_lower_block_statements<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    bb: &mir::BasicBlockData<'tcx>,
    blocks: &[mir_passes::EmittedBlock],
    plan: &mut LowerPlan,
    stmts: &mut Vec<Stmt>,
) {
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
    tcx: TyCtxt<'tcx>,
    returns_unit: bool,
    bb: &mir::BasicBlockData<'tcx>,
    blocks: &[mir_passes::EmittedBlock],
    plan: &mut LowerPlan,
    stmts: &mut Vec<Stmt>,
) -> Terminator {
    let has_match_dest = mir_passes::blocks_have_ret_match(blocks) || stmts_have_ret_match(stmts);
    let has_ret_binding =
        mir_passes::blocks_have_ret_binding(blocks) || stmts_have_ret_binding(stmts);

    let Some(term_ref) = &bb.terminator else {
        return Terminator::None;
    };

    if let mir::TerminatorKind::Call {
        func,
        args,
        destination,
        target,
        ..
    } = &term_ref.kind
    {
        mir_terminator::lower_call_terminator(
            tcx,
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
        )
    } else {
        mir_terminator::lower_non_call_terminator(
            tcx,
            term_ref,
            returns_unit,
            &plan.resolver,
            &plan.mir_to_emitted,
            stmts,
            &mut plan.defined,
            has_ret_binding,
            has_match_dest,
        )
    }
}

fn stage_finalize_body(blocks: Vec<BasicBlock>) -> Body {
    Body::Blocks(blocks)
}

fn lower_assign_statement<'tcx>(stmt: &mir::Statement<'tcx>, ctx: &mut AssignLowerCtx<'_, 'tcx>) {
    let mir::StatementKind::Assign(boxed) = &stmt.kind else {
        return;
    };
    let (lhs, rvalue) = &**boxed;
    let lhs_name = ctx.resolver.label_place(lhs);

    match mir_patterns::dispatch_stmt_pattern(ctx.tcx, rvalue) {
        MirOpKind::FieldAccess => {
            if let Some(field_stmt) =
                mir_expr::mir_field_access_stmt(ctx.tcx, ctx.local_decls, lhs, rvalue, ctx.resolver)
            {
                if !mir_guard::structural_guard(
                    &field_stmt,
                    ctx.defined,
                    ctx.suppressed_sentinel_names,
                ) {
                    if let Stmt::FieldAccess { dest: Some(dest), .. } = &field_stmt {
                        mir_util::emit_suppressed_for_name(
                            dest,
                            ctx.stmts,
                            ctx.defined,
                            ctx.suppressed_sentinel_names,
                        );
                    }
                    return;
                }
                if let Stmt::FieldAccess { dest: Some(dest), .. } = &field_stmt {
                    ctx.defined.insert(dest.clone());
                }
                ctx.stmts.push(field_stmt);
                return;
            }
            if let Some(lhs_name) = ctx.resolver.label_place(lhs) {
                ctx.defined.insert(lhs_name);
            }
            return;
        }
        MirOpKind::StructLit => {
            if let Some(struct_stmt) = mir_expr::mir_struct_lit_stmt(ctx.tcx, lhs, rvalue, ctx.resolver)
            {
                if !mir_guard::structural_guard(
                    &struct_stmt,
                    ctx.defined,
                    ctx.suppressed_sentinel_names,
                ) {
                    if let Stmt::StructLit { dest: Some(dest), .. } = &struct_stmt {
                        mir_util::emit_suppressed_for_name(
                            dest,
                            ctx.stmts,
                            ctx.defined,
                            ctx.suppressed_sentinel_names,
                        );
                    }
                    return;
                }
                if let Stmt::StructLit { dest: Some(dest), .. } = &struct_stmt {
                    ctx.defined.insert(dest.clone());
                }
                ctx.stmts.push(struct_stmt);
            }
            return;
        }
        MirOpKind::OpaqueAggregate => {
            if let Some(lhs_name) = lhs_name.clone() {
                mir_util::emit_suppressed_for_name(
                    &lhs_name,
                    ctx.stmts,
                    ctx.defined,
                    ctx.suppressed_sentinel_names,
                );
            }
            return;
        }
        MirOpKind::ZeroArgEnumCtor => {
            if let Some(lhs_name) = lhs_name.clone() {
                mir_util::emit_suppressed_for_name(
                    &lhs_name,
                    ctx.stmts,
                    ctx.defined,
                    ctx.suppressed_sentinel_names,
                );
            }
            return;
        }
        MirOpKind::ConstUse => {}
        MirOpKind::ArrayAggregate => {
            if let Some(lhs_name) = lhs_name.as_ref() && ctx.defined.contains(lhs_name) {
                return;
            }
        }
        MirOpKind::Assign => {}
    }

    if let Some(assign_stmt) = mir_expr::mir_assign_stmt(
        ctx.tcx,
        ctx.local_decls,
        lhs,
        rvalue,
        ctx.resolver,
        ctx.defined,
        ctx.suppressed_sentinel_names,
    ) {
        if !mir_guard::structural_guard(&assign_stmt, ctx.defined, ctx.suppressed_sentinel_names) {
            if let Stmt::Assign { lhs, .. } = &assign_stmt {
                mir_util::emit_suppressed_for_name(
                    lhs,
                    ctx.stmts,
                    ctx.defined,
                    ctx.suppressed_sentinel_names,
                );
            }
            return;
        }
        if let Stmt::Assign { lhs, .. } = &assign_stmt {
            ctx.defined.insert(lhs.clone());
        }
        ctx.stmts.push(assign_stmt);
    } else if let Some(lhs_name) = lhs_name {
        mir_util::emit_suppressed_for_name(
            &lhs_name,
            ctx.stmts,
            ctx.defined,
            ctx.suppressed_sentinel_names,
        );
    }
}

fn stmts_have_ret_match(stmts: &[Stmt]) -> bool {
    stmts
        .iter()
        .any(|stmt| matches!(stmt, Stmt::Match { dest: Some(dest) } if dest == "__ret"))
}

fn stmts_have_ret_binding(stmts: &[Stmt]) -> bool {
    stmts.iter().any(mir_util::stmt_defines_ret)
}
