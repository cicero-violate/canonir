pub fn helper_function(id: usize) -> String {
    format!("Tool output {}", id)
}

pub fn generate_bulk() -> Vec<String> {
    let mut v = Vec::new();
    for i in 0..500 {
        v.push(helper_function(i));
    }
    v
}
