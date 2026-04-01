// TRACE: global runtime introspection (file, line, function)

#[macro_export]
macro_rules! trace {
    ($msg:expr) => {
        // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
        eprintln!(
            "[TRACE] {}:{} {} - {}",
            file!(),
            line!(),
            module_path!(),
            $msg
        );
    };
}
