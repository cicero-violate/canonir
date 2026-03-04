use super::capability::CapabilityClass;

const RESET: &str = "\x1b[0m";
const GRAY: &str = "\x1b[90m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const BLUE: &str = "\x1b[34m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";

pub fn tag(label: &str, color: &str) -> String {
    format!("{}[{}]{}", color, label, RESET)
}

pub fn info(label: &str, msg: &str) -> String {
    format!("{} {}", tag(label, BLUE), msg)
}

pub fn warn(label: &str, msg: &str) -> String {
    format!("{} {}", tag(label, YELLOW), msg)
}

pub fn err(label: &str, msg: &str) -> String {
    format!("{} {}", tag(label, RED), msg)
}

pub fn phase(label: &str, msg: &str) -> String {
    format!("{} {}", tag(label, MAGENTA), msg)
}

pub fn llm(msg: &str) -> String {
    format!("{} {}", tag("llm", CYAN), msg)
}

pub fn mode_label(class: CapabilityClass) -> (&'static str, &'static str) {
    match class {
        CapabilityClass::Observe => ("Observe", GREEN),
        CapabilityClass::Verify => ("Verify", YELLOW),
        CapabilityClass::Mutate => ("Mutate", RED),
    }
}

pub fn mode_tag(class: CapabilityClass) -> String {
    let (label, color) = mode_label(class);
    tag(label, color)
}

pub fn dim(msg: &str) -> String {
    format!("{}{}{}", GRAY, msg, RESET)
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut out = s.chars().take(max).collect::<String>();
    out.push_str("…");
    out
}
