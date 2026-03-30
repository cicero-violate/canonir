use canon_proc_macros::{canon_event_enum, canon_event_struct};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

canon_event_struct!(
    #[event(class = "Effect")]
    SessionStart {
        #[input]
        project: String,
        #[serde(default)]
        #[delta]
        #[output]
        schema: u64,
        #[serde(default)]
        byte_offset: u64,
    }
);

canon_event_struct!(
    #[event(class = "Effect")]
    NodeDefined {
        #[input]
        symbol: String,
        #[input]
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
        #[delta]
        #[output]
        hi: u32,
    }
);

canon_event_struct!(
    #[event(class = "Effect")]
    NodeUpdated {
        #[input]
        symbol: String,
        #[input]
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
        #[delta]
        #[output]
        hi: u32,
    }
);

canon_event_struct!(
    #[event(class = "Effect")]
    NodeRemoved {
        #[input]
        symbol: String,
        #[delta]
        #[output]
        removed: bool,
    }
);

canon_event_struct!(
    #[event(class = "Effect")]
    EdgeDefined {
        #[input]
        src: String,
        #[input]
        dst: String,
        #[input]
        kind: String,
        #[delta]
        #[output]
        defined: bool,
    }
);

canon_event_struct!(
    #[event(class = "Effect")]
    EdgeRemoved {
        #[input]
        src: String,
        #[input]
        dst: String,
        #[input]
        kind: String,
        #[delta]
        #[output]
        removed: bool,
    }
);

canon_event_struct!(
    #[event(class = "Effect")]
    FileSeen {
        #[input]
        path: String,
        #[delta]
        #[output]
        seen: bool,
    }
);

canon_event_struct!(
    #[event(class = "Effect")]
    CallsiteObserved {
        #[input]
        kind: String,
        #[delta]
        #[output]
        resolved: bool,
    }
);

canon_event_struct!(
    #[event(class = "Effect")]
    SymbolDefined {
        #[input]
        symbol: String,
        #[input]
        kind: String,
        #[delta]
        #[output]
        defined: bool,
    }
);

canon_event_struct!(
    #[event(class = "Effect")]
    SpanDefined {
        #[input]
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
        #[delta]
        #[output]
        hi: u32,
    }
);

canon_event_struct!(#[event(class = "Effect")] PanicCaptured {
    #[input] def_id: String,
    #[input] message: String,
    #[serde(default)]
    mir_variant: Option<String>,
    #[serde(default)]
    lowering_stage: Option<String>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    span: Option<String>,
    #[serde(default)]
    #[delta]
    #[output] frames: Vec<PanicFrame>,
});

canon_event_struct!(
    #[event(class = "Effect")]
    WarningCaptured {
        #[input]
        message: String,
        #[delta]
        #[output]
        captured: bool,
    }
);

canon_event_struct!(
    #[event(class = "Effect")]
    CompilationUnitFinished {
        #[input]
        crate_name: String,
        #[delta]
        #[output]
        finished: bool,
    }
);

canon_event_struct!(
    #[event(class = "Effect")]
    InvariantViolation {
        #[input]
        message: String,
        #[delta]
        #[output]
        recorded: bool,
    }
);

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDelta {
    pub id: u64,
    pub tick: u64,
    pub event: RustcEvent,
}

impl Default for EventDelta {
    fn default() -> Self {
        Self { id: 0, tick: 0, event: RustcEvent::SessionStart(SessionStart { project: String::new(), schema: 0, byte_offset: 0 }) }
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

canon_event_struct!(#[event(class = "Effect")] PanicFrame {
    #[input] frame_index: usize,
    #[delta]
    #[output] symbols: Vec<PanicSymbol>,
});

canon_event_struct!(#[event(class = "Effect")] PanicSymbol {
    #[input] symbol: String,
    #[serde(default)]
    #[delta]
    #[output] file: Option<String>,
    #[serde(default)]
    line: Option<u32>,
});
