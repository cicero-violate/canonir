use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    bracketed,
    parse::{Parse, ParseBuffer, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    token::{Comma, Paren, Semi},
    visit::Visit,
    Attribute, Expr, ExprMatch, Ident, ItemFn, LitStr, Pat, Path, Token, Type,
    spanned::Spanned,
};

// ---------------------------------------------------------------------
// must_emit
// ---------------------------------------------------------------------

/// Applied to an `on_event` implementation.
/// Fails compilation if the function body contains any `match` expression
/// where one arm is a wildcard (`_`) or lowercase binding pattern at the
/// top level, AND another arm contains a `RuntimeEvent::` path pattern.
#[proc_macro_attribute]
pub fn must_emit(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let mut checker = WildcardChecker { errors: Vec::new() };
    checker.visit_item_fn(&func);
    if !checker.errors.is_empty() {
        let msgs = checker.errors.join("\n");
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            format!(
                "#[must_emit]: wildcard or binding match arms are forbidden \
                 in RuntimeEvent handlers. List every RuntimeEvent variant. \n{msgs}"
            ),
        )
        .to_compile_error()
        .into();
    }
    quote::quote! { #func }.into()
}

struct WildcardChecker {
    errors: Vec<String>,
}

impl<'ast> Visit<'ast> for WildcardChecker {
    fn visit_expr_match(&mut self, node: &'ast ExprMatch) {
        let has_event_arm = node.arms.iter().any(|arm| pattern_is_runtime_event(&arm.pat));
        if has_event_arm {
            for arm in &node.arms {
                if pattern_is_wildcard_or_binding(&arm.pat) {
                    self.errors.push(
                        "  wildcard/binding arm found in RuntimeEvent match — add explicit arms for every variant".to_string(),
                    );
                }
            }
        }
        syn::visit::visit_expr_match(self, node);
    }
}

fn pattern_is_runtime_event(pat: &Pat) -> bool {
    match pat {
        Pat::TupleStruct(ts) => quote::quote!(#ts).to_string().contains("RuntimeEvent"),
        Pat::Or(or) => or.cases.iter().any(pattern_is_runtime_event),
        Pat::Tuple(t) => t.elems.iter().any(pattern_is_runtime_event),
        _ => false,
    }
}

fn pattern_is_wildcard_or_binding(pat: &Pat) -> bool {
    match pat {
        Pat::Wild(_) => true,
        Pat::Ident(i) => {
            i.subpat.is_none()
                && i.by_ref.is_none()
                && !i.ident.to_string().starts_with(|c: char| c.is_uppercase())
        }
        Pat::Or(or) => or.cases.iter().any(pattern_is_wildcard_or_binding),
        _ => false,
    }
}

// ---------------------------------------------------------------------
// canon_event_struct!
// ---------------------------------------------------------------------

/// Determines whether an attribute is a payload slot marker (#[input], #[output], #[delta]).
fn get_slot_name(attr: &Attribute) -> Option<&'static str> {
    if attr.path().is_ident("input") {
        Some("input")
    } else if attr.path().is_ident("output") {
        Some("output")
    } else if attr.path().is_ident("delta") {
        Some("delta")
    } else {
        None
    }
}

struct EventStruct {
    no_input: bool,
    /// When true, generates `impl crate::CanonPayloadShape for Name { ... }`.
    /// Only set this for structs defined within the `canon_event` crate itself.
    impl_shape: bool,
    class: EventClassSpec,
    next: Vec<Ident>,
    name: Ident,
    fields: Punctuated<FieldSpec, Comma>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EventClassSpec {
    Control,
    Effect,
}

struct FieldSpec {
    /// Real field attributes (serde, etc.) — slot attrs are stripped.
    attrs: Vec<Attribute>,
    /// Slot membership: "input", "output", "delta" (a field can be in multiple slots).
    slots: Vec<&'static str>,
    ident: Ident,
    ty: Type,
}

impl Parse for EventStruct {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // Parse optional struct-level attributes: #[no_input], #[impl_shape]
        let struct_attrs = Attribute::parse_outer(input)?;
        let no_input = struct_attrs.iter().any(|a| a.path().is_ident("no_input"));
        let impl_shape = struct_attrs.iter().any(|a| a.path().is_ident("impl_shape"));
        let mut class: Option<EventClassSpec> = None;
        let mut next: Vec<Ident> = Vec::new();
        for attr in &struct_attrs {
            if !attr.path().is_ident("event") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("class") {
                    let value = meta.value()?;
                    let lit: LitStr = value.parse()?;
                    class = Some(match lit.value().as_str() {
                        "Control" => EventClassSpec::Control,
                        "Effect" => EventClassSpec::Effect,
                        other => {
                            return Err(syn::Error::new(
                                lit.span(),
                                format!("invalid event class `{other}`; expected \"Control\" or \"Effect\""),
                            ))
                        }
                    });
                    return Ok(());
                }
                if meta.path.is_ident("next") {
                    let value = meta.value()?;
                    let content;
                    bracketed!(content in value);
                    let items = Punctuated::<Ident, Comma>::parse_terminated(&content)?;
                    next.extend(items.into_iter());
                    return Ok(());
                }
                Err(meta.error("unsupported #[event(...)] key; expected class or next"))
            })?;
        }
        let class = class.ok_or_else(|| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                "canon_event_struct!: missing #[event(class = \"Control\" | \"Effect\", ...)]",
            )
        })?;
        if class == EventClassSpec::Control && next.is_empty() {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "canon_event_struct!: Control events must declare #[event(next = [..])]",
            ));
        }
        if class == EventClassSpec::Effect && !next.is_empty() {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "canon_event_struct!: Effect events must not declare #[event(next = [..])]",
            ));
        }
        let name: Ident = input.parse()?;
        let content;
        syn::braced!(content in input);
        let mut fields = Punctuated::new();
        while !content.is_empty() {
            let field = FieldSpec::parse_field(&content)?;
            fields.push(field);
            if content.peek(Comma) {
                content.parse::<Comma>()?;
            }
        }
        Ok(EventStruct { no_input, impl_shape, class, next, name, fields })
    }
}

impl FieldSpec {
    fn parse_field(input: &ParseBuffer) -> syn::Result<Self> {
        let all_attrs = Attribute::parse_outer(input)?;
        let mut real_attrs = Vec::new();
        let mut slots = Vec::new();
        for attr in all_attrs {
            if let Some(slot) = get_slot_name(&attr) {
                slots.push(slot);
            } else {
                real_attrs.push(attr);
            }
        }
        let ident: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let ty: Type = input.parse()?;
        Ok(FieldSpec { attrs: real_attrs, slots, ident, ty })
    }
}

/// Build a `serde_json::Value::Object(...)` expression from a list of (key_str, field_ident) pairs.
fn build_json_object(pairs: &[(String, &Ident)]) -> TokenStream2 {
    if pairs.is_empty() {
        quote! { serde_json::Value::Object(serde_json::Map::new()) }
    } else {
        let keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
        let idents: Vec<&&Ident> = pairs.iter().map(|(_, i)| i).collect();
        quote! {{
            let mut __obj = serde_json::Map::new();
            #(
                __obj.insert(
                    #keys.to_string(),
                    serde_json::to_value(&self.#idents).unwrap_or(serde_json::Value::Null),
                );
            )*
            serde_json::Value::Object(__obj)
        }}
    }
}

#[proc_macro]
pub fn canon_event_struct(input: TokenStream) -> TokenStream {
    let EventStruct { no_input, impl_shape, class, next, name, fields } = parse_macro_input!(input as EventStruct);

    let field_idents: Vec<_> = fields.iter().map(|f| &f.ident).collect();
    let field_types: Vec<_> = fields.iter().map(|f| &f.ty).collect();
    let field_attrs: Vec<_> = fields.iter().map(|f| &f.attrs).collect();

    // Collect slot-tagged fields
    let input_pairs: Vec<(String, &Ident)> = fields
        .iter()
        .filter(|f| f.slots.contains(&"input"))
        .map(|f| (f.ident.to_string(), &f.ident))
        .collect();
    let output_pairs: Vec<(String, &Ident)> = fields
        .iter()
        .filter(|f| f.slots.contains(&"output"))
        .map(|f| (f.ident.to_string(), &f.ident))
        .collect();
    let delta_pairs: Vec<(String, &Ident)> = fields
        .iter()
        .filter(|f| f.slots.contains(&"delta"))
        .map(|f| (f.ident.to_string(), &f.ident))
        .collect();

    // Enforce: at least one #[output] field — no exceptions.
    if output_pairs.is_empty() {
        return syn::Error::new(
            name.span(),
            format!(
                "canon_event_struct!: `{name}` has no #[output] fields. \
                 Every event must produce observable output. \
                 Add `#[output] success: bool` at minimum."
            ),
        )
        .to_compile_error()
        .into();
    }

    // Enforce: at least one #[input] field unless #[no_input] is declared.
    if !no_input && input_pairs.is_empty() {
        return syn::Error::new(
            name.span(),
            format!(
                "canon_event_struct!: `{name}` has no #[input] fields. \
                 Add #[input] to at least one field, or add #[no_input] \
                 before the struct name for events with no consumable input."
            ),
        )
        .to_compile_error()
        .into();
    }

    // Enforce: at least one #[delta] field — the writer rejects empty deltas at runtime,
    // so event definitions must declare a static delta contract up front.
    if delta_pairs.is_empty() {
        return syn::Error::new(
            name.span(),
            format!(
                "canon_event_struct!: `{name}` has no #[delta] fields. \
                 Writer invariants require every event to carry non-empty delta. \
                 Add #[delta] to the field(s) that represent state change."
            ),
        )
        .to_compile_error()
        .into();
    }

    let input_json = build_json_object(&input_pairs);
    let output_json = build_json_object(&output_pairs);
    let delta_json = build_json_object(&delta_pairs);
    let class_str = match class {
        EventClassSpec::Control => "Control",
        EventClassSpec::Effect => "Effect",
    };
    let next_names: Vec<String> = next.iter().map(|i| i.to_string()).collect();

    // Serialization totality — compile-time assertion.
    let assert_serialize = quote! {
        const _: fn() = || {
            fn __assert_serialize<T: serde::Serialize>() {}
            __assert_serialize::<#name>();
        };
    };

    // content_hash() — always generated; hashes the #[delta] fields via JSON serialization.
    // Returns 0 when no #[delta] fields are present.
    let delta_idents: Vec<&&Ident> = delta_pairs.iter().map(|(_, i)| i).collect();
    let content_hash_impl = if delta_idents.is_empty() {
        quote! {
            impl #name {
                /// Returns a hash of the delta fields for O(1) dedup checks.
                /// Always 0 for this struct (no #[delta] fields declared).
                pub fn content_hash(&self) -> u64 { 0 }
            }
        }
    } else {
        quote! {
            impl #name {
                /// Returns a hash of the #[delta] fields for O(1) dedup checks.
                /// Uses JSON serialization so no `Hash` bound is required on field types.
                pub fn content_hash(&self) -> u64 {
                    use std::hash::{Hash, Hasher};
                    let mut __h = std::collections::hash_map::DefaultHasher::new();
                    #(
                        serde_json::to_string(&self.#delta_idents)
                            .unwrap_or_default()
                            .hash(&mut __h);
                    )*
                    __h.finish()
                }
            }
        }
    };

    // CanonPayloadShape impl — only generated when #[impl_shape] is present.
    // Use this for structs defined within the `canon_event` crate where
    // `crate::CanonPayloadShape` resolves. Other crates omit this attr.
    let shape_impl = if impl_shape {
        quote! {
            impl crate::CanonPayloadShape for #name {
                fn payload_input(&self) -> serde_json::Value {
                    #input_json
                }
                fn payload_output(&self) -> serde_json::Value {
                    #output_json
                }
                fn payload_delta(&self) -> serde_json::Value {
                    #delta_json
                }
                fn payload_data(&self) -> serde_json::Value {
                    serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
                }
            }
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        // Default intentionally omitted — full construction required.
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
        pub struct #name {
            #(
                #(#field_attrs)*
                pub #field_idents: #field_types,
            )*
        }

        #content_hash_impl

        impl #name {
            pub const EVENT_CLASS: &'static str = #class_str;
            pub const EVENT_NEXT: &'static [&'static str] = &[#(#next_names),*];
        }

        #shape_impl

        #assert_serialize
    };
    expanded.into()
}

// ---------------------------------------------------------------------
// canon_event_enum!
// ---------------------------------------------------------------------

struct EventEnum {
    attrs: Vec<Attribute>,
    name: Ident,
    variants: Punctuated<VariantSpec, Comma>,
}

struct VariantSpec {
    attrs: Vec<Attribute>,
    ident: Ident,
    ty: Type,
}

impl Parse for EventEnum {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let attrs = Attribute::parse_outer(input)?;
        let name: Ident = input.parse()?;
        let variants;
        syn::braced!(variants in input);
        let mut variant_list = Punctuated::new();
        while !variants.is_empty() {
            let v_attrs = Attribute::parse_outer(&variants)?;
            let ident: Ident = variants.parse()?;
            let content;
            syn::parenthesized!(content in variants);
            let ty: Type = content.parse()?;
            variant_list.push(VariantSpec { attrs: v_attrs, ident, ty });
            if variants.peek(Comma) {
                variants.parse::<Comma>()?;
            }
        }
        Ok(EventEnum { attrs, name, variants: variant_list })
    }
}

/// Convert PascalCase identifier to snake_case string.
fn pascal_to_snake(s: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() && i > 0 {
            let prev_lower = chars[i - 1].is_lowercase() || chars[i - 1].is_ascii_digit();
            let next_lower = chars.get(i + 1).map(|nc| nc.is_lowercase()).unwrap_or(false);
            if prev_lower || next_lower {
                result.push('_');
            }
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}

#[proc_macro]
pub fn canon_event_enum(input: TokenStream) -> TokenStream {
    let EventEnum { attrs, name, variants } = parse_macro_input!(input as EventEnum);
    let v_idents: Vec<_> = variants.iter().map(|v| &v.ident).collect();
    let v_tys: Vec<_> = variants.iter().map(|v| &v.ty).collect();
    let v_attrs: Vec<_> = variants.iter().map(|v| &v.attrs).collect();

    let is_runtime_event = name == "RuntimeEvent";

    let kind_methods = if is_runtime_event {
        let kind_strs: Vec<String> = v_idents.iter().map(|id| pascal_to_snake(&id.to_string())).collect();
        // kind_str() returns the snake_case name of the active variant.
        let kind_str_method = quote! {
            pub fn kind_str(&self) -> &'static str {
                match self {
                    #( #name::#v_idents(_) => #kind_strs, )*
                }
            }
        };
        // kind() → EventKind — every RuntimeEvent variant name must match an EventKind variant.
        let kind_method = quote! {
            pub fn kind(&self) -> crate::EventKind {
                match self {
                    #( #name::#v_idents(_) => crate::EventKind::#v_idents, )*
                }
            }
        };
        quote! {
            impl #name {
                #kind_str_method
                #kind_method
            }
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        #[derive(Debug, Clone)]
        #(#attrs)*
        pub enum #name {
            #(
                #(#v_attrs)*
                #v_idents(#v_tys),
            )*
        }

        #kind_methods
    };
    expanded.into()
}

// ---------------------------------------------------------------------
// canon_emit!
// ---------------------------------------------------------------------

enum EmitForm {
    Typed { emitter: Expr, variant: Path, inner: Expr, parents: Option<Expr> },
    Debug { emitter: Expr, source: Expr, kind: Expr, payload: Expr },
    DirectRoot { source: Expr, kind: Expr, payload: Expr, path: Expr },
    DirectWithParents { source: Expr, kind: Expr, payload: Expr, path: Expr, parents: Expr },
}

struct EmitInput {
    form: EmitForm,
}

impl Parse for EmitInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let first: Expr = input.parse()?;
        // root; form
        if let Expr::Path(p) = &first {
            if p.path.is_ident("root") && input.peek(Semi) {
                input.parse::<Semi>()?;
                let source: Expr = input.parse()?;
                input.parse::<Token![,]>()?;
                let kind: Expr = input.parse()?;
                input.parse::<Token![,]>()?;
                let payload: Expr = input.parse()?;
                input.parse::<Token![,]>()?;
                let path: Expr = input.parse()?;
                return Ok(EmitInput { form: EmitForm::DirectRoot { source, kind, payload, path } });
            }
        }
        if input.peek(Semi) {
            input.parse::<Semi>()?;
            if input.peek(Ident) && input.peek2(Paren) {
                let variant: Path = input.parse()?;
                let content;
                syn::parenthesized!(content in input);
                let inner: Expr = content.parse()?;
                // Optional: , parents: &[...]
                let parents = if input.peek(Comma) {
                    input.parse::<Comma>()?;
                    let label: Ident = input.parse()?;
                    if !label.eq("parents") {
                        return Err(syn::Error::new(label.span(), "expected `parents:`"));
                    }
                    input.parse::<Token![:]>()?;
                    Some(input.parse::<Expr>()?)
                } else {
                    None
                };
                Ok(EmitInput { form: EmitForm::Typed { emitter: first, variant, inner, parents } })
            } else {
                let source: Expr = input.parse()?;
                input.parse::<Token![,]>()?;
                let kind: Expr = input.parse()?;
                input.parse::<Token![,]>()?;
                let payload: Expr = input.parse()?;
                Ok(EmitInput { form: EmitForm::Debug { emitter: first, source, kind, payload } })
            }
        } else {
            let source = first;
            input.parse::<Token![,]>()?;
            let kind: Expr = input.parse()?;
            input.parse::<Token![,]>()?;
            let payload: Expr = input.parse()?;
            input.parse::<Token![,]>()?;
            let path: Expr = input.parse()?;
            let mut parents: Option<Expr> = None;
            if input.peek(Comma) {
                input.parse::<Comma>()?;
                let label: Ident = input.parse()?;
                if !label.eq("parents") {
                    return Err(syn::Error::new(label.span(), "expected `parents:`"));
                }
                input.parse::<Token![:]>()?;
                parents = Some(input.parse()?);
            }
            if let Some(par_expr) = parents {
                Ok(EmitInput {
                    form: EmitForm::DirectWithParents { source, kind, payload, path, parents: par_expr },
                })
            } else {
                Err(syn::Error::new(
                    path.span(),
                    "canon_emit! requires either `root;` prefix or `parents: &[...]` argument — \
                     every non-root event must declare its causal parents",
                ))
            }
        }
    }
}

#[proc_macro]
pub fn canon_emit(input: TokenStream) -> TokenStream {
    let EmitInput { form } = parse_macro_input!(input as EmitInput);
    match form {
        EmitForm::Typed { emitter, variant, inner, parents } => {
            if let Some(par_expr) = parents {
                quote! {{
                    let __parents: Vec<canon_event::EventId> = (#par_expr)
                        .iter()
                        .map(|p| canon_event::EventId::new(p.to_string()))
                        .collect();
                    #emitter.emit_with_parents(
                        canon_event::RuntimeEvent::#variant(#inner),
                        __parents,
                        ::std::file!(),
                        ::std::line!(),
                    )
                }}
            } else {
                quote! {
                    #emitter.emit_with_parents(
                        canon_event::RuntimeEvent::#variant(#inner),
                        ::std::vec![],
                        ::std::file!(),
                        ::std::line!(),
                    )
                }
            }
        }
        EmitForm::Debug { emitter, source, kind, payload } => {
            quote! {{
                let __wrapped = serde_json::json!({
                    "meta": {
                        "file": ::std::file!(),
                        "line": ::std::line!(),
                        "module": ::std::module_path!(),
                        "crate_name": ::std::env!("CARGO_PKG_NAME"),
                    },
                    "data": #payload,
                });
                #emitter.emit_with_parents(
                    canon_event::RuntimeEvent::Debug(canon_event::DebugEvent {
                        source: (#source).to_string(),
                        kind: (#kind).to_string(),
                        payload: __wrapped,
                    }),
                    ::std::vec![],
                    ::std::file!(),
                    ::std::line!(),
                )
            }}
        }
        EmitForm::DirectRoot { source, kind, payload, path } => {
            quote! {{
                let __kind_raw = (#kind);
                let __kind = ::std::str::FromStr::from_str(&__kind_raw.to_string())
                    .unwrap_or(canon_event::EventKind::Debug);
                let __meta = canon_event::CanonPayloadMeta {
                    file: ::std::file!().to_string(),
                    line: ::std::line!(),
                };
                let __payload = canon_event::CanonPayload::from_data(
                    serde_json::json!({}),
                    serde_json::json!({}),
                    serde_json::json!({}),
                    __meta,
                    serde_json::to_value(#payload)
                        .expect("canon_emit!: payload serialization must not fail"),
                );
                let __wire = canon_event::CanonEvent::new(
                    canon_event::EventId::new(canon_event::new_event_id()),
                    Vec::new(),
                    (#source).to_string(),
                    __kind,
                    canon_event::now_millis(),
                    __payload,
                    true,
                );
                canon_event::write_canon_event_auto(#path, &__wire)
            }}
        }
        EmitForm::DirectWithParents { source, kind, payload, path, parents } => {
            quote! {{
                let __kind_raw = (#kind);
                let __kind = ::std::str::FromStr::from_str(&__kind_raw.to_string())
                    .unwrap_or(canon_event::EventKind::Debug);
                let __meta = canon_event::CanonPayloadMeta {
                    file: ::std::file!().to_string(),
                    line: ::std::line!(),
                };
                let __payload = canon_event::CanonPayload::from_data(
                    serde_json::json!({}),
                    serde_json::json!({}),
                    serde_json::json!({}),
                    __meta,
                    serde_json::to_value(#payload)
                        .expect("canon_emit!: payload serialization must not fail"),
                );
                let __parents: Vec<canon_event::EventId> = (#parents)
                    .iter()
                    .map(|p| canon_event::EventId::new(p.to_string()))
                    .collect();
                let __wire = canon_event::CanonEvent::new(
                    canon_event::EventId::new(canon_event::new_event_id()),
                    __parents,
                    (#source).to_string(),
                    __kind,
                    canon_event::now_millis(),
                    __payload,
                    false,
                );
                canon_event::write_canon_event_auto(#path, &__wire)
            }}
        }
    }
    .into()
}
