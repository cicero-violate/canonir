use canon::ir::CanonIR;
use canon::node::{flags, CanonNodeKind};

pub fn vis_token(f: u32) -> &'static str {
    if (f & flags::PUB) != 0 {
        "pub "
    } else if (f & flags::PUB_CRATE) != 0 {
        "pub(crate) "
    } else if (f & flags::PUB_SUPER) != 0 {
        "pub(super) "
    } else {
        ""
    }
}

pub fn normalize_use_path<'a>(path: &'a str, ir: &CanonIR) -> std::borrow::Cow<'a, str> {
    normalize_crate_path(path, ir)
}

pub fn normalize_extern_path(path: &str, ir: &CanonIR) -> String {
    let mut s = normalize_crate_path(path, ir).into_owned();
    s = s.replace("std::PathBuf", "std::path::PathBuf");
    s = s.replace("std::Path", "std::path::Path");
    s = s.replace("&std::path::Path", "&Path");
    s = s.replace("std::path::PathBuf", "PathBuf");
    s = s.replace("std::path::Path", "Path");

    for p in ["crate::Vec<", "crate::Box<", "crate::Result<", "crate::Option<"] {
        if s.starts_with(p) {
            s = s.replacen("crate::", "", 1);
        }
    }
    s = s.replace("<dyn traits::", "<dyn crate::traits::");
    s = s.replace("<data::", "<crate::data::");
    if s.starts_with("data::") {
        s = format!("crate::{}", s);
    }
    if s.starts_with("traits::") {
        s = format!("crate::{}", s);
    }
    if s == "Symbol" {
        s = "crate::symbol::Symbol".to_string();
    }
    s = s.replace("<Symbol>", "<crate::symbol::Symbol>");
    s
}

fn normalize_crate_path<'a>(path: &'a str, ir: &CanonIR) -> std::borrow::Cow<'a, str> {
    let crate_name: Option<String> = ir.nodes.iter().find_map(|n| if let CanonNodeKind::Crate { name_id, .. } = &n.kind { Some(ir.lookup_name(*name_id).to_string()) } else { None });
    if let Some(ref name) = crate_name {
        let prefix = format!("{}::", name);
        if let Some(rest) = path.strip_prefix(prefix.as_str()) {
            return std::borrow::Cow::Owned(format!("crate::{}", rest));
        }
    }
    // Keep non-crate paths untouched. Auto-prefixing with `crate::` caused
    // invalid paths for external crates and standard/prelude types.
    std::borrow::Cow::Borrowed(path)
}
