/// Generate a serializable event struct with standard derives.
/// Supports optional per-field attributes (e.g. `#[serde(default)]`).
#[macro_export]
macro_rules! canon_event_struct {
    ($name:ident { $($(#[$meta:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
        pub struct $name {
            $($(#[$meta])* pub $field: $ty),*
        }
    };
}

/// Generate an event enum where each variant wraps its named inner type.
/// Optional extra derives can be passed as attributes before the enum name.
///
/// # Examples
/// ```rust,ignore
/// // No extra derives:
/// canon_event_enum!(MyEvent { Foo(Foo), Bar(Bar) });
///
/// // With serde:
/// canon_event_enum!(#[derive(serde::Serialize, serde::Deserialize)] MyEvent { Foo(Foo) });
/// ```
#[macro_export]
macro_rules! canon_event_enum {
    ($(#[$($attr:tt)*])* $enum_name:ident { $($variant:ident($inner:ty)),* $(,)? }) => {
        #[derive(Debug, Clone)]
        $(#[$($attr)*])*
        pub enum $enum_name {
            $($variant($inner)),*
        }

        impl $enum_name {
            /// Returns one sample of each variant using Default inner values.
            pub fn sample_all() -> Vec<Self>
            where
                $($inner: Default),*
            {
                vec![
                    $(Self::$variant(<$inner>::default())),*
                ]
            }
        }
    };
}

/// Emit a canonical event.
///
/// Two forms:
///
/// **Emitter-routed form** (consumers/handlers — never touches disk):
/// ```rust,ignore
/// canon_emit!(emitter; "source", "kind", payload);
/// ```
/// Routes through `emitter.emit(RuntimeEvent::Debug(...))` → EventRuntime → canonical writer.
///
/// **Direct form** (external processes — supervisor, tools, smoke tests):
/// ```rust,ignore
/// canon_emit!("source", "kind", payload, &tlog_path)?;
/// ```
/// Writes directly to tlog via `write_event_auto`.
#[macro_export]
macro_rules! canon_emit {
    // Typed variant form: canon_emit!(emitter; LoopPlanned(payload))
    ($emitter:expr; $variant:ident($inner:expr)) => {{
        $emitter.emit(canon_event::RuntimeEvent::$variant($inner))
    }};
    // Emitter-routed form: routes through EventRuntime → canonical writer
    ($emitter:expr; $source:expr, $kind:expr, $payload:expr) => {{
        let __wrapped = serde_json::json!({
            "meta": {
                "file": file!(),
                "line": line!(),
                "module": module_path!(),
                "crate_name": env!("CARGO_PKG_NAME"),
            },
            "data": $payload,
        });
        $emitter.emit(canon_event::RuntimeEvent::Debug(canon_event::DebugEvent { source: $source.to_string(), kind: $kind.to_string(), payload: __wrapped }))
    }};
    // Direct form: writes directly to tlog path (external processes only)
    ($source:expr, $kind:expr, $payload:expr, $path:expr) => {{
        let __wrapped = serde_json::json!({
            "meta": {
                "file": file!(),
                "line": line!(),
                "module": module_path!(),
                "crate_name": env!("CARGO_PKG_NAME"),
            },
            "data": $payload,
        });
        let __event = canon_event::TlogEvent::new($source, $kind, __wrapped);
        canon_event::write_event_auto($path, &__event)
    }};
}
