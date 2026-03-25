use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseBuffer, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    token::{Comma, Paren, Semi},
    visit::Visit,
    Attribute, Expr, ExprMatch, Ident, ItemFn, Pat, Path, Token, Type,
    spanned::Spanned,
};

/// Applied to an `on_event` implementation.
/// Fails compilation if the function body contains any `match` expression
/// where one arm is a wildcard (`_`) or lowercase binding pattern at the
/// top level, AND another arm contains a `RuntimeEvent::` path pattern.
/// This ensures new RuntimeEvent variants must be handled explicitly.
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
                    self.errors.push("  wildcard/binding arm found in RuntimeEvent match — add explicit arms for every variant".to_string());
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
        Pat::Ident(i) => i.subpat.is_none() && i.by_ref.is_none() && !i.ident.to_string().starts_with(|c: char| c.is_uppercase()),
        Pat::Or(or) => or.cases.iter().any(pattern_is_wildcard_or_binding),
        _ => false,
    }
}

// ---------------------------------------------------------------------
// canon_event_struct!
// ---------------------------------------------------------------------

struct EventStruct {
    name: Ident,
    fields: Punctuated<FieldSpec, Comma>,
}

struct FieldSpec {
    attrs: Vec<Attribute>,
    ident: Ident,
    ty: Type,
}

impl Parse for EventStruct {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        let content;
        syn::braced!(content in input);
        let mut fields = Punctuated::new();
        while !content.is_empty() {
            let field = FieldSpec::parse(&content)?;
            fields.push(field);
            if content.peek(Comma) {
                content.parse::<Comma>()?;
            }
        }
        Ok(EventStruct { name, fields })
    }
}

impl FieldSpec {
    fn parse(input: &ParseBuffer) -> syn::Result<Self> {
        let attrs = Attribute::parse_outer(input)?;
        let ident: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let ty: Type = input.parse()?;
        Ok(FieldSpec { attrs, ident, ty })
    }
}

#[proc_macro]
pub fn canon_event_struct(input: TokenStream) -> TokenStream {
    let EventStruct { name, fields } = parse_macro_input!(input as EventStruct);
    let field_idents: Vec<_> = fields.iter().map(|f| &f.ident).collect();
    let field_types: Vec<_> = fields.iter().map(|f| &f.ty).collect();
    let field_attrs: Vec<_> = fields.iter().map(|f| &f.attrs).collect();

    let expanded = quote! {
        #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
        pub struct #name {
            #(
                #(#field_attrs)*
                pub #field_idents: #field_types,
            )*
        }
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

#[proc_macro]
pub fn canon_event_enum(input: TokenStream) -> TokenStream {
    let EventEnum { attrs, name, variants } = parse_macro_input!(input as EventEnum);
    let v_idents: Vec<_> = variants.iter().map(|v| &v.ident).collect();
    let v_tys: Vec<_> = variants.iter().map(|v| &v.ty).collect();
    let v_attrs: Vec<_> = variants.iter().map(|v| &v.attrs).collect();
    let expanded = quote! {
        #[derive(Debug, Clone)]
        #(#attrs)*
        pub enum #name {
            #(
                #(#v_attrs)*
                #v_idents(#v_tys),
            )*
        }

    };
    expanded.into()
}

// ---------------------------------------------------------------------
// canon_emit!
// ---------------------------------------------------------------------

enum EmitForm {
    Typed { emitter: Expr, variant: Path, inner: Expr },
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
                Ok(EmitInput { form: EmitForm::Typed { emitter: first, variant, inner } })
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
                Ok(EmitInput { form: EmitForm::DirectWithParents { source, kind, payload, path, parents: par_expr } })
            } else {
                Err(syn::Error::new(path.span(), "canon_emit! requires either `root;` prefix or `parents: &[...]`"))
            }
        }
    }
}

#[proc_macro]
pub fn canon_emit(input: TokenStream) -> TokenStream {
    let EmitInput { form } = parse_macro_input!(input as EmitInput);
    match form {
        EmitForm::Typed { emitter, variant, inner } => {
            quote! {
                #emitter.emit_located(canon_event::RuntimeEvent::#variant(#inner), ::std::file!(), ::std::line!())
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
                #emitter.emit_located(
                    canon_event::RuntimeEvent::Debug(canon_event::DebugEvent {
                        source: (#source).to_string(),
                        kind: (#kind).to_string(),
                        payload: __wrapped,
                    }),
                    ::std::file!(),
                    ::std::line!(),
                )
            }}
        }
        EmitForm::DirectRoot { source, kind, payload, path } => {
            quote! {{
                let __kind_raw = (#kind);
                let __kind = ::std::str::FromStr::from_str(&__kind_raw.to_string()).unwrap_or(canon_event::EventKind::Debug);
                let __meta = canon_event::CanonPayloadMeta { file: ::std::file!().to_string(), line: ::std::line!() };
                let __payload = canon_event::CanonPayload::from_data(
                    serde_json::json!({}),
                    serde_json::json!({}),
                    serde_json::json!({}),
                    __meta,
                    serde_json::to_value(#payload).unwrap_or_else(|_| serde_json::Value::Null),
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
                let __kind = ::std::str::FromStr::from_str(&__kind_raw.to_string()).unwrap_or(canon_event::EventKind::Debug);
                let __meta = canon_event::CanonPayloadMeta { file: ::std::file!().to_string(), line: ::std::line!() };
                let __payload = canon_event::CanonPayload::from_data(
                    serde_json::json!({}),
                    serde_json::json!({}),
                    serde_json::json!({}),
                    __meta,
                    serde_json::to_value(#payload).unwrap_or_else(|_| serde_json::Value::Null),
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
