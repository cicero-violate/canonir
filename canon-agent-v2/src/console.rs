use super::capability::CapabilityMode;
const RESET: &str = "\x1b[0m";
const GRAY: &str = "\x1b[90m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const BLUE: &str = "\x1b[34m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";
pub fn console_ui_tag(label: &str, color: &str) -> String {
    format!("{}[{}]{}", color, label, RESET)
}
pub fn console_ui_note(label: &str, msg: &str) -> String {
    format!("{} {}", console_ui_tag(label, BLUE), msg)
}
pub fn console_ui_warn(label: &str, msg: &str) -> String {
    format!("{} {}", console_ui_tag(label, YELLOW), msg)
}
pub fn console_ui_err(label: &str, msg: &str) -> String {
    format!("{} {}", console_ui_tag(label, RED), msg)
}
pub fn console_ui_phase(label: &str, msg: &str) -> String {
    format!("{} {}", console_ui_tag(label, MAGENTA), msg)
}
pub fn console_ui_llm(msg: &str) -> String {
    format!("{} {}", console_ui_tag("llm", CYAN), msg)
}
pub fn console_ui_mode_label(class: CapabilityMode) -> (&'static str, &'static str) {
    match class {
        CapabilityMode::Observe => ("Observe", GREEN),
        CapabilityMode::Verify => ("Verify", YELLOW),
        CapabilityMode::Mutate => ("Mutate", RED),
    }
}
pub fn console_ui_mode_tag(class: CapabilityMode) -> String {
    let (label, color) = console_ui_mode_label(class);
    console_ui_tag(label, color)
}
pub fn console_ui_dim(msg: &str) -> String {
    format!("{}{}{}", GRAY, msg, RESET)
}
pub fn console_ui_truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut out = s.chars().take(max).collect::<String>();
    out.push_str("…");
    out
}
