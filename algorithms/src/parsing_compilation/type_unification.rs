use crate::constraints::unification::{solve_constraints_auto, solve_constraints_cpu, EqualityConstraint};

#[derive(Debug, Clone, Default)]
pub struct TypeConstraintGraph {
    pub var_count: u32,
    pub constraints: Vec<EqualityConstraint>,
}

impl TypeConstraintGraph {
    pub fn new(var_count: u32) -> Self {
        Self { var_count, constraints: Vec::new() }
    }

    pub fn from_equalities(var_count: u32, edges: &[(u32, u32)]) -> Result<Self, String> {
        let mut g = Self::new(var_count);
        for &(left, right) in edges {
            g.add_equality(left, right)?;
        }
        Ok(g)
    }

    pub fn add_equality(&mut self, left: u32, right: u32) -> Result<(), String> {
        if left >= self.var_count || right >= self.var_count {
            return Err(format!(
                "type variable out of range: ({left}, {right}) with var_count={}",
                self.var_count
            ));
        }
        self.constraints.push(EqualityConstraint::new(left, right));
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.constraints.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeUnificationResult {
    pub representative: Vec<u32>,
}

impl TypeUnificationResult {
    pub fn representative_of(&self, var: u32) -> Option<u32> {
        self.representative.get(var as usize).copied()
    }

    pub fn equivalent(&self, a: u32, b: u32) -> bool {
        self.representative_of(a) == self.representative_of(b)
    }
}

pub fn unify_types_cpu(graph: &TypeConstraintGraph) -> Result<TypeUnificationResult, String> {
    let representative = solve_constraints_cpu(graph.var_count as usize, &graph.constraints)?;
    Ok(TypeUnificationResult { representative })
}

#[cfg(feature = "cuda")]
pub fn unify_types_gpu(graph: &TypeConstraintGraph) -> Result<TypeUnificationResult, String> {
    let representative = crate::constraints::unification::solve_constraints_gpu(graph.var_count as usize, &graph.constraints)?;
    Ok(TypeUnificationResult { representative })
}

pub fn unify_types_auto(graph: &TypeConstraintGraph) -> Result<TypeUnificationResult, String> {
    let representative = solve_constraints_auto(graph.var_count as usize, &graph.constraints)?;
    Ok(TypeUnificationResult { representative })
}

#[cfg(test)]
mod tests {
    use super::{unify_types_auto, unify_types_cpu, TypeConstraintGraph};

    #[test]
    fn type_unification_cpu_connects_equalities() {
        let mut g = TypeConstraintGraph::new(7);
        g.add_equality(0, 1).unwrap();
        g.add_equality(1, 2).unwrap();
        g.add_equality(3, 4).unwrap();
        g.add_equality(5, 6).unwrap();

        let out = unify_types_cpu(&g).unwrap();
        assert!(out.equivalent(0, 2));
        assert!(out.equivalent(3, 4));
        assert!(out.equivalent(5, 6));
        assert!(!out.equivalent(0, 3));
    }

    #[test]
    fn type_unification_auto_matches_cpu_shape() {
        let g = TypeConstraintGraph::from_equalities(5, &[(0, 1), (1, 2), (3, 4)]).unwrap();
        let cpu = unify_types_cpu(&g).unwrap();
        let auto = unify_types_auto(&g).unwrap();

        assert_eq!(cpu.representative.len(), auto.representative.len());
        assert_eq!(cpu.equivalent(0, 2), auto.equivalent(0, 2));
        assert_eq!(cpu.equivalent(3, 4), auto.equivalent(3, 4));
        assert_eq!(cpu.equivalent(0, 3), auto.equivalent(0, 3));
    }
}
