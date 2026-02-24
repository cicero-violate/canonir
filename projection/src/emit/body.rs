use model::ir::node::{BasicBlock, Stmt, Terminator};

pub fn emit_blocks(blocks: &[BasicBlock], pad: &str) -> String {
    if blocks.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let mut visited = vec![false; blocks.len()];
    emit_block_rec(blocks, 0, pad, &mut visited, &mut out);
    out
}

fn emit_block_rec(
    blocks: &[BasicBlock],
    idx: usize,
    pad: &str,
    visited: &mut Vec<bool>,
    out: &mut String,
) {
    if idx >= blocks.len() || visited[idx] {
        return;
    }
    visited[idx] = true;
    let bb = &blocks[idx];
    for stmt in &bb.stmts {
        out.push_str(&emit_stmt(stmt, pad));
    }
    match &bb.terminator {
        Terminator::Goto(t) => {
            emit_block_rec(blocks, *t as usize, pad, visited, out);
        }
        Terminator::Branch { cond, true_bb, false_bb } => {
            let inner = format!("{}    ", pad);
            out.push_str(&format!("{}if {} {{\n", pad, cond));
            emit_block_rec(blocks, *true_bb as usize, &inner, visited, out);
            out.push_str(&format!("{}}} else {{\n", pad));
            emit_block_rec(blocks, *false_bb as usize, &inner, visited, out);
            out.push_str(&format!("{}}}\n", pad));
        }
        Terminator::Return | Terminator::None => {}
    }
}

fn emit_stmt(stmt: &Stmt, pad: &str) -> String {
    match stmt {
        Stmt::Let { pat, ty, init } => {
            let ty_s = ty.as_deref().map(|t| format!(": {}", t)).unwrap_or_default();
            let init_s = init.as_deref().map(|e| format!(" = {}", e)).unwrap_or_default();
            format!("{}let {}{}{};\n", pad, pat, ty_s, init_s)
        }
        Stmt::Expr(e) => format!("{}{};\n", pad, e),
        Stmt::Return(None) => format!("{}return;\n", pad),
        Stmt::Return(Some(e)) => format!("{}return {};\n", pad, e),
        Stmt::Raw(s) => format!("{}{}\n", pad, s),
    }
}

/// Indent every line of a raw body string by `pad`.
pub fn indent_raw(src: &str, pad: &str) -> String {
    src.lines().map(|line| format!("{}{}\n", pad, line)).collect()
}

