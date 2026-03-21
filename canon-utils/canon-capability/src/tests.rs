#[cfg(test)]
mod tests {
    use crate::registry::CapabilityRegistry;
    #[test]
    fn all_capability_routes_are_safe() {
        use canon_tools_editor::capabilities::register_editor_capabilities;

        let mut registry = CapabilityRegistry::new();
        register_editor_capabilities(&mut registry);
        canon_introspection::assert_all_routes_safe(&registry);
    }
}
