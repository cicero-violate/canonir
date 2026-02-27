use crate::types::{EdgeHint, EdgeKind};

#[inline]
pub fn push(edges: &mut Vec<EdgeHint>, src: u32, dst: u32, kind: EdgeKind) {
    edges.push(EdgeHint { src, dst, kind });
}

#[inline]
pub fn push_contains(edges: &mut Vec<EdgeHint>, src: u32, dst: u32) {
    push(edges, src, dst, EdgeKind::Contains);
}

#[inline]
pub fn push_resolves(edges: &mut Vec<EdgeHint>, src: u32, dst: u32) {
    push(edges, src, dst, EdgeKind::Resolves);
}

#[inline]
pub fn push_reexports(edges: &mut Vec<EdgeHint>, src: u32, dst: u32) {
    push(edges, src, dst, EdgeKind::Reexports);
}

#[inline]
pub fn push_assoc_item(edges: &mut Vec<EdgeHint>, src: u32, dst: u32) {
    push(edges, src, dst, EdgeKind::AssocItem);
}

#[inline]
pub fn push_impl_for(edges: &mut Vec<EdgeHint>, src: u32, dst: u32) {
    push(edges, src, dst, EdgeKind::ImplFor);
}

#[inline]
pub fn push_impl_ref(edges: &mut Vec<EdgeHint>, src: u32, dst: u32) {
    push(edges, src, dst, EdgeKind::ImplRef);
}
