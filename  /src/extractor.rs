use crate::symbol::Symbol;

use tree_sitter::Node;

use tree_sitter::Parser;

fn collect_enum_variants<Node>(body: Node<'_>, src: &[u8]) -> Vec<String> {
    let mut variants = Vec::new();
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        if child.kind() == "enum_variant" {
            if let Some(name_node) = child.child_by_field_name("name") {
                variants.push(node_text(name_node, src).to_string());
            }
        }
    }
    variants
}

fn collect_methods<Node>(body: Node<'_>, src: &[u8]) -> Vec<String> {
    let mut methods = Vec::new();
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        if child.kind() == "function_item" {
            methods.push(fn_signature(child, src));
        }
    }
    methods
}

fn collect_struct_fields<Node>(body: Node<'_>, src: &[u8]) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        if child.kind() == "field_declaration" {
            if let Some(name_node) = child.child_by_field_name("name") {
                fields.push(node_text(name_node, src).to_string());
            }
        }
    }
    fields
}

pub fn extract_symbols(src: &str) -> Vec<Symbol> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_rust::LANGUAGE.into()).expect("failed to load Rust grammar");
    
    let tree = parser.parse(src, None).expect("parse failed");
    extract_top_level(tree.root_node(), src.as_bytes())
}

fn extract_top_level<Node>(root: Node<'_>, src: &[u8]) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let mut cursor = root.walk();
    
    for node in root.children(&mut cursor) {
        let line = node.start_position().row + 1; // 1-indexed
    
        match node.kind() {
            "struct_item" => {
                let name = field_text(node, "name", src).unwrap_or("?").to_string();
                let fields = node.child_by_field_name("body").map(|b| collect_struct_fields(b, src)).unwrap_or_default();
                symbols.push(Symbol::Struct { name, fields, line });
            }
    
            "enum_item" => {
                let name = field_text(node, "name", src).unwrap_or("?").to_string();
                let variants = node.child_by_field_name("body").map(|b| collect_enum_variants(b, src)).unwrap_or_default();
                symbols.push(Symbol::Enum { name, variants, line });
            }
    
            "trait_item" => {
                let name = field_text(node, "name", src).unwrap_or("?").to_string();
                let methods = node.child_by_field_name("body").map(|b| collect_methods(b, src)).unwrap_or_default();
                symbols.push(Symbol::Trait { name, methods, line });
            }
    
            "function_item" => {
                let name = field_text(node, "name", src).unwrap_or("?").to_string();
                let signature = fn_signature(node, src);
                symbols.push(Symbol::Function { name, signature, line });
            }
    
            "impl_item" => {
                // `impl Trait for Type` or plain `impl Type`
                let type_name = field_text(node, "type", src).unwrap_or("?").to_string();
                let trait_name = field_text(node, "trait", src).map(|s| s.to_string());
                let methods = node.child_by_field_name("body").map(|b| collect_methods(b, src)).unwrap_or_default();
                symbols.push(Symbol::Impl { type_name, trait_name, methods, line });
            }
    
            "type_item" => {
                let name = field_text(node, "name", src).unwrap_or("?").to_string();
                symbols.push(Symbol::TypeAlias { name, line });
            }
    
            _ => {}
        }
    }
    
    symbols
}

fn field_text<'a, Node>(node: Node<'_>, field: &str, src: &'a [u8]) -> Option<&'a str> {
    node.child_by_field_name(field).map(|n| node_text(n, src))
}

fn fn_signature<Node>(node: Node<'_>, src: &[u8]) -> String {
    let name = field_text(node, "name", src).unwrap_or("?");
    
    // parameters node
    let params = node.child_by_field_name("parameters").map(|n| node_text(n, src)).unwrap_or("()");
    
    // optional return type
    let ret = node.child_by_field_name("return_type").map(|n| format!(" -> {}", node_text(n, src))).unwrap_or_default();
    
    format!("fn {}{}{}", name, params, ret)
}

fn node_text<'a, Node>(node: Node<'_>, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or("")
}