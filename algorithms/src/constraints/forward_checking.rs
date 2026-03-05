use super::ac3::GpuArcConstraints;
use super::ac3::{ConstraintGraph, Domain};

/// Forward checking for a partial assignment.
///
/// `assignment[i] = Some(value)` if assigned, otherwise None.
/// Returns pruned domains or None if inconsistency is detected.
pub fn forward_check(domains: &[Domain], assignment: &[Option<i32>], graph: &ConstraintGraph) -> Option<Vec<Domain>> {
    if domains.len() != assignment.len() {
        return None;
    }
    let mut pruned = domains.to_vec();
    for i in 0..assignment.len() {
        if let Some(val) = assignment[i] {
            pruned[i] = vec![val];
            for j in 0..assignment.len() {
                if i == j {
                    continue;
                }
                if let Some(pred) = graph.constraint(i, j) {
                    pruned[j].retain(|&v| pred(val, v));
                    if pruned[j].is_empty() {
                        return None;
                    }
                }
            }
        }
    }
    Some(pruned)
}

/// Build GPU buffers and apply current assignment to prune domains.
pub fn forward_check_gpu_build(domains: &[Domain], assignment: &[Option<i32>], graph: &ConstraintGraph) -> Option<GpuArcConstraints> {
    if domains.len() != assignment.len() {
        return None;
    }
    let mut gpu = GpuArcConstraints::new(domains, graph);
    let mut assign_idx = vec![-1i32; assignment.len()];
    for (i, a) in assignment.iter().enumerate() {
        if let Some(val) = a {
            let dom = &domains[i];
            let pos = dom.iter().position(|v| v == val)?;
            assign_idx[i] = pos as i32;
        }
    }
    #[cfg(feature = "cuda")]
    {
        let _changed = super::forward_checking::forward_check_gpu(
            assignment.len(),
            &gpu.domain_offsets,
            &mut gpu.domain_active,
            &assign_idx,
            &gpu.arc_i,
            &gpu.arc_j,
            &gpu.arc_dom_i_len,
            &gpu.arc_dom_j_len,
            &gpu.arc_constraint_offset,
            &gpu.constraint_values,
        );
    }
    #[cfg(not(feature = "cuda"))]
    {
        let _ = assign_idx;
    }
    if gpu.validate().is_err() {
        return None;
    }
    Some(gpu)
}

#[cfg(feature = "cuda")]
unsafe extern "C" {
    fn gpu_forward_check(
        var_count: i32, domain_offsets: *const i32, domain_active: *mut i32, assignment: *const i32, arc_count: i32, arc_i: *const i32, arc_j: *const i32, arc_dom_i_len: *const i32,
        arc_dom_j_len: *const i32, arc_constraint_offset: *const i32, constraint_values: *const u8,
    ) -> i32;
}

/// GPU forward checking over arc constraints with dense per-arc constraint matrices.
/// Domains are represented by index (0..len-1) and tracked via domain_active.
#[cfg(feature = "cuda")]
pub fn forward_check_gpu(
    var_count: usize, domain_offsets: &[i32], domain_active: &mut [i32], assignment: &[i32], arc_i: &[i32], arc_j: &[i32], arc_dom_i_len: &[i32], arc_dom_j_len: &[i32], arc_constraint_offset: &[i32],
    constraint_values: &[u8],
) -> bool {
    unsafe {
        gpu_forward_check(
            var_count as i32,
            domain_offsets.as_ptr(),
            domain_active.as_mut_ptr(),
            assignment.as_ptr(),
            arc_i.len() as i32,
            arc_i.as_ptr(),
            arc_j.as_ptr(),
            arc_dom_i_len.as_ptr(),
            arc_dom_j_len.as_ptr(),
            arc_constraint_offset.as_ptr(),
            constraint_values.as_ptr(),
        ) != 0
    }
}
