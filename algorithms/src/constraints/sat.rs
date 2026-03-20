use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Lit {
    pub var: usize,
    pub negated: bool,
}

impl Lit {
    pub fn pos(var: usize) -> Self {
        Self { var, negated: false }
    }

    pub fn neg(var: usize) -> Self {
        Self { var, negated: true }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Cnf {
    pub num_vars: usize,
    pub clauses: Vec<Vec<Lit>>,
}

impl Cnf {
    pub fn new(num_vars: usize) -> Self {
        Self { num_vars, clauses: Vec::new() }
    }

    pub fn add_clause(&mut self, clause: Vec<Lit>) {
        self.clauses.push(clause);
    }
}

/// Solve CNF with a mature backend first (`z3` CLI), then fallback to local DPLL.
pub fn solve(cnf: &Cnf) -> Option<Vec<bool>> {
    solve_with_z3_cli(cnf).or_else(|| solve_with_dpll(cnf))
}

pub fn is_partial_consistent(cnf: &Cnf, assignment: &[Option<bool>]) -> bool {
    cnf.clauses.iter().all(|clause| {
        let mut has_true = false;
        let mut has_unassigned = false;
        for lit in clause {
            match assignment.get(lit.var).and_then(|v| *v) {
                Some(value) => {
                    let lit_value = if lit.negated { !value } else { value };
                    if lit_value {
                        has_true = true;
                        break;
                    }
                }
                None => has_unassigned = true,
            }
        }
        has_true || has_unassigned
    })
}

fn solve_with_dpll(cnf: &Cnf) -> Option<Vec<bool>> {
    let mut assignment: Vec<Option<bool>> = vec![None; cnf.num_vars];
    if dpll(cnf, &mut assignment) {
        Some(assignment.into_iter().map(|v| v.unwrap_or(false)).collect())
    } else {
        None
    }
}

fn dpll(cnf: &Cnf, assignment: &mut [Option<bool>]) -> bool {
    if !is_partial_consistent(cnf, assignment) {
        return false;
    }
    if assignment.iter().all(Option::is_some) {
        return true;
    }

    let var = assignment.iter().position(|v| v.is_none()).expect("some variable must be unassigned");

    assignment[var] = Some(true);
    if dpll(cnf, assignment) {
        return true;
    }

    assignment[var] = Some(false);
    if dpll(cnf, assignment) {
        return true;
    }

    assignment[var] = None;
    false
}

fn solve_with_z3_cli(cnf: &Cnf) -> Option<Vec<bool>> {
    let mut child = Command::new("z3").args(["-in", "-smt2"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;

    {
        let stdin = child.stdin.as_mut()?;
        stdin.write_all(build_smt2(cnf).as_bytes()).ok()?;
    }

    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    parse_z3_model(&text, cnf.num_vars)
}

fn build_smt2(cnf: &Cnf) -> String {
    let mut s = String::new();
    s.push_str("(set-logic QF_BOOL)\n");
    for i in 0..cnf.num_vars {
        s.push_str(&format!("(declare-fun x{i} () Bool)\n"));
    }
    for clause in &cnf.clauses {
        if clause.is_empty() {
            s.push_str("(assert false)\n");
            continue;
        }
        s.push_str("(assert (or");
        for lit in clause {
            if lit.negated {
                s.push_str(&format!(" (not x{})", lit.var));
            } else {
                s.push_str(&format!(" x{}", lit.var));
            }
        }
        s.push_str("))\n");
    }
    s.push_str("(check-sat)\n(get-model)\n");
    s
}

fn parse_z3_model(out: &str, num_vars: usize) -> Option<Vec<bool>> {
    let mut lines = out.lines();
    let status = lines.next()?.trim();
    if status == "unsat" {
        return None;
    }
    if status != "sat" {
        return None;
    }

    let mut model = vec![false; num_vars];
    let all = out.replace('\n', " ");
    for (i, slot) in model.iter_mut().enumerate().take(num_vars) {
        let needle = format!("(define-fun x{i} () Bool ");
        if let Some(pos) = all.find(&needle) {
            let rest = &all[pos + needle.len()..];
            *slot = rest.starts_with("true");
        }
    }
    Some(model)
}
