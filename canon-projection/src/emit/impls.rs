use canon::ir::CanonIR;
use canon::node::{flags, CanonId, CanonNodeKind};

use crate::emit::functions::emit_fn;
use crate::emit::types::render_type_id;

pub fn emit_impl(ir: &CanonIR, impl_id: CanonId, for_ty: CanonId, for_trait: Option<CanonId>, flags_u: u32, pad: &str) -> String {
    let unsafe_kw = if (flags_u & flags::UNSAFE) != 0 { "unsafe " } else { "" };
    let head = match for_trait {
        Some(t) => format!("{}{}impl {} for {} {{\n", pad, unsafe_kw, render_type_id(ir, t), render_type_id(ir, for_ty)),
        None => format!("{}{}impl {} {{\n", pad, unsafe_kw, render_type_id(ir, for_ty)),
    };

    let mut out = head;
    for (dst, edge) in ir.module_graph.neighbours(canon::id::NodeId(impl_id.0)) {
        if !matches!(edge, canon::edge::EdgeKind::Contains) {
            continue;
        }
        if let CanonNodeKind::Fn { name_id, sig_id, body, flags, .. } = &ir.node(CanonId(dst.0)).kind {
            let method_flags = if for_trait.is_some() { *flags & !(flags::PUB | flags::PUB_CRATE | flags::PUB_SUPER) } else { *flags };
            out.push_str(&emit_fn(ir, *name_id, *sig_id, *body, method_flags, &format!("{}    ", pad)));
        }
    }
    out.push_str(&format!("{}}}\n", pad));
    out
}
