//! FFI bridge to optimization/genetic_algorithm.cu via unified libgpu.a
//!
//! Variables:
//!   population : *mut u64  — individual values, length POP
//!   POP        : i32       — population size
//!   GEN        : i32       — number of generations
//!   return     : u64       — best individual after GEN rounds
//!
//! Equation (parallel tournament selection + crossover):
//!   parent_a = max(pop[ia], pop[ib]),  ia,ib ~ Uniform(0,POP)
//!   parent_b = max(pop[ic], pop[id]),  ic,id ~ Uniform(0,POP)
//!   child    = (parent_a + parent_b) / 2
//!   result   = max_i population[i]

#[cfg(feature = "cuda")]
unsafe extern "C" {
    pub fn gpu_genetic_optimize(population: *mut u64, pop: i32, r#gen: i32) -> u64;
}

/// Safe wrapper: evolves population in-place, returns best fitness.
#[cfg(feature = "cuda")]
pub fn genetic_optimize_gpu(population: &mut Vec<u64>, generations: usize) -> u64 {
    unsafe { gpu_genetic_optimize(population.as_mut_ptr(), population.len() as i32, generations as i32) }
}
