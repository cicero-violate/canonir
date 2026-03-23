use std::collections::HashSet;

use crate::capture::pipeline::mir::analysis::SwitchAnalysis;
use crate::capture::pipeline::mir::util as mir_util;
use crate::capture::types::{BasicBlock, Stmt, Terminator};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockRole {
    SwitchSource,
    Normal,
}

pub(crate) struct EmittedBlock {
    pub(crate) role: BlockRole,
    pub(crate) block: BasicBlock,
}

pub(crate) struct BodyDraft {
    pub(crate) emitted_blocks: Vec<EmittedBlock>,
    pub(crate) suppressed_dest_sentinels: Vec<Stmt>,
}

pub(crate) type ProloguePass = fn(Vec<EmittedBlock>, Vec<Stmt>) -> Vec<EmittedBlock>;
pub(crate) type NormalizePass = fn(Vec<EmittedBlock>) -> Vec<EmittedBlock>;
pub(crate) type FinalizePass = fn(Vec<EmittedBlock>) -> Vec<BasicBlock>;

pub(crate) struct NormalizationPipeline {
    prologue: ProloguePass,
    normalize: NormalizePass,
    finalize: FinalizePass,
}

impl NormalizationPipeline {
    pub(crate) fn canonical() -> Self {
        Self { prologue: pass_inject_suppressed_prologue, normalize: pass_lower_match_and_prune_bindings, finalize: pass_strip_roles }
    }

    pub(crate) fn run(&self, draft: BodyDraft) -> Vec<BasicBlock> {
        let emitted = (self.prologue)(draft.emitted_blocks, draft.suppressed_dest_sentinels);
        let emitted = (self.normalize)(emitted);
        (self.finalize)(emitted)
    }
}

pub(crate) fn make_body_draft(emitted_blocks: Vec<EmittedBlock>, suppressed_dest_sentinels: Vec<Stmt>) -> BodyDraft {
    BodyDraft { emitted_blocks, suppressed_dest_sentinels }
}

pub(crate) fn emit_special_block(returns_unit: bool, mir_idx_usize: usize, blocks: &[EmittedBlock], switch_analysis: &SwitchAnalysis, defined: &mut HashSet<String>) -> Option<EmittedBlock> {
    if switch_analysis.switch_sources.contains(&mir_idx_usize) {
        if switch_analysis.iterator_switches.contains_key(&mir_idx_usize) {
            return None;
        }
        let _ = (returns_unit, blocks, defined);
        return Some(EmittedBlock { role: BlockRole::SwitchSource, block: BasicBlock { stmts: Vec::new(), terminator: Terminator::Unreachable } });
    }
    if switch_analysis.switchint_arm_blocks.contains(&mir_idx_usize) {
        // Do not drop switch arm bodies. Switch arm blocks frequently
        // contain the only structural assignments that bind locals
        // (e.g., iterator `child` bindings). Emitting empty arm blocks
        // causes undefined local errors in emitted Rust.
        return None;
    }
    None
}

pub(crate) fn make_normal_block(stmts: Vec<Stmt>, term: Terminator) -> EmittedBlock {
    EmittedBlock { role: BlockRole::Normal, block: BasicBlock { stmts, terminator: term } }
}

pub(crate) fn blocks_have_ret_match(blocks: &[EmittedBlock]) -> bool {
    blocks.iter().any(|bb| bb.block.stmts.iter().any(|stmt| matches!(stmt, Stmt::Match { dest: Some(dest) } if dest == "__ret")))
}

pub(crate) fn blocks_have_ret_binding(blocks: &[EmittedBlock]) -> bool {
    blocks.iter().any(|bb| bb.block.stmts.iter().any(mir_util::stmt_defines_ret))
}

fn pass_inject_suppressed_prologue(mut emitted: Vec<EmittedBlock>, suppressed_dest_sentinels: Vec<Stmt>) -> Vec<EmittedBlock> {
    if suppressed_dest_sentinels.is_empty() {
        return emitted;
    }
    if let Some(first_normal_idx) = emitted.iter().position(|b| b.role == BlockRole::Normal) {
        let mut merged = suppressed_dest_sentinels;
        merged.append(&mut emitted[first_normal_idx].block.stmts);
        emitted[first_normal_idx].block.stmts = merged;
    }
    emitted
}

fn pass_strip_roles(emitted: Vec<EmittedBlock>) -> Vec<BasicBlock> {
    emitted.into_iter().map(|e| e.block).collect()
}

fn pass_lower_match_and_prune_bindings(emitted: Vec<EmittedBlock>) -> Vec<EmittedBlock> {
    let emitted = pass_lower_match_dest_to_suppressed(emitted);
    pass_prune_unused_suppressed_bindings(emitted)
}

fn pass_lower_match_dest_to_suppressed(mut emitted: Vec<EmittedBlock>) -> Vec<EmittedBlock> {
    for block in &mut emitted {
        for stmt in &mut block.block.stmts {
            if let Stmt::Match { dest: Some(dest) } = stmt {
                // Structural invariant: do not introduce suppressed bindings.
                // Instead, lower non-__ret match destinations to a concrete
                // default value so no suppressed placeholder is emitted.
                if dest != "__ret" {
                    // Avoid emitting untyped Default::default() for match
                    // destinations, as this causes downstream type
                    // inference failures. Use unit placeholder; structural
                    // return synthesis handles typed defaults where needed.
                    *stmt = Stmt::Assign { lhs: dest.clone(), rhs: "panic!(\"lowering error: unlowered match dest\")".to_string() };
                }
            }
        }
    }
    emitted
}

fn pass_prune_unused_suppressed_bindings(emitted: Vec<EmittedBlock>) -> Vec<EmittedBlock> {
    // Suppressed bindings are no longer introduced anywhere in the
    // structural pipeline. Pruning logic that reasons about the
    // "__canon_suppressed__" sentinel can cause accidental removal
    // of required assignments if stale sentinels appear. Make this
    // pass a no-op to preserve all structurally lowered bindings.
    emitted
}
