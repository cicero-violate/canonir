use crate::core::ProjectEditor;
use crate::core::rustc_session::RustcSession;
use crate::core::symbol_id::normalize_symbol_id;

pub(crate) struct VerifySummary {
    pub(crate) applied: bool,
    pub(crate) pairs_checked: usize,
    pub(crate) pairs_changed: usize,
}

pub(crate) fn verify_renames_applied(
    session: &RustcSession,
    editor: &ProjectEditor,
    renames: &[(String, String)],
) -> VerifySummary {
    let mut pairs_checked = 0usize;
    let mut pairs_changed = 0usize;
    let sources = &editor.last_applied_sources;
    if sources.is_empty() || renames.is_empty() {
        return VerifySummary {
            applied: false,
            pairs_checked,
            pairs_changed,
        };
    }

    for (old_symbol, new_symbol) in renames {
        let old_norm = normalize_symbol_id(old_symbol);
        let old_ident = old_symbol
            .rsplit_once("::")
            .map(|(_, s)| s)
            .unwrap_or(old_symbol.as_str());
        let new_ident = new_symbol
            .rsplit_once("::")
            .map(|(_, s)| s)
            .unwrap_or(new_symbol.as_str());
        pairs_checked += 1;

        let Some(spans_by_file) = session.spans_for(&old_norm) else {
            continue;
        };

        let mut saw_file = false;
        let mut all_files_match = true;
        for (path, spans) in spans_by_file {
            if spans.is_empty() {
                continue;
            }
            saw_file = true;
            let Some(after) = sources.get(path) else {
                all_files_match = false;
                break;
            };
            if !after.contains(new_ident) {
                all_files_match = false;
                break;
            }
            if let Some(first_span) = spans.first() {
                let lo = first_span.lo;
                let hi = lo + old_ident.len();
                if after.as_bytes().get(lo..hi) == Some(old_ident.as_bytes()) {
                    all_files_match = false;
                    break;
                }
            }
        }

        if saw_file && all_files_match {
            pairs_changed += 1;
        }
    }

    VerifySummary {
        applied: pairs_checked > 0,
        pairs_checked,
        pairs_changed,
    }
}
