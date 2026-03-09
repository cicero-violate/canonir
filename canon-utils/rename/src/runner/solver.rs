use crate::core::rustc_session::RustcSession;
use std::collections::HashMap;

pub(crate) fn build_rename_groups(
    pairs: &[(String, String)],
    session: &RustcSession,
) -> (Vec<(String, String)>, usize) {
    let mut by_key: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut independent: Vec<(String, String)> = Vec::new();
    let mut impls_by_key: HashMap<String, Vec<String>> = HashMap::new();
    let mut trait_decl_by_key: HashMap<String, String> = HashMap::new();

    for symbol_id in session.symbol_ids() {
        if let Some((trait_path, method)) = trait_method_key_from_impl(&symbol_id) {
            let key = format!("{trait_path}::{method}");
            impls_by_key.entry(key).or_default().push(symbol_id);
        }
    }
    for (key, _impls) in &impls_by_key {
        if let Some((trait_path, method)) = key.rsplit_once("::") {
            let trait_decl = format!("{trait_path}::{method}");
            if session.symbol_ids().iter().any(|s| s == &trait_decl) {
                trait_decl_by_key.insert(key.clone(), trait_decl);
            }
        }
    }

    for (old, new) in pairs {
        if let Some((trait_path, method)) = trait_method_key_from_impl(old) {
            let key = format!("{trait_path}::{method}");
            by_key.entry(key).or_default().push((old.clone(), new.clone()));
            continue;
        }
        if let Some((parent, method)) = old.rsplit_once("::") {
            let key = format!("{parent}::{method}");
            if impls_by_key.contains_key(&key) {
                by_key.entry(key).or_default().push((old.clone(), new.clone()));
                continue;
            }
        }
        independent.push((old.clone(), new.clone()));
    }

    let mut out = independent;
    let mut skipped_groups = 0usize;
    let pair_map: HashMap<String, String> = pairs.iter().map(|(o, n)| (o.clone(), n.clone())).collect();
    for (key, members) in by_key {
        let expected_impls = impls_by_key.get(&key).cloned().unwrap_or_default();
        let mut expected: Vec<String> = expected_impls;
        if let Some(trait_decl) = trait_decl_by_key.get(&key) {
            expected.push(trait_decl.clone());
        }
        expected.sort();
        expected.dedup();
        let mut missing = false;
        let mut expected_new: Option<String> = None;
        for symbol_id in &expected {
            let Some(new_name) = pair_map.get(symbol_id) else {
                missing = true;
                break;
            };
            if let Some(exp) = &expected_new {
                if exp != new_name {
                    missing = true;
                    break;
                }
            } else {
                expected_new = Some(new_name.clone());
            }
        }
        if missing {
            skipped_groups += 1;
            continue;
        }
        out.extend(members);
    }
    (out, skipped_groups)
}

pub(crate) fn trait_method_key_from_impl(symbol_id: &str) -> Option<(String, String)> {
    if !symbol_id.contains(" as ") {
        return None;
    }
    let trait_part = extract_trait_from_impl_symbol(symbol_id)?.to_string();
    if !trait_part.starts_with("crate::") {
        return None;
    }
    let method = symbol_id.rsplit("::").next()?.to_string();
    Some((trait_part, method))
}

pub(crate) fn extract_trait_from_impl_symbol(symbol_id: &str) -> Option<&str> {
    let as_pos = symbol_id.find(" as ")?;
    let after_as = &symbol_id[as_pos + 4..];
    let end = after_as.find('>')?;
    Some(&after_as[..end])
}

pub(crate) fn is_known_external_trait_method(symbol_id: &str) -> bool {
    const EXTERNAL_TRAITS: &[&str] = &[
        "std::fmt::Display",
        "std::fmt::Debug",
        "std::fmt::Write",
        "std::fmt::LowerHex",
        "std::fmt::UpperHex",
        "std::convert::From",
        "std::convert::Into",
        "std::convert::TryFrom",
        "std::convert::TryInto",
        "std::convert::AsRef",
        "std::convert::AsMut",
        "std::clone::Clone",
        "std::default::Default",
        "std::ops::Add",
        "std::ops::Sub",
        "std::ops::Mul",
        "std::ops::Div",
        "std::ops::Neg",
        "std::ops::Not",
        "std::ops::Index",
        "std::ops::IndexMut",
        "std::ops::Deref",
        "std::ops::DerefMut",
        "std::ops::Drop",
        "std::iter::Iterator",
        "std::iter::IntoIterator",
        "std::iter::FromIterator",
        "std::cmp::PartialEq",
        "std::cmp::Eq",
        "std::cmp::PartialOrd",
        "std::cmp::Ord",
        "std::hash::Hash",
        "std::error::Error",
        "std::str::FromStr",
        "std::io::Read",
        "std::io::Write",
        "std::io::Seek",
        "serde::Serialize",
        "serde::Deserialize",
        "async_trait",
    ];
    EXTERNAL_TRAITS.iter().any(|name| symbol_id.contains(name))
}

pub(crate) fn classify_rename_safety(symbol_id: &str, _kind: &str) -> &'static str {
    if symbol_id.contains(" as ") {
        if let Some(trait_part) = extract_trait_from_impl_symbol(symbol_id) {
            if !trait_part.starts_with("crate::") {
                return "external_trait_impl";
            }
        }
    }
    if is_known_external_trait_method(symbol_id) {
        return "external_trait_impl";
    }
    "safe"
}

pub(crate) fn is_degenerate_rename(old: &str, new: &str) -> bool {
    if old == new {
        return true;
    }
    if new == format!("{old}{old}") {
        return true;
    }
    if new.len() > old.len() {
        let prefix = &new[..new.len() - old.len()];
        if to_snake(prefix) == to_snake(old) {
            return true;
        }
    }
    false
}

pub(crate) fn to_snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        if let Some(lower) = c.to_lowercase().next() {
            out.push(lower);
        }
    }
    out
}
