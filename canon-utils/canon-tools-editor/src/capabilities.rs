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

fn emit_edit(event: EditEvent) -> CapabilityExecutionResult {
    CapabilityExecutionResult::Emit(CanonEvent::Edit(event))
}

struct RenameSymbolCapability;

impl CapabilityHandler for RenameSymbolCapability {
    fn name(&self) -> &'static str {
        CAP_RENAME_SYMBOL
    }

    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        match ctx.event {
            CanonEvent::Edit(EditEvent::RenameSymbol(ev)) => Ok(emit_edit(EditEvent::RenameSymbol(ev))),
            _ => Ok(CapabilityExecutionResult::NoOp),
        }
    }
}

struct MoveSymbolCapability;

impl CapabilityHandler for MoveSymbolCapability {
    fn name(&self) -> &'static str {
        CAP_MOVE_SYMBOL
    }

    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        match ctx.event {
            CanonEvent::Edit(EditEvent::MoveSymbol(ev)) => Ok(emit_edit(EditEvent::MoveSymbol(ev))),
            _ => Ok(CapabilityExecutionResult::NoOp),
        }
    }
}

struct DeleteSymbolCapability;

impl CapabilityHandler for DeleteSymbolCapability {
    fn name(&self) -> &'static str {
        CAP_DELETE_SYMBOL
    }

    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        match ctx.event {
            CanonEvent::Edit(EditEvent::DeleteSymbol(ev)) => Ok(emit_edit(EditEvent::DeleteSymbol(ev))),
            _ => Ok(CapabilityExecutionResult::NoOp),
        }
    }
}

struct RenameModuleCapability;

impl CapabilityHandler for RenameModuleCapability {
    fn name(&self) -> &'static str {
        CAP_RENAME_MODULE
    }

    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        match ctx.event {
            CanonEvent::Edit(EditEvent::RenameModule(ev)) => Ok(emit_edit(EditEvent::RenameModule(ev))),
            _ => Ok(CapabilityExecutionResult::NoOp),
        }
    }
}

struct RenameDirCapability;

impl CapabilityHandler for RenameDirCapability {
    fn name(&self) -> &'static str {
        CAP_RENAME_DIR
    }

    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        match ctx.event {
            CanonEvent::Edit(EditEvent::RenameDir(ev)) => Ok(emit_edit(EditEvent::RenameDir(ev))),
            _ => Ok(CapabilityExecutionResult::NoOp),
        }
    }
}
