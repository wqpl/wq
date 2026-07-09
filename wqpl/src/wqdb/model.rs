use crate::wqdb::data::ChunkId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakpointKind {
    Persistent,
    Pause,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Breakpoint {
    pub id: usize,
    pub enabled: bool,
    pub kind: BreakpointKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepMode {
    None,
    In,
    Over,
    Out,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum StepGranularity {
    /// Pause once per source line in each frame.
    Line,
    /// Pause at every semicolon-separated expression boundary.
    #[default]
    Expr,
    /// Pause before every VM instruction.
    Inst,
}

impl StepGranularity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Expr => "expr",
            Self::Inst => "inst",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "line" | "l" => Some(Self::Line),
            "expr" | "expression" | "e" => Some(Self::Expr),
            "inst" | "instruction" | "i" => Some(Self::Inst),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SymbolTrackTarget {
    Global {
        name: String,
    },
    Local {
        chunk: ChunkId,
        slot: u16,
        name: String,
    },
    Capture {
        chunk: ChunkId,
        slot: u16,
        name: Option<String>,
    },
}

impl SymbolTrackTarget {
    pub fn matches_event(&self, event: &SymbolTrackTarget) -> bool {
        match (self, event) {
            (Self::Global { name: a }, Self::Global { name: b }) => a == b,
            (
                Self::Local {
                    chunk: chunk_a,
                    slot: slot_a,
                    ..
                },
                Self::Local {
                    chunk: chunk_b,
                    slot: slot_b,
                    ..
                },
            )
            | (
                Self::Capture {
                    chunk: chunk_a,
                    slot: slot_a,
                    ..
                },
                Self::Capture {
                    chunk: chunk_b,
                    slot: slot_b,
                    ..
                },
            ) => chunk_a == chunk_b && slot_a == slot_b,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolTracker {
    pub id: usize,
    pub enabled: bool,
    pub target: SymbolTrackTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopHook {
    pub id: usize,
    pub enabled: bool,
    pub command: String,
}
