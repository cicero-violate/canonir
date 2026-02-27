use std::collections::HashSet;

use crate::capture::mir::analysis::SwitchAnalysis;
use crate::capture::mir::util as mir_util;
use crate::types::{BasicBlock, Stmt, Terminator};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockRole {
    SwitchSource,
    SwitchArm,
    Normal,
}

pub(crate) struct EmittedBlock {
    pub(crate) role: BlockRole,
    pub(crate) block: BasicBlock,
}

pub(crate) fn emit_special_block(
    returns_unit: bool,
    mir_idx_usize: usize,
    blocks: &[EmittedBlock],
    switch_analysis: &SwitchAnalysis,
    defined: &mut HashSet<String>,
) -> Option<EmittedBlock> {
    if switch_analysis.switch_sources.contains(&mir_idx_usize) {
        let writes_ret = switch_analysis
            .switch_source_writes_ret
            .get(&mir_idx_usize)
            .copied()
            .unwrap_or(false);
        let dest = if !returns_unit && writes_ret && blocks_have_ret_match(blocks) == false {
            defined.insert("__ret".to_string());
            Some("__ret".to_string())
        } else {
            None
        };
        return Some(EmittedBlock {
            role: BlockRole::SwitchSource,
            block: BasicBlock {
                stmts: vec![Stmt::Match { dest }],
                terminator: Terminator::Unreachable,
            },
        });
    }
    if switch_analysis.switchint_arm_blocks.contains(&mir_idx_usize) {
        return Some(EmittedBlock {
            role: BlockRole::SwitchArm,
            block: BasicBlock {
                stmts: Vec::new(),
                terminator: Terminator::Unreachable,
            },
        });
    }
    None
}

pub(crate) fn normalize_blocks(
    emitted: Vec<EmittedBlock>,
    suppressed_dest_sentinels: Vec<Stmt>,
) -> Vec<BasicBlock> {
    let emitted = pass_inject_suppressed_prologue(emitted, suppressed_dest_sentinels);
    pass_strip_roles(emitted)
}

pub(crate) fn make_normal_block(stmts: Vec<Stmt>, term: Terminator) -> EmittedBlock {
    EmittedBlock {
        role: BlockRole::Normal,
        block: BasicBlock {
            stmts,
            terminator: term,
        },
    }
}

pub(crate) fn blocks_have_ret_match(blocks: &[EmittedBlock]) -> bool {
    blocks.iter().any(|bb| {
        bb.block
            .stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::Match { dest: Some(dest) } if dest == "__ret"))
    })
}

pub(crate) fn blocks_have_ret_binding(blocks: &[EmittedBlock]) -> bool {
    blocks
        .iter()
        .any(|bb| bb.block.stmts.iter().any(mir_util::stmt_defines_ret))
}

fn pass_inject_suppressed_prologue(
    mut emitted: Vec<EmittedBlock>,
    suppressed_dest_sentinels: Vec<Stmt>,
) -> Vec<EmittedBlock> {
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
