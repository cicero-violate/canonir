use proc_macro2::Span;
use syn::visit_mut::VisitMut;

#[derive(Clone)]
enum UseTail {
    Name,
    Rename(syn::Ident),
}

pub(crate) fn rewrite_string_attrs_in_file(ast: &mut syn::File, old_name: &str, new_name: &str) -> bool {
    struct AttrStringRewriter<'a> {
        old: &'a str,
        new: &'a str,
        changed: bool,
    }

    impl VisitMut for AttrStringRewriter<'_> {
        fn visit_attribute_mut(&mut self, attr: &mut syn::Attribute) {
            if let syn::Meta::List(ref mut meta_list) = attr.meta {
                let tokens = meta_list.tokens.clone();
                let mut new_tokens = proc_macro2::TokenStream::new();
                let mut token_iter = tokens.into_iter().peekable();
                let mut local_changed = false;
                while let Some(tt) = token_iter.next() {
                    match &tt {
                        proc_macro2::TokenTree::Literal(lit) => {
                            let s = lit.to_string();
                            if s == format!("\"{}\"", self.old) {
                                let new_lit = proc_macro2::Literal::string(self.new);
                                new_tokens.extend(std::iter::once(proc_macro2::TokenTree::Literal(new_lit)));
                                local_changed = true;
                                continue;
                            }
                        }
                        _ => {}
                    }
                    new_tokens.extend(std::iter::once(tt));
                }
                if local_changed {
                    meta_list.tokens = new_tokens;
                    self.changed = true;
                }
            }
            syn::visit_mut::visit_attribute_mut(self, attr);
        }
    }

    let mut rewriter = AttrStringRewriter { old: old_name, new: new_name, changed: false };
    rewriter.visit_file_mut(ast);
    rewriter.changed
}

pub(crate) struct PathRewriter {
    old_full: Option<Vec<String>>,
    new_full: Option<Vec<String>>,
    old_prefix: Option<Vec<String>>,
    new_prefix: Option<Vec<String>>,
    changed: bool,
}

impl PathRewriter {
    pub(crate) fn replace_full(old_full: &[String], new_full: &[String]) -> Self {
        Self { old_full: Some(old_full.to_vec()), new_full: Some(new_full.to_vec()), old_prefix: None, new_prefix: None, changed: false }
    }
    pub(crate) fn replace_prefix(old_prefix: &[String], new_prefix: &[String]) -> Self {
        Self { old_full: None, new_full: None, old_prefix: Some(old_prefix.to_vec()), new_prefix: Some(new_prefix.to_vec()), changed: false }
    }
    pub(crate) fn visit_file(&mut self, file: &mut syn::File) -> bool {
        self.changed = false;
        self.visit_file_mut(file);
        self.changed
    }
    fn rewrite_segments(&self, segments: &mut Vec<String>) -> bool {
        let original = segments.clone();
        if let (Some(old_full), Some(new_full)) = (&self.old_full, &self.new_full) {
            if segments == old_full {
                *segments = new_full.clone();
            } else if segments.starts_with(old_full) {
                let mut replaced = new_full.clone();
                replaced.extend_from_slice(&segments[old_full.len()..]);
                *segments = replaced;
            }
        }
        if let (Some(old_prefix), Some(new_prefix)) = (&self.old_prefix, &self.new_prefix) {
            if segments.starts_with(old_prefix) {
                let mut replaced = new_prefix.clone();
                replaced.extend_from_slice(&segments[old_prefix.len()..]);
                *segments = replaced;
            }
        }
        *segments != original
    }
}

impl VisitMut for PathRewriter {
    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        let mut segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        let local_changed = self.rewrite_segments(&mut segments);
        if local_changed {
            path.segments.clear();
            for seg in segments {
                path.segments.push(syn::PathSegment { ident: syn::Ident::new(&seg, Span::call_site()), arguments: syn::PathArguments::None });
            }
            self.changed = true;
        }
        syn::visit_mut::visit_path_mut(self, path);
    }
    fn visit_use_tree_mut(&mut self, tree: &mut syn::UseTree) {
        if let Some((segments, tail)) = flatten_use_tree(tree) {
            let mut new_segments = segments.clone();
            let local_changed = self.rewrite_segments(&mut new_segments);
            if new_segments != segments {
                *tree = build_use_tree(&new_segments, tail);
                if local_changed {
                    self.changed = true;
                }
                return;
            }
        }
        if let Some((prefix, group)) = use_tree_group_prefix(tree) {
            let mut local_changed = false;
            let mut updated_prefix: Option<Vec<String>> = None;
            for item in group.items.iter_mut() {
                if let Some((segments, tail)) = flatten_use_tree(item) {
                    let mut candidate = prefix.clone();
                    candidate.extend_from_slice(&segments);
                    let mut rewritten = candidate.clone();
                    if self.rewrite_segments(&mut rewritten) {
                        let new_prefix = rewritten[..prefix.len()].to_vec();
                        let rewritten_tail = &rewritten[prefix.len()..];
                        *item = build_use_tree(rewritten_tail, tail);
                        if new_prefix != prefix {
                            if let Some(existing) = &updated_prefix {
                                if existing != &new_prefix {
                                    continue;
                                }
                            } else {
                                updated_prefix = Some(new_prefix);
                            }
                        }
                        local_changed = true;
                    }
                }
            }
            if local_changed {
                if let Some(new_prefix) = updated_prefix {
                    let items = std::mem::take(&mut group.items);
                    *tree = build_use_group_tree(&new_prefix, items);
                }
                self.changed = true;
                return;
            }
        }
        syn::visit_mut::visit_use_tree_mut(self, tree);
    }
}

fn build_use_tree(segments: &[String], tail: UseTail) -> syn::UseTree {
    if segments.is_empty() {
        return syn::UseTree::Glob(syn::UseGlob { star_token: syn::token::Star::default() });
    }
    if segments.len() == 1 {
        let ident = syn::Ident::new(&segments[0], Span::call_site());
        return match tail {
            UseTail::Name => syn::UseTree::Name(syn::UseName { ident }),
            UseTail::Rename(rename) => syn::UseTree::Rename(syn::UseRename { ident, rename, as_token: Default::default() }),
        };
    }
    let ident = syn::Ident::new(&segments[0], Span::call_site());
    syn::UseTree::Path(syn::UsePath { ident, colon2_token: Default::default(), tree: Box::new(build_use_tree(&segments[1..], tail)) })
}

fn flatten_use_tree(tree: &syn::UseTree) -> Option<(Vec<String>, UseTail)> {
    let mut segments = Vec::new();
    let mut current = tree;
    loop {
        match current {
            syn::UseTree::Path(path) => {
                segments.push(path.ident.to_string());
                current = &path.tree;
            }
            syn::UseTree::Name(name) => {
                segments.push(name.ident.to_string());
                return Some((segments, UseTail::Name));
            }
            syn::UseTree::Rename(rename) => {
                segments.push(rename.ident.to_string());
                return Some((segments, UseTail::Rename(rename.rename.clone())));
            }
            syn::UseTree::Glob(_) | syn::UseTree::Group(_) => return None,
        }
    }
}

fn use_tree_group_prefix(tree: &mut syn::UseTree) -> Option<(Vec<String>, &mut syn::UseGroup)> {
    let mut prefix = Vec::new();
    let mut current = tree;
    loop {
        match current {
            syn::UseTree::Path(use_path) => {
                prefix.push(use_path.ident.to_string());
                current = &mut *use_path.tree;
            }
            syn::UseTree::Group(group) => return Some((prefix, group)),
            _ => return None,
        }
    }
}

fn build_use_group_tree(
    prefix: &[String],
    items: syn::punctuated::Punctuated<syn::UseTree, syn::token::Comma>,
) -> syn::UseTree {
    let group = syn::UseGroup { brace_token: Default::default(), items };
    let mut tree = syn::UseTree::Group(group);
    for seg in prefix.iter().rev() {
        let ident = syn::Ident::new(seg, Span::call_site());
        tree = syn::UseTree::Path(syn::UsePath { ident, colon2_token: Default::default(), tree: Box::new(tree) });
    }
    tree
}
