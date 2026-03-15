use canon_capability_engine::{Capability, CapabilityContext, CapabilityRegistry, CapabilityResult};
use canon_event::emit_debug::{error, info};
use canon_event::{EditEvent, RuntimeEvent};

pub const CAP_RENAME_SYMBOL: &str = "edit.rename_symbol";
pub const CAP_MOVE_SYMBOL: &str = "edit.move_symbol";
pub const CAP_DELETE_SYMBOL: &str = "edit.delete_symbol";
pub const CAP_RENAME_MODULE: &str = "edit.rename_module";
pub const CAP_RENAME_DIR: &str = "edit.rename_dir";

pub fn register_editor_capabilities(registry: &mut CapabilityRegistry) {
    registry.register(std::sync::Arc::new(RenameSymbolCapability));
    registry.register(std::sync::Arc::new(MoveSymbolCapability));
    registry.register(std::sync::Arc::new(DeleteSymbolCapability));
    registry.register(std::sync::Arc::new(RenameModuleCapability));
    registry.register(std::sync::Arc::new(RenameDirCapability));
}

fn require_arg<'a>(args: &'a serde_json::Value, key: &str) -> anyhow::Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing or invalid arg: {key}"))
}

fn emit_edit(event: EditEvent) -> CapabilityResult {
    CapabilityResult::Emit(RuntimeEvent::Edit(event))
}

struct RenameSymbolCapability;

impl Capability for RenameSymbolCapability {
    fn name(&self) -> &'static str {
        CAP_RENAME_SYMBOL
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let RuntimeEvent::CapabilityRequested(request) = ctx.event else {
            anyhow::bail!("capability context missing request");
        };
        let project = require_arg(&request.args, "project")?;
        let old = require_arg(&request.args, "old")?;
        let new = require_arg(&request.args, "new")?;
        info(
            "edit_capability",
            "rename_symbol",
            serde_json::json!({ "project": project, "old": old, "new": new }),
        );
        Ok(emit_edit(EditEvent::RenameSymbol {
            project: project.to_string(),
            old: old.to_string(),
            new: new.to_string(),
        }))
    }
}

struct MoveSymbolCapability;

impl Capability for MoveSymbolCapability {
    fn name(&self) -> &'static str {
        CAP_MOVE_SYMBOL
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let RuntimeEvent::CapabilityRequested(request) = ctx.event else {
            anyhow::bail!("capability context missing request");
        };
        let project = require_arg(&request.args, "project")?;
        let symbol = require_arg(&request.args, "symbol")?;
        let module = require_arg(&request.args, "module")?;
        info(
            "edit_capability",
            "move_symbol",
            serde_json::json!({ "project": project, "symbol": symbol, "module": module }),
        );
        Ok(emit_edit(EditEvent::MoveSymbol {
            project: project.to_string(),
            symbol: symbol.to_string(),
            module: module.to_string(),
        }))
    }
}

struct DeleteSymbolCapability;

impl Capability for DeleteSymbolCapability {
    fn name(&self) -> &'static str {
        CAP_DELETE_SYMBOL
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let RuntimeEvent::CapabilityRequested(request) = ctx.event else {
            anyhow::bail!("capability context missing request");
        };
        let project = require_arg(&request.args, "project")?;
        let symbol = require_arg(&request.args, "symbol")?;
        info(
            "edit_capability",
            "delete_symbol",
            serde_json::json!({ "project": project, "symbol": symbol }),
        );
        Ok(emit_edit(EditEvent::DeleteSymbol {
            project: project.to_string(),
            symbol: symbol.to_string(),
        }))
    }
}

struct RenameModuleCapability;

impl Capability for RenameModuleCapability {
    fn name(&self) -> &'static str {
        CAP_RENAME_MODULE
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let RuntimeEvent::CapabilityRequested(request) = ctx.event else {
            anyhow::bail!("capability context missing request");
        };
        let project = require_arg(&request.args, "project")?;
        let old = require_arg(&request.args, "old")?;
        let new = require_arg(&request.args, "new")?;
        info(
            "edit_capability",
            "rename_module",
            serde_json::json!({ "project": project, "old": old, "new": new }),
        );
        Ok(emit_edit(EditEvent::RenameModule {
            project: project.to_string(),
            old: old.to_string(),
            new: new.to_string(),
        }))
    }
}

struct RenameDirCapability;

impl Capability for RenameDirCapability {
    fn name(&self) -> &'static str {
        CAP_RENAME_DIR
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let RuntimeEvent::CapabilityRequested(request) = ctx.event else {
            anyhow::bail!("capability context missing request");
        };
        let project = require_arg(&request.args, "project")?;
        let old = require_arg(&request.args, "old")?;
        let new = require_arg(&request.args, "new")?;
        info(
            "edit_capability",
            "rename_dir",
            serde_json::json!({ "project": project, "old": old, "new": new }),
        );
        Ok(emit_edit(EditEvent::RenameDir {
            project: project.to_string(),
            old: old.into(),
            new: new.into(),
        }))
    }
}

fn _log_error(action: &str, err: &str) {
    error(
        "edit_capability",
        "capability_failed",
        serde_json::json!({ "action": action, "error": err }),
    );
}
