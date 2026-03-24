use proc_macro::TokenStream;
use syn::{parse_macro_input, ItemFn, visit::Visit, ExprMatch, Pat};

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
                    self.errors.push(
                        "  wildcard/binding arm found in RuntimeEvent match — add explicit arms for every variant".to_string()
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
