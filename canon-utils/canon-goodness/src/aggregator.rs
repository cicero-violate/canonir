use crate::Metrics;

pub fn normalize(obs: f32, target: f32) -> f32 {
    if target <= 0.0 {
        return 0.0;
    }
    (obs / target).clamp(0.0, 1.0)
}

pub fn compute_g(m: &Metrics) -> f32 {
    let arr = m.as_array();
    let product: f32 = arr.iter().map(|&x| x.max(0.01)).product();
    product.powf(1.0 / arr.len() as f32)
}

pub fn compute_reward(g_now: f32, g_prev: f32) -> f32 {
    g_now - g_prev
}

