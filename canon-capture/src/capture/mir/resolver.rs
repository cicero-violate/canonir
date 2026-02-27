use rustc_middle::mir;
use std::collections::HashMap;

pub(crate) struct LocalNameResolver {
    by_local: HashMap<u32, String>,
}

impl LocalNameResolver {
    pub(crate) fn new<'tcx>(body: &mir::Body<'tcx>, param_names: &[String]) -> Self {
        let mut by_local: HashMap<u32, String> = HashMap::new();
        let mut owner_by_name: HashMap<String, u32> = HashMap::new();
        insert_unique_name(&mut by_local, &mut owner_by_name, 0, "__ret".to_string());
        for (idx, name) in param_names.iter().enumerate() {
            let local_idx = (idx + 1) as u32;
            if is_rust_ident(name) {
                insert_unique_name(&mut by_local, &mut owner_by_name, local_idx, name.clone());
            }
        }
        for dbg in &body.var_debug_info {
            let mir::VarDebugInfoContents::Place(place) = &dbg.value else {
                continue;
            };
            let projection_ok = place.projection.is_empty()
                || (place.projection.len() == 1
                    && matches!(
                        place.projection[0],
                        mir::ProjectionElem::Field(..) | mir::ProjectionElem::Deref
                    ));
            if !projection_ok {
                continue;
            }
            let name = dbg.name.as_str().to_string();
            if !is_rust_ident(&name) {
                continue;
            }
            if by_local.contains_key(&place.local.as_u32()) {
                continue;
            }
            insert_unique_name(&mut by_local, &mut owner_by_name, place.local.as_u32(), name);
        }
        for local in body.local_decls.indices() {
            if by_local.contains_key(&local.as_u32()) {
                continue;
            }
            insert_unique_name(
                &mut by_local,
                &mut owner_by_name,
                local.as_u32(),
                format!("_v{}", local.as_u32()),
            );
        }
        Self { by_local }
    }

    pub(crate) fn label_place(&self, place: &mir::Place<'_>) -> Option<String> {
        if place
            .projection
            .iter()
            .any(|p| matches!(p, mir::ProjectionElem::Downcast(..)))
        {
            return None;
        }
        if place
            .projection
            .iter()
            .any(|p| matches!(p, mir::ProjectionElem::OpaqueCast(..) | mir::ProjectionElem::UnwrapUnsafeBinder(..)))
        {
            return None;
        }
        if !place.projection.is_empty() {
            return None;
        }
        self.label_local(place.local)
    }

    pub(crate) fn label_local(&self, local: mir::Local) -> Option<String> {
        let name = self.by_local.get(&local.as_u32())?;
        if !is_value_name_safe(name) {
            return None;
        }
        Some(name.clone())
    }

    pub(crate) fn label_place_ref(&self, place: mir::PlaceRef<'_>) -> Option<String> {
        if place
            .projection
            .iter()
            .any(|p| matches!(p, mir::ProjectionElem::Downcast(..)))
        {
            return None;
        }
        if place
            .projection
            .iter()
            .any(|p| matches!(p, mir::ProjectionElem::OpaqueCast(..) | mir::ProjectionElem::UnwrapUnsafeBinder(..)))
        {
            return None;
        }
        if !place.projection.is_empty() {
            return None;
        }
        let name = self.by_local.get(&place.local.as_u32())?;
        if !is_value_name_safe(name) {
            return None;
        }
        Some(name.clone())
    }
}

fn is_rust_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn is_value_name_safe(s: &str) -> bool {
    if s.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        return false;
    }
    true
}

fn insert_unique_name(
    by_local: &mut HashMap<u32, String>,
    owner_by_name: &mut HashMap<String, u32>,
    local_idx: u32,
    preferred: String,
) {
    if let Some(existing) = owner_by_name.get(&preferred) {
        if *existing == local_idx {
            by_local.insert(local_idx, preferred);
            return;
        }
        let mut candidate = format!("{preferred}_{local_idx}");
        while owner_by_name.contains_key(&candidate) {
            candidate.push('_');
        }
        owner_by_name.insert(candidate.clone(), local_idx);
        by_local.insert(local_idx, candidate);
        return;
    }
    owner_by_name.insert(preferred.clone(), local_idx);
    by_local.insert(local_idx, preferred);
}
