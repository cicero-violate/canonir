// TRACE: global runtime introspection (file, line, function)

#[macro_export]
macro_rules! trace {
    ($msg:expr) => {
        #[cfg(feature = "trace")]
        {
            eprintln!(
                "[TRACE] {}:{} {} - {}",
                file!(),
                line!(),
                module_path!(),
                $msg
            );
        }
    };
}

