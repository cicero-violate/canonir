use crate::data_structures::union_find::UnionFind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EqualityConstraint {
    pub left: u32,
    pub right: u32,
}

impl EqualityConstraint {
    pub fn new(left: u32, right: u32) -> Self {
        Self { left, right }
    }
}

pub fn solve_constraints_cpu(var_count: usize, constraints: &[EqualityConstraint]) -> Result<Vec<u32>, String> {
    validate_constraints(var_count, constraints)?;
    let mut uf = UnionFind::new(var_count);
    for c in constraints {
        uf.union(c.left as usize, c.right as usize);
    }
    Ok(uf.labels().into_iter().map(|x| x as u32).collect())
}

#[cfg(feature = "cuda")]
unsafe extern "C" {
    fn gpu_union_find_solve(var_count: i32, edge_count: i32, edge_u: *const i32, edge_v: *const i32, parent_out: *mut i32) -> i32;
}

#[cfg(feature = "cuda")]
pub fn solve_constraints_gpu(var_count: usize, constraints: &[EqualityConstraint]) -> Result<Vec<u32>, String> {
    validate_constraints(var_count, constraints)?;

    let mut edge_u = Vec::with_capacity(constraints.len());
    let mut edge_v = Vec::with_capacity(constraints.len());
    for c in constraints {
        edge_u.push(c.left as i32);
        edge_v.push(c.right as i32);
    }

    let mut parents = vec![-1i32; var_count];
    let ok = unsafe {
        gpu_union_find_solve(
            var_count as i32,
            constraints.len() as i32,
            edge_u.as_ptr(),
            edge_v.as_ptr(),
            parents.as_mut_ptr(),
        )
    };

    if ok == 0 {
        return Err("gpu_union_find_solve failed".into());
    }

    normalize_parent_roots(&mut parents);
    Ok(parents.into_iter().map(|p| p as u32).collect())
}

pub fn solve_constraints_auto(var_count: usize, constraints: &[EqualityConstraint]) -> Result<Vec<u32>, String> {
    #[cfg(feature = "cuda")]
    {
        if let Ok(out) = solve_constraints_gpu(var_count, constraints) {
            return Ok(out);
        }
    }

    solve_constraints_cpu(var_count, constraints)
}

fn validate_constraints(var_count: usize, constraints: &[EqualityConstraint]) -> Result<(), String> {
    for (i, c) in constraints.iter().enumerate() {
        if c.left as usize >= var_count || c.right as usize >= var_count {
            return Err(format!("constraint {i} out of range: ({}, {}) with var_count={var_count}", c.left, c.right));
        }
    }
    Ok(())
}

fn normalize_parent_roots(parents: &mut [i32]) {
    for i in 0..parents.len() {
        if parents[i] < 0 {
            parents[i] = i as i32;
        }
    }

    for i in 0..parents.len() {
        let mut x = i;
        while (parents[x] as usize) != x {
            x = parents[x] as usize;
        }
        let root = x as i32;

        let mut y = i;
        while (parents[y] as usize) != y {
            let next = parents[y] as usize;
            parents[y] = root;
            y = next;
        }
        parents[i] = root;
    }
}

#[cfg(test)]
mod tests {
    use super::{solve_constraints_cpu, EqualityConstraint};

    #[test]
    fn cpu_unification_merges_connected_components() {
        let constraints = vec![
            EqualityConstraint::new(0, 1),
            EqualityConstraint::new(1, 2),
            EqualityConstraint::new(4, 5),
        ];
        let out = solve_constraints_cpu(6, &constraints).unwrap();
        assert_eq!(out[0], out[1]);
        assert_eq!(out[1], out[2]);
        assert_eq!(out[4], out[5]);
        assert_ne!(out[0], out[4]);
        assert_ne!(out[3], out[0]);
    }

    #[test]
    fn cpu_unification_rejects_invalid_constraint() {
        let constraints = vec![EqualityConstraint::new(0, 9)];
        let err = solve_constraints_cpu(4, &constraints).unwrap_err();
        assert!(err.contains("out of range"));
    }
}
