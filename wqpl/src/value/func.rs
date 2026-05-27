use std::sync::Arc;

use crate::astnode::BinaryOperator;
use crate::value::Value;
use crate::value::cell::ValueCell;
use crate::vm::inst::{DebugStmtMark, Instruction};
use crate::wqdb::data::{ChunkId, DebugPcSpans, DebugProvenance, DebugStmtSpans};

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionData {
    pub(crate) params: Option<Arc<[String]>>,
    pub(crate) named_params: Option<Arc<[Arc<str>]>>,
    pub(crate) locals: u16,
    /// Shared immutable instruction array
    pub(crate) instructions: Arc<[Instruction]>,
    /// Debug chunk id for this function's code
    pub(crate) dbg_chunk: Option<ChunkId>,
    /// Statement spans for the function body (byte start,end in source)
    pub(crate) dbg_stmt_spans: Option<DebugStmtSpans>,
    /// Byte offset of the defining snippet within the source file.
    pub(crate) dbg_source_base_offset: usize,
    /// Exact per-pc statement spans when available.
    pub(crate) dbg_pc_spans: Option<DebugPcSpans>,
    /// Exact statement-ending PCs for debugger/backtrace mapping.
    pub(crate) dbg_stmt_marks: Option<Arc<[DebugStmtMark]>>,
    /// Local variable names by slot index (for wqdb)
    pub(crate) dbg_local_names: Option<Arc<[String]>>,
    /// Provenance frames for callable values returned from earlier calls.
    pub(crate) dbg_provenance: Option<DebugProvenance>,
}

#[derive(Debug, Clone)]
pub struct ClosureData {
    pub(crate) params: Option<Arc<[String]>>,
    pub(crate) named_params: Option<Arc<[Arc<str>]>>,
    pub(crate) locals: u16,
    pub(crate) captured: Arc<[ValueCell]>,
    /// Shared immutable instruction array
    pub(crate) instructions: Arc<[Instruction]>,
    /// Debug chunk id for this function's code
    pub(crate) dbg_chunk: Option<ChunkId>,
    /// Statement spans for the function body (byte start,end in source)
    pub(crate) dbg_stmt_spans: Option<DebugStmtSpans>,
    /// Byte offset of the defining snippet within the source file.
    pub(crate) dbg_source_base_offset: usize,
    /// Exact per-pc statement spans when available.
    pub(crate) dbg_pc_spans: Option<DebugPcSpans>,
    /// Exact statement-ending PCs for debugger/backtrace mapping.
    pub(crate) dbg_stmt_marks: Option<Arc<[DebugStmtMark]>>,
    /// Local variable names by slot index (for wqdb)
    pub(crate) dbg_local_names: Option<Arc<[String]>>,
    /// Provenance frames for callable values returned from earlier calls.
    pub(crate) dbg_provenance: Option<DebugProvenance>,
}

#[derive(Debug, Clone)]
pub struct FunctionCompositionData {
    pub(crate) op: BinaryOperator,
    pub(crate) left: Value,
    pub(crate) right: Value,
}
