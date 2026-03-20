use canon_capability::{CapabilityExecutionContext, CapabilityExecutionResult, CapabilityHandler, CapabilityRegistry};
use canon_event::{CanonEvent, EditEvent};

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
    args.get(key).and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("missing or invalid arg: {key}"))
}

fn emit_edit(event: EditEvent) -> CapabilityExecutionResult {
    CapabilityExecutionResult::Emit(CanonEvent::Edit(event))
}

struct RenameSymbolCapability;

impl CapabilityHandler for RenameSymbolCapability {
    fn name(&self) -> &'static str {
        CAP_RENAME_SYMBOL
    }

    fn execute(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        let CanonEvent::CapabilityRequested(request) = ctx.event else {
            anyhow::bail!("capability context missing request");
        };
        let project = require_arg(&request.args, "project")?;
        let old = require_arg(&request.args, "old")?;
        let new = require_arg(&request.args, "new")?;
        Ok(emit_edit(EditEvent::RenameSymbol(canon_event::RenameSymbol { project: project.to_string(), old: old.to_string(), new: new.to_string() })))
    }
}

struct MoveSymbolCapability;

impl CapabilityHandler for MoveSymbolCapability {
    fn name(&self) -> &'static str {
        CAP_MOVE_SYMBOL
    }

    fn execute(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        let CanonEvent::CapabilityRequested(request) = ctx.event else {
            anyhow::bail!("capability context missing request");
        };
        let project = require_arg(&request.args, "project")?;
        let symbol = require_arg(&request.args, "symbol")?;
        let module = require_arg(&request.args, "module")?;
        Ok(emit_edit(EditEvent::MoveSymbol(canon_event::MoveSymbol { project: project.to_string(), symbol: symbol.to_string(), module: module.to_string() })))
    }
}

struct DeleteSymbolCapability;

impl CapabilityHandler for DeleteSymbolCapability {
    fn name(&self) -> &'static str {
        CAP_DELETE_SYMBOL
    }

    fn execute(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        let CanonEvent::CapabilityRequested(request) = ctx.event else {
            anyhow::bail!("capability context missing request");
        };
        let project = require_arg(&request.args, "project")?;
        let symbol = require_arg(&request.args, "symbol")?;
        Ok(emit_edit(EditEvent::DeleteSymbol(canon_event::DeleteSymbol { project: project.to_string(), symbol: symbol.to_string() })))
    }
}

struct RenameModuleCapability;

impl CapabilityHandler for RenameModuleCapability {
    fn name(&self) -> &'static str {
        CAP_RENAME_MODULE
    }

    fn execute(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        let CanonEvent::CapabilityRequested(request) = ctx.event else {
            anyhow::bail!("capability context missing request");
        };
        let project = require_arg(&request.args, "project")?;
        let old = require_arg(&request.args, "old")?;
        let new = require_arg(&request.args, "new")?;
        Ok(emit_edit(EditEvent::RenameModule(canon_event::RenameModule { project: project.to_string(), old: old.to_string(), new: new.to_string() })))
    }
}

struct RenameDirCapability;

impl CapabilityHandler for RenameDirCapability {
    fn name(&self) -> &'static str {
        CAP_RENAME_DIR
    }

    fn execute(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        let CanonEvent::CapabilityRequested(request) = ctx.event else {
            anyhow::bail!("capability context missing request");
        };
        let project = require_arg(&request.args, "project")?;
        let old = require_arg(&request.args, "old")?;
        let new = require_arg(&request.args, "new")?;
        Ok(emit_edit(EditEvent::RenameDir(canon_event::RenameDir { project: project.to_string(), old: old.into(), new: new.into() })))
    }
}
