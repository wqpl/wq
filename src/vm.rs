pub mod compiler;
pub mod debug;
pub mod execloop;
pub mod fuse;
pub mod instruction;

mod cmpchain;
mod fastpath;

use std::sync::Arc;

use crate::{
    builtins::Builtins,
    value::{Value, ValueCell},
    vm::{debug::Backtrace, instruction::Instruction},
    wqdb::{ChunkId, DebugInfo, Wqdb},
};

use ahash::AHashMap;

pub type GlobalMap = AHashMap<String, Value>;

pub struct Vm {
    pub instructions: Arc<[Instruction]>,
    pc: usize,
    stack: Vec<Value>,
    /// Global variables
    globals: GlobalMap,
    builtins: Builtins,
    /// Stack of local slot frames
    locals: Vec<Vec<Slot>>,
    /// Stack of capture vectors (per frame), for closures
    captures: Vec<Vec<ValueCell>>,
    /// Inline caches for global lookups and call sites
    inline_cache: Vec<InlineCache>,
    /// Version number bumped whenever globals change
    global_version: u64,
    /// Stack of currently executing functions/closures for LoadSelf
    current_closure_stack: Vec<Value>,
    // args_scratch: Vec<Value>,

    // Debugging
    pub wqdb: Wqdb,
    pub debug_info: DebugInfo,
    current_chunk: ChunkId,
    call_stack: Vec<Frame>,
    /// Lightweight backtrace mode: build minimal debug info for frames on error
    bt_mode: bool,
    /// Base byte offset into current source file for this execution (for loader slices)
    debug_src_offset: usize,
    last_backtrace: Option<Backtrace>,
}

// #[derive(Clone)]
struct Frame {
    pub chunk: ChunkId,
    pub pc: usize,
    pub func_name: std::sync::Arc<str>,
}

#[derive(Clone)]
enum CallTarget {
    Cfn(ResolvedCfn),
    Closure(ResolvedClosure),
}

#[derive(Clone)]
pub struct ResolvedCfn {
    value: Value,
    params: Option<Arc<[String]>>,
    locals: u16,
    code: Arc<[Instruction]>,
    dbg_chunk: Option<ChunkId>,
}

#[derive(Clone)]
pub struct ResolvedClosure {
    value: Value,
    params: Option<Arc<[String]>>,
    locals: u16,
    captured: Vec<ValueCell>,
    code: Arc<[Instruction]>,
    dbg_chunk: Option<ChunkId>,
}

#[derive(Clone, Default)]
struct InlineCache {
    version: u64,
    value: Option<Value>,
    call_target: Option<CallTarget>,
}

#[derive(Clone)]
pub enum Slot {
    Value(Value),
    // Cell(ValueCell),
}

impl Default for Slot {
    fn default() -> Self {
        Slot::Value(Value::unit())
    }
}

impl Slot {
    pub fn read(&self) -> Value {
        match self {
            Slot::Value(v) => v.clone(),
            // Slot::Cell(cell) => cell.lock().expect("poisoned upvalue").clone(),
        }
    }

    pub fn write(&mut self, val: Value) {
        match self {
            Slot::Value(slot_val) => {
                *slot_val = val;
            } // Slot::Cell(cell) => {
              //     *cell.lock().expect("poisoned upvalue") = val;
              // }
        }
    }

    pub fn with_mut<R>(&mut self, f: impl FnOnce(&mut Value) -> R) -> R {
        match self {
            Slot::Value(slot_val) => f(slot_val),
            // Slot::Cell(cell) => {
            //     let mut guard = cell.lock().expect("poisoned upvalue");
            //     f(&mut guard)
            // }
        }
    }

    pub fn with_ref<R>(&self, f: impl FnOnce(&Value) -> R) -> R {
        match self {
            Slot::Value(slot_val) => f(slot_val),
            // Slot::Cell(cell) => {
            //     let guard = cell.lock().expect("poisoned upvalue");
            //     f(&guard)
            // }
        }
    }

    // pub fn ensure_cell(&mut self) -> ValueCell {
    //     match self {
    //         Slot::Cell(cell) => cell.clone(),
    //         Slot::Value(slot_val) => {
    //             let current = std::mem::replace(slot_val, Value::unit());
    //             let cell = Arc::new(Mutex::new(current));
    //             *self = Slot::Cell(cell.clone());
    //             cell
    //         }
    //     }
    // }
}

impl Vm {
    #[inline]
    fn is_internal_ephemeral(&self, name: &str) -> bool {
        // Loop-counter and internal per-iteration temporaries updated very frequently.
        // Skipping global_version bumps for these prevents flushing all LoadVar caches
        // on every loop iteration while remaining correct when paired with
        // non-caching loads for these names in the executor.
        name == "_n" || name.starts_with("--vm-n-loop-old-") || name.starts_with("--vm-n-loop-res-")
    }
    pub fn new(instructions: Vec<Instruction>) -> Self {
        let len = instructions.len();
        Vm {
            instructions: Arc::<[Instruction]>::from(instructions),
            pc: 0,
            stack: Vec::with_capacity(256),
            globals: AHashMap::new(),
            builtins: Builtins::new(),
            locals: Vec::new(),
            captures: Vec::new(),
            inline_cache: vec![InlineCache::default(); len],
            global_version: 0,
            current_closure_stack: Vec::new(),
            // args_scratch: Vec::new(),
            wqdb: Wqdb::default(),
            debug_info: DebugInfo::default(),
            current_chunk: ChunkId(0),
            call_stack: Vec::new(),
            bt_mode: false,
            debug_src_offset: 0,
            last_backtrace: None,
        }
    }

    /// Replace instructions and reset execution state.
    pub fn reset(&mut self, instructions: Vec<Instruction>) {
        self.instructions = Arc::<[Instruction]>::from(instructions);
        self.pc = 0;
        self.stack.clear();
        self.locals.clear();
        self.inline_cache = vec![InlineCache::default(); self.instructions.len()];
        self.current_closure_stack.clear();
        // Ensure no stale frames leak across runs (affects backtraces)
        self.call_stack.clear();
        // Keep debug_src_offset as set by evaluator for current run
    }

    /// Access the global environment.
    pub fn global_env(&self) -> &GlobalMap {
        &self.globals
    }

    /// Mutable access to the global environment.
    pub fn global_env_mut(&mut self) -> &mut GlobalMap {
        &mut self.globals
    }

    pub fn current_chunk_id(&self) -> ChunkId {
        self.current_chunk
    }

    fn lookup_global(&self, name: &str) -> Option<Value> {
        self.globals.get(name).cloned()
    }

    fn assign_global(&mut self, name: &str, mut value: Value) {
        if self.wqdb.enabled || self.bt_mode {
            match &mut value {
                Value::CompiledFunction {
                    params,
                    instructions,
                    dbg_chunk,
                    dbg_stmt_spans,
                    dbg_local_names,
                    ..
                }
                | Value::Closure {
                    params,
                    instructions,
                    dbg_chunk,
                    dbg_stmt_spans,
                    dbg_local_names,
                    ..
                } => {
                    let chunk = self.ensure_dbg_chunk_with_spans(
                        name,
                        *dbg_chunk,
                        instructions.as_ref(),
                        dbg_stmt_spans,
                        dbg_local_names,
                        params,
                    );
                    *dbg_chunk = chunk;
                }
                _ => {}
            }
        }
        self.globals.insert(name.to_string(), value);
        // Frequent updates to ephemeral loop vars (_n, --vm-n-loop-old-*, --vm-n-loop-res-*)
        // should not invalidate all inline caches; keep version stable for them.
        if !self.is_internal_ephemeral(name) {
            self.global_version += 1;
        }
    }
}
