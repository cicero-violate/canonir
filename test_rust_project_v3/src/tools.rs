pub fn echo_tool(input: &str) -> String {
    format!("Echo: {}", input)
}

pub fn uppercase_tool(input: &str) -> String {
    input.to_uppercase()
}

pub fn reverse_tool(input: &str) -> String {
    input.chars().rev().collect()
}
