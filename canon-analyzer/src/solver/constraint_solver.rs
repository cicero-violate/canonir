use algorithms::constraints::sat::{self, Cnf, Lit};
use algorithms::graph::scc::kosaraju_scc;
use algorithms::graph::topological_sort::topological_sort;
use anyhow::Result;
use canon::edge::EdgeKind;
use canon::id::NodeId;
use canon::node::CanonNodeKind;
use canon::CanonIR;

#[derive(Debug, Clone)]
pub struct RenameTransform {
    pub var_id: usize,
    pub src_idx: usize,
    pub dst_idx: usize,
    pub old_name: String,
    pub new_name: String,
}

#[derive(Debug, Clone)]
pub struct RenameConstraintProblem {
    pub transforms: Vec<RenameTransform>,
    pub dependencies: Vec<(usize, usize)>,
    pub conflicts: Vec<(usize, usize)>,
    pub cnf: Cnf,
    pub topo_order: Vec<usize>,
    pub sccs: Vec<Vec<usize>>,
}

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let problem = build_problem(ir);
    let Some(model) = sat::solve(&problem.cnf) else {
        emit_diag(
            ir,
            &format!(
                "constraint_solver:unsat vars={} deps={} conflicts={}",
                problem.transforms.len(),
                problem.dependencies.len(),
                problem.conflicts.len()
            ),
        );
        return Ok(());
    };

    let selected = model.iter().filter(|&&v| v).count();
    emit_diag(
        ir,
        &format!(
            "constraint_solver:sat selected={}/{} deps={} conflicts={}",
            selected,
            problem.transforms.len(),
            problem.dependencies.len(),
            problem.conflicts.len()
        ),
    );
    Ok(())
}

pub fn build_problem(ir: &CanonIR) -> RenameConstraintProblem {
    let transforms = collect_rename_transforms(ir);
    let dependencies = build_dependencies(&transforms);
    let conflicts = build_conflicts(&transforms);
    let cnf = build_cnf(transforms.len(), &dependencies, &conflicts);

    let mut dep_adj = vec![Vec::new(); transforms.len()];
    for &(a, b) in &dependencies {
        dep_adj[a].push(b);
    }
    let topo_order = topological_sort(&dep_adj);
    let sccs = kosaraju_scc(&dep_adj);

    RenameConstraintProblem {
        transforms,
        dependencies,
        conflicts,
        cnf,
        topo_order,
        sccs,
    }
}

pub fn is_subset_valid(problem: &RenameConstraintProblem, subset: &[usize]) -> bool {
    let mut selected = vec![false; problem.transforms.len()];
    for &idx in subset {
        if idx >= selected.len() {
            return false;
        }
        selected[idx] = true;
    }
    is_selected_vector_valid(problem, &selected)
}

pub fn is_selected_vector_valid(problem: &RenameConstraintProblem, selected: &[bool]) -> bool {
    if selected.len() != problem.transforms.len() {
        return false;
    }
    for &(a, b) in &problem.dependencies {
        if selected[b] && !selected[a] {
            return false;
        }
    }
    for &(a, b) in &problem.conflicts {
        if selected[a] && selected[b] {
            return false;
        }
    }
    true
}

fn collect_rename_transforms(ir: &CanonIR) -> Vec<RenameTransform> {
    let mut out = Vec::new();
    for src_idx in 0..ir.name_graph.vertex_count() {
        let src_name = node_name(ir, src_idx);
        let Some(new_name) = src_name else {
            continue;
        };
        for (dst_id, edge) in ir.name_graph.neighbours(NodeId(src_idx as u32)) {
            if *edge != EdgeKind::Renames {
                continue;
            }
            let dst_idx = dst_id.index();
            let Some(old_name) = node_name(ir, dst_idx) else {
                continue;
            };
            let var_id = out.len();
            out.push(RenameTransform {
                var_id,
                src_idx,
                dst_idx,
                old_name,
                new_name: new_name.clone(),
            });
        }
    }
    out
}

fn build_dependencies(transforms: &[RenameTransform]) -> Vec<(usize, usize)> {
    // dep(a,b): if a renames a symbol that is used as source in b, enforce a before b.
    let mut deps = Vec::new();
    for a in transforms {
        for b in transforms {
            if a.var_id == b.var_id {
                continue;
            }
            if a.dst_idx == b.src_idx {
                deps.push((a.var_id, b.var_id));
            }
        }
    }
    deps.sort_unstable();
    deps.dedup();
    deps
}

fn build_conflicts(transforms: &[RenameTransform]) -> Vec<(usize, usize)> {
    let mut conflicts = Vec::new();
    for i in 0..transforms.len() {
        for j in (i + 1)..transforms.len() {
            let a = &transforms[i];
            let b = &transforms[j];
            let same_target = a.dst_idx == b.dst_idx;
            let same_new_name = a.new_name == b.new_name && a.dst_idx != b.dst_idx;
            if same_target || same_new_name {
                conflicts.push((i, j));
            }
        }
    }
    conflicts
}

fn build_cnf(num_vars: usize, dependencies: &[(usize, usize)], conflicts: &[(usize, usize)]) -> Cnf {
    let mut cnf = Cnf::new(num_vars);
    // dep(a,b) => b -> a
    for &(a, b) in dependencies {
        cnf.add_clause(vec![Lit::neg(b), Lit::pos(a)]);
    }
    // conflict(a,b) => !(a && b)
    for &(a, b) in conflicts {
        cnf.add_clause(vec![Lit::neg(a), Lit::neg(b)]);
    }
    cnf
}

fn node_name(ir: &CanonIR, idx: usize) -> Option<String> {
    let kind = &ir.nodes.get(idx)?.kind;
    match kind {
        CanonNodeKind::Struct { name_id, .. }
        | CanonNodeKind::Enum { name_id, .. }
        | CanonNodeKind::Trait { name_id, .. }
        | CanonNodeKind::Fn { name_id, .. }
        | CanonNodeKind::TypeRef { name_id }
        | CanonNodeKind::TypeAlias { name_id, .. }
        | CanonNodeKind::Const { name_id, .. }
        | CanonNodeKind::Static { name_id, .. }
        | CanonNodeKind::ExternCrate { name_id, .. }
        | CanonNodeKind::Lifetime { name_id }
        | CanonNodeKind::GenericParam { name_id, .. }
        | CanonNodeKind::Param { name_id, .. }
        | CanonNodeKind::Variant { name_id, .. } => Some(ir.lookup_name(*name_id).to_string()),
        CanonNodeKind::Use { alias, path_id, .. } => {
            if let Some(a) = alias {
                Some(ir.lookup_name(*a).to_string())
            } else {
                Some(ir.lookup_path(*path_id).to_string())
            }
        }
        _ => None,
    }
}

fn emit_diag(ir: &mut CanonIR, label: &str) {
    let name_id = ir.intern_name(label);
    let id = ir.push_node(CanonNodeKind::TypeRef { name_id });
    ir.emit_order.push(id);
}
