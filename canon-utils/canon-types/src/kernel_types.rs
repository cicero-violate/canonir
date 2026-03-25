use canon_proc_macros::{canon_event_enum, canon_event_struct};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

canon_event_struct!(SessionStart {
    project: String,
    #[serde(default)]
    schema: u64,
    #[serde(default)]
    byte_offset: u64,
});

canon_event_struct!(NodeDefined {
    symbol: String,
    kind: String,
    #[serde(default)]
    file: String,
    #[serde(default)]
    line: u32,
    #[serde(default)]
    col: u32,
    #[serde(default)]
    lo: u32,
    #[serde(default)]
    hi: u32,
});

canon_event_struct!(NodeUpdated {
    symbol: String,
    kind: String,
    #[serde(default)]
    file: String,
    #[serde(default)]
    line: u32,
    #[serde(default)]
    col: u32,
    #[serde(default)]
    lo: u32,
    #[serde(default)]
    hi: u32,
});

canon_event_struct!(NodeRemoved { symbol: String });
canon_event_struct!(EdgeDefined { src: String, dst: String, kind: String });
canon_event_struct!(EdgeRemoved { src: String, dst: String, kind: String });
canon_event_struct!(FileSeen { path: String });
canon_event_struct!(CallsiteObserved { kind: String, resolved: bool });
canon_event_struct!(SymbolDefined { symbol: String, kind: String });

canon_event_struct!(SpanDefined {
    symbol: String,
    #[serde(default)]
    file: String,
    #[serde(default)]
    line: u32,
    #[serde(default)]
    col: u32,
    #[serde(default)]
    lo: u32,
    #[serde(default)]
    hi: u32,
});

canon_event_struct!(PanicCaptured {
    def_id: String,
    message: String,
    #[serde(default)]
    mir_variant: Option<String>,
    #[serde(default)]
    lowering_stage: Option<String>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    span: Option<String>,
    #[serde(default)]
    frames: Vec<PanicFrame>,
});

canon_event_struct!(WarningCaptured { message: String });
canon_event_struct!(CompilationUnitFinished { crate_name: String });
canon_event_struct!(InvariantViolation { message: String });

canon_event_enum!(#[derive(serde::Serialize, serde::Deserialize)]
RustcEvent {
    SessionStart(SessionStart),
    NodeDefined(NodeDefined),
    NodeUpdated(NodeUpdated),
    NodeRemoved(NodeRemoved),
    EdgeDefined(EdgeDefined),
    EdgeRemoved(EdgeRemoved),
    FileSeen(FileSeen),
    CallsiteObserved(CallsiteObserved),
    SymbolDefined(SymbolDefined),
    SpanDefined(SpanDefined),
    PanicCaptured(PanicCaptured),
    WarningCaptured(WarningCaptured),
    CompilationUnitFinished(CompilationUnitFinished),
    InvariantViolation(InvariantViolation),
});

#[derive(Debug, Clone)]
pub struct EventDelta {
    pub id: u64,
    pub tick: u64,
    pub event: RustcEvent,
}

impl Default for EventDelta {
    fn default() -> Self {
        Self { id: 0, tick: 0, event: RustcEvent::SessionStart(Default::default()) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustcState {
    pub tick: u64,
    pub phase: String,
    #[serde(alias = "last_mutation_id")]
    pub last_event_id: u64,
    pub invariant_hash: String,
    pub graph_version: u64,
    #[serde(default)]
    pub known_symbols: HashMap<String, String>,
    #[serde(default)]
    pub known_edges: Vec<(String, String, String)>,
    #[serde(default)]
    pub known_files: HashSet<String>,
    #[serde(default)]
    pub removed_symbols: HashSet<String>,
    #[serde(default)]
    pub removed_edges: Vec<(String, String, String)>,
}

impl Default for RustcState {
    fn default() -> Self {
        Self {
            tick: 0,
            phase: String::new(),
            last_event_id: 0,
            invariant_hash: String::new(),
            graph_version: 0,
            known_symbols: HashMap::new(),
            known_edges: Vec::new(),
            known_files: HashSet::new(),
            removed_symbols: HashSet::new(),
            removed_edges: Vec::new(),
        }
    }
}

canon_event_struct!(PanicFrame {
    frame_index: usize,
    symbols: Vec<PanicSymbol>,
});

canon_event_struct!(PanicSymbol {
    symbol: String,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    line: Option<u32>,
});
