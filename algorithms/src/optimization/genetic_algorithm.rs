pub fn genetic_optimize<F>(mut population: Vec<u64>, fitness: F, generations: usize) -> u64
where F: Fn(u64) -> u64 {
    fn lcg(seed: &mut u64) -> u64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        *seed
    }

    let mut seed = 1u64;

    for _ in 0..generations {
        population.sort_by_key(|&x| fitness(x));
        population.truncate(population.len() / 2);

        while population.len() < 100 {
            let i = (lcg(&mut seed) as usize) % population.len();
            let j = (lcg(&mut seed) as usize) % population.len();
            let child = (population[i] + population[j]) / 2;
            population.push(child);
        }
    }

    *population.iter().max_by_key(|&&x| fitness(x)).unwrap()
}
