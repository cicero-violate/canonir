use anyhow::{anyhow, Result};
use canon_event::{CanonEvent, EditEvent};

fn require_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing or invalid arg: {key}"))
}

/// Decode CapabilityRequested into a typed CanonEvent. Temporary bridge during migration.
pub fn decode_capability_event(event: &CanonEvent) -> Result<CanonEvent> {
    let CanonEvent::CapabilityRequested(req) = event else {
        return Ok(event.clone());
    };

    match req.name.as_str() {
        "edit.rename_symbol" => {
            let project = require_str(&req.args, "project")?.to_string();
            let old = require_str(&req.args, "old")?.to_string();
            let new = require_str(&req.args, "new")?.to_string();
            Ok(CanonEvent::Edit(EditEvent::RenameSymbol(canon_event::RenameSymbol { project, old, new })))
        }
        "edit.move_symbol" => {
            let project = require_str(&req.args, "project")?.to_string();
            let symbol = require_str(&req.args, "symbol")?.to_string();
            let module = require_str(&req.args, "module")?.to_string();
            Ok(CanonEvent::Edit(EditEvent::MoveSymbol(canon_event::MoveSymbol { project, symbol, module })))
        }
        "edit.delete_symbol" => {
            let project = require_str(&req.args, "project")?.to_string();
            let symbol = require_str(&req.args, "symbol")?.to_string();
            Ok(CanonEvent::Edit(EditEvent::DeleteSymbol(canon_event::DeleteSymbol { project, symbol })))
        }
        "edit.rename_module" => {
            let project = require_str(&req.args, "project")?.to_string();
            let old = require_str(&req.args, "old")?.to_string();
            let new = require_str(&req.args, "new")?.to_string();
            Ok(CanonEvent::Edit(EditEvent::RenameModule(canon_event::RenameModule { project, old, new })))
        }
        "edit.rename_dir" => {
            let project = require_str(&req.args, "project")?.to_string();
            let old = require_str(&req.args, "old")?.to_string();
            let new = require_str(&req.args, "new")?.to_string();
            Ok(CanonEvent::Edit(EditEvent::RenameDir(canon_event::RenameDir { project, old: old.into(), new: new.into() })))
        }
        name => Err(anyhow!("no decode for capability: {name}")),
    }
}
