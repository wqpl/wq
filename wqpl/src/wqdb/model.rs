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
