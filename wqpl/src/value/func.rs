use std::sync::Arc;

use crate::astnode::BinaryOperator;
use crate::value::Value;
use crate::value::cell::{self, ValueCell};
use crate::vm::inst::{DebugStmtMark, Instruction};
use crate::wqdb::data::{ChunkId, DebugChunkSpec, DebugPcSpans, DebugProvenance, DebugStmtSpans};

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

pub(crate) struct UserFunctionShape<'a> {
    pub(crate) params: &'a Option<Arc<[String]>>,
    pub(crate) named_params: &'a Option<Arc<[Arc<str>]>>,
    pub(crate) locals: u16,
    captured: Option<&'a Arc<[ValueCell]>>,
    pub(crate) instructions: &'a Arc<[Instruction]>,
    pub(crate) dbg_chunk: Option<ChunkId>,
    pub(crate) dbg_stmt_spans: &'a Option<DebugStmtSpans>,
    pub(crate) dbg_source_base_offset: usize,
    pub(crate) dbg_pc_spans: &'a Option<DebugPcSpans>,
    pub(crate) dbg_stmt_marks: &'a Option<Arc<[DebugStmtMark]>>,
    pub(crate) dbg_local_names: &'a Option<Arc<[String]>>,
}

impl UserFunctionShape<'_> {
    pub(crate) fn params_len(&self) -> Option<usize> {
        self.params.as_ref().map(|params| params.len())
    }

    pub(crate) fn captured(&self) -> Arc<[ValueCell]> {
        self.captured.cloned().unwrap_or_else(cell::empty_cells)
    }

    pub(crate) fn debug_spec(&self) -> DebugChunkSpec<'_> {
        DebugChunkSpec {
            dbg_chunk: self.dbg_chunk,
            instructions: self.instructions.as_ref(),
            dbg_stmt_spans: self.dbg_stmt_spans,
            source_base_offset: self.dbg_source_base_offset,
            dbg_pc_spans: self.dbg_pc_spans,
            dbg_stmt_marks: self.dbg_stmt_marks,
            dbg_local_names: self.dbg_local_names,
            params: self.params,
        }
    }
}

impl Value {
    pub(crate) fn as_user_function(&self) -> Option<UserFunctionShape<'_>> {
        match self {
            Value::CompiledFunction(f) => Some(UserFunctionShape {
                params: &f.params,
                named_params: &f.named_params,
                locals: f.locals,
                captured: None,
                instructions: &f.instructions,
                dbg_chunk: f.dbg_chunk,
                dbg_stmt_spans: &f.dbg_stmt_spans,
                dbg_source_base_offset: f.dbg_source_base_offset,
                dbg_pc_spans: &f.dbg_pc_spans,
                dbg_stmt_marks: &f.dbg_stmt_marks,
                dbg_local_names: &f.dbg_local_names,
            }),
            Value::Closure(c) => Some(UserFunctionShape {
                params: &c.params,
                named_params: &c.named_params,
                locals: c.locals,
                captured: Some(&c.captured),
                instructions: &c.instructions,
                dbg_chunk: c.dbg_chunk,
                dbg_stmt_spans: &c.dbg_stmt_spans,
                dbg_source_base_offset: c.dbg_source_base_offset,
                dbg_pc_spans: &c.dbg_pc_spans,
                dbg_stmt_marks: &c.dbg_stmt_marks,
                dbg_local_names: &c.dbg_local_names,
            }),
            _ => None,
        }
    }

    pub(crate) fn with_user_function_dbg_chunk(&self, dbg_chunk: Option<ChunkId>) -> Option<Self> {
        match self {
            Value::CompiledFunction(f) => {
                if f.dbg_chunk == dbg_chunk {
                    Some(self.clone())
                } else {
                    let mut new_f = FunctionData::clone(f);
                    new_f.dbg_chunk = dbg_chunk;
                    Some(Value::CompiledFunction(Arc::new(new_f)))
                }
            }
            Value::Closure(c) => {
                if c.dbg_chunk == dbg_chunk {
                    Some(self.clone())
                } else {
                    let mut new_c = ClosureData::clone(c);
                    new_c.dbg_chunk = dbg_chunk;
                    Some(Value::Closure(Arc::new(new_c)))
                }
            }
            _ => None,
        }
    }
}
