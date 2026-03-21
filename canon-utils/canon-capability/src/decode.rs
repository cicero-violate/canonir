use anyhow::{anyhow, Result};
use canon_event::{CanonEvent, EditEvent};
use canon_event::{AnalysisEvent, AnalysisRun, AnalysisWorkspace, BashInvoke, CargoBuild, CargoCheck, CargoEvent, CargoRun, FileEvent, FilePatch, FileRead, FileWrite, LlmCall};

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
        "cargo.build" => {
            let crate_name = require_str(&req.args, "crate")?.to_string();
            Ok(CanonEvent::Cargo(CargoEvent::Build(CargoBuild { crate_name })))
        }
        "cargo.run" => {
            let crate_name = require_str(&req.args, "crate")?.to_string();
            let bin = req.args.get("bin").and_then(|v| v.as_str()).map(str::to_string);
            let args = req.args
                .get("args")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).map(str::to_string).collect())
                .unwrap_or_default();
            Ok(CanonEvent::Cargo(CargoEvent::Run(CargoRun { crate_name, bin, args })))
        }
        "cargo.check" => {
            let crate_name = require_str(&req.args, "crate")?.to_string();
            Ok(CanonEvent::Cargo(CargoEvent::Check(CargoCheck { crate_name })))
        }
        "file.read" => {
            let path = require_str(&req.args, "path")?.to_string();
            Ok(CanonEvent::File(FileEvent::Read(FileRead { path })))
        }
        "file.write" => {
            let path = require_str(&req.args, "path")?.to_string();
            let content = require_str(&req.args, "content")?.to_string();
            Ok(CanonEvent::File(FileEvent::Write(FileWrite { path, content })))
        }
        "file.patch" => {
            let path = require_str(&req.args, "path")?.to_string();
            let old = require_str(&req.args, "old")?.to_string();
            let new = require_str(&req.args, "new")?.to_string();
            Ok(CanonEvent::File(FileEvent::Patch(FilePatch { path, old, new })))
        }
        "bash" => {
            let cmd = require_str(&req.args, "cmd")?.to_string();
            let cwd = req.args.get("cwd").and_then(|v| v.as_str()).map(str::to_string);
            Ok(CanonEvent::Bash(BashInvoke { cmd, cwd }))
        }
        "llm.call" => {
            let prompt = require_str(&req.args, "prompt")?.to_string();
            let role = req.args.get("role").and_then(|v| v.as_str()).map(str::to_string);
            Ok(CanonEvent::Llm(LlmCall { prompt, role }))
        }
        "analysis.run" => {
            let crate_name = require_str(&req.args, "crate")?.to_string();
            let batch_id = req.args.get("batch_id").and_then(|v| v.as_str()).map(str::to_string);
            Ok(CanonEvent::Analysis(AnalysisEvent::Run(AnalysisRun { crate_name, batch_id })))
        }
        "analysis.workspace" => {
            Ok(CanonEvent::Analysis(AnalysisEvent::Workspace(AnalysisWorkspace {})))
        }
        name => Err(anyhow!("no decode for capability: {name}")),
    }
}
