pub mod compiler;
pub mod instruction;

use crate::astnode::{BinaryOperator, UnaryOperator};
use crate::builtins::Builtins;
use crate::value::{Value, WqResult};
use crate::wqdb::{
    ChunkId, CodeLoc, DebugHost, DebugInfo, Wqdb, apply_stmt_spans_exact_offs, mark_stmt_heuristic,
};
use crate::wqerror::WqError;
use compiler::Compiler;
use indexmap::IndexMap;
use instruction::{Capture, Instruction};
use std::collections::HashMap;

type Backtrace = Vec<(CodeLoc, std::sync::Arc<str>)>;

// thread_local! {
//     pub static LAST_BT: std::cell::RefCell<Option<Backtrace>> =
//         const { std::cell::RefCell::new(None) };
// }

#[derive(Clone, Default)]
struct InlineCache {
    version: u64,
    value: Option<Value>,
}

pub struct Vm {
    pub instructions: Vec<Instruction>,
    pc: usize,
    stack: Vec<Value>,
    /// Global variables
    globals: HashMap<String, Value>,
    builtins: Builtins,
    /// Stack of local slot frames
    locals: Vec<Vec<Value>>,
    /// Stack of capture vectors (per frame), for closures
    captures: Vec<Vec<Value>>,
    /// Inline caches for global lookups and call sites
    inline_cache: Vec<InlineCache>,
    /// Version number bumped whenever globals change
    global_version: u64,

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

impl Vm {
    #[inline]
    pub fn clear_last_bt(&mut self) {
        self.last_backtrace = None;
    }
    #[inline]
    pub fn take_last_bt(&mut self) -> Option<Backtrace> {
        self.last_backtrace.take()
    }
    #[inline]
    fn capture_bt_if_empty(&mut self) {
        if self.last_backtrace.is_none() {
            self.last_backtrace = Some(self.bt_frames());
        }
    }
}

// #[derive(Clone)]
struct Frame {
    pub chunk: ChunkId,
    pub pc: usize,
    pub func_name: std::sync::Arc<str>,
}

impl Vm {
    pub fn set_bt_mode(&mut self, flag: bool) {
        self.bt_mode = flag;
    }
    pub fn new(instructions: Vec<Instruction>) -> Self {
        let len = instructions.len();
        Vm {
            instructions,
            pc: 0,
            stack: Vec::with_capacity(256),
            globals: HashMap::new(),
            builtins: Builtins::new(),
            locals: Vec::new(),
            captures: Vec::new(),
            inline_cache: vec![InlineCache::default(); len],
            global_version: 0,
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
        self.instructions = instructions;
        self.pc = 0;
        self.stack.clear();
        self.locals.clear();
        self.inline_cache = vec![InlineCache::default(); self.instructions.len()];
        // Ensure no stale frames leak across runs (affects backtraces)
        self.call_stack.clear();
        // Keep debug_src_offset as set by evaluator for current run
    }

    /// Prepare debug info for a top-level script run in the REPL.
    /// Creates a new source file and a script chunk and selects it as current.
    pub fn debug_prepare_script(&mut self, virtual_path: &str, source: &str) {
        if !(self.wqdb.enabled || self.bt_mode) {
            return;
        }
        let file_id = self
            .debug_info
            .new_file(virtual_path.to_string(), source.to_string());
        let len = self.instructions.len();
        let chunk = self.debug_info.new_chunk("<script>", file_id, len);
        self.current_chunk = chunk;
    }

    pub fn set_debug_src_offset(&mut self, offs: usize) {
        self.debug_src_offset = offs;
    }
    pub fn debug_src_offset(&self) -> usize {
        self.debug_src_offset
    }

    /// Access the global environment.
    pub fn global_env(&self) -> &HashMap<String, Value> {
        &self.globals
    }

    /// Mutable access to the global environment.
    pub fn global_env_mut(&mut self) -> &mut HashMap<String, Value> {
        &mut self.globals
    }

    pub fn current_chunk_id(&self) -> ChunkId {
        self.current_chunk
    }

    fn lookup_global(&self, name: &str) -> Option<Value> {
        self.globals.get(name).cloned()
    }

    fn assign_global(&mut self, name: &str, value: Value) {
        self.globals.insert(name.to_string(), value);
        self.global_version += 1;
    }

    #[allow(clippy::too_many_arguments)]
    fn call_function(
        &mut self,
        instructions: Vec<Instruction>,
        params: Option<Vec<String>>,
        local_count: u16,
        captured: Vec<Value>,
        args: Vec<Value>,
        callee_name: Option<&str>,
        dbg_chunk: Option<ChunkId>,
    ) -> WqResult<Value> {
        // Determine or create a debug chunk for the callee (only if debugging)
        let callee_chunk = if self.wqdb.enabled || self.bt_mode {
            if let Some(id) = dbg_chunk {
                id
            } else {
                let file_id = self.debug_info.chunk(self.current_chunk).file_id;
                let id = self.debug_info.new_chunk(
                    callee_name.unwrap_or("<fn>").to_string(),
                    file_id,
                    instructions.len(),
                );
                // Heuristically mark statements in the callee for stepping (no spans here)
                {
                    let table = &mut self.debug_info.chunk_mut(id).line_table;
                    mark_stmt_heuristic(table, &instructions);
                }
                id
            }
        } else {
            self.current_chunk
        };

        let saved_instructions = std::mem::replace(&mut self.instructions, instructions);
        let saved_pc = self.pc;
        // Preserve stack allocation to avoid reallocations during the call
        let prev_cap = self.stack.capacity();
        let mut saved_stack = std::mem::replace(
            &mut self.stack,
            Vec::with_capacity(std::cmp::max(prev_cap, 256)),
        );
        let saved_cache = std::mem::replace(
            &mut self.inline_cache,
            vec![InlineCache::default(); self.instructions.len()],
        );
        // Push a debug frame and switch chunk/pc to callee
        let mut pushed_dbg = false;
        if self.wqdb.enabled || self.bt_mode {
            let caller_chunk = self.current_chunk;
            self.call_stack.push(Frame {
                chunk: caller_chunk,
                pc: saved_pc,
                func_name: std::sync::Arc::from(self.func_name_for_chunk(caller_chunk)),
            });
            self.current_chunk = callee_chunk;
            pushed_dbg = true;
        }
        self.pc = 0;

        let mut frame = vec![Value::unit(); local_count as usize];
        // let capture_count = captured.len();
        if let Some(p) = params {
            if args.len() != p.len() {
                return Err(WqError::ArityError(format!(
                    "function expects {} args, got {}",
                    p.len(),
                    args.len()
                )));
            }
            for (i, arg) in args.into_iter().enumerate() {
                frame[i] = arg;
            }
        } else {
            if args.len() > 3 {
                return Err(WqError::ArityError(
                    "implicit function expects up to 3 args".to_string(),
                ));
            }
            for (i, arg) in args.into_iter().enumerate() {
                frame[i] = arg;
            }
        }
        self.locals.push(frame);
        self.captures.push(captured);
        let limit = self.instructions.len();
        // let res = self.execute_until(limit);
        let res = match self.execute_until(limit) {
            Ok(v) => Ok(v),
            Err(e) => {
                self.capture_bt_if_empty(); // keep innermost snapshot
                Err(e)
            }
        };

        // Always unwind local frames regardless of call result
        self.locals.pop();
        self.captures.pop();

        // Restore caller VM state before propagating result
        std::mem::swap(&mut self.stack, &mut saved_stack);
        self.instructions = saved_instructions;
        self.pc = saved_pc;
        self.inline_cache = saved_cache;
        // Pop debug frame and restore caller chunk
        if pushed_dbg {
            if let Some(fr) = self.call_stack.pop() {
                self.current_chunk = fr.chunk;
            }
        }
        // Return the callee result (Ok or Err) after full restoration
        res
    }

    pub fn run(&mut self) -> WqResult<Value> {
        let limit = self.instructions.len();
        self.execute_until(limit)
    }

    fn execute_until(&mut self, limit: usize) -> WqResult<Value> {
        while self.pc < limit {
            if self.wqdb.enabled {
                let here = CodeLoc {
                    chunk: self.current_chunk,
                    pc: self.pc,
                };
                let depth = self.call_depth();
                // Check first, then perform pause bookkeeping and call hook without borrowing conflicts
                if self.wqdb.should_pause_at(&self.debug_info, here, depth) {
                    let cb = self.wqdb.on_pause;
                    self.wqdb.note_pause(here);
                    if let Some(f) = cb {
                        f(self);
                    }
                }
            }
            let idx = self.pc;
            // Borrow current instruction by reference to avoid cloning per step
            let op_ref = unsafe { self.instructions.get_unchecked(idx) };
            // advance program counter
            self.pc += 1;

            match op_ref {
                Instruction::LoadConst(v) => self.stack.push(v.clone()),
                Instruction::LoadVar(name) => {
                    let (cver, cval) = {
                        let c = &self.inline_cache[idx];
                        (c.version, c.value.as_ref())
                    };
                    if cver == self.global_version {
                        if let Some(v) = cval {
                            self.stack.push(v.clone());
                            continue;
                        }
                    }
                    let (val, ver) = if let Some(val) = self.lookup_global(name) {
                        (val, self.global_version)
                    } else if self.builtins.has_function(name) {
                        (Value::BuiltinFunction(name.clone()), u64::MAX)
                    } else {
                        return Err(WqError::ValueError(format!("`{name}` is not defined")));
                    };
                    {
                        let c = &mut self.inline_cache[idx];
                        c.version = ver;
                        c.value = Some(val.clone());
                    }
                    self.stack.push(val);
                }
                Instruction::StoreVar(name) => {
                    let name_owned = name.clone();
                    let val = self.stack.pop().ok_or_else(|| {
                        WqError::VmError(format!(
                            "stack underflow: cannot store into variable '{name_owned}'",
                        ))
                    })?;
                    self.assign_global(&name_owned, val);
                }
                Instruction::StoreVarKeep(name) => {
                    let name_owned = name.clone();
                    let val = self.stack.last().cloned().ok_or_else(|| {
                        WqError::VmError(format!(
                            "stack underflow: cannot store into variable '{name_owned}'",
                        ))
                    })?;
                    self.assign_global(&name_owned, val);
                }
                Instruction::LoadLocal(i) => {
                    let val = self
                        .locals
                        .last()
                        .and_then(|f| f.get(*i as usize))
                        .ok_or_else(|| WqError::VmError(format!("invalid local slot {i}")))?;
                    self.stack.push(val.clone());
                }
                Instruction::StoreLocal(i) => {
                    let val = self.stack.pop().ok_or_else(|| {
                        WqError::VmError(format!(
                            "stack underflow: cannot store into local slot {i}",
                        ))
                    })?;
                    if let Some(frame) = self.locals.last_mut() {
                        if let Some(slot) = frame.get_mut(*i as usize) {
                            *slot = val;
                        } else {
                            return Err(WqError::VmError(format!("invalid local slot {i}")));
                        }
                    } else {
                        return Err(WqError::VmError("no local frame".into()));
                    }
                }
                Instruction::StoreLocalKeep(i) => {
                    let val = self.stack.last().ok_or_else(|| {
                        WqError::VmError(format!(
                            "stack underflow: cannot store into local slot {i}",
                        ))
                    })?;
                    if let Some(frame) = self.locals.last_mut() {
                        if let Some(slot) = frame.get_mut(*i as usize) {
                            *slot = val.clone();
                        } else {
                            return Err(WqError::VmError(format!("invalid local slot {i}")));
                        }
                    } else {
                        return Err(WqError::VmError("no local frame".into()));
                    }
                }
                Instruction::BinaryOp(op) => {
                    let right = self.stack.pop().ok_or_else(|| {
                        WqError::VmError("stack underflow: missing right operand".into())
                    })?;
                    let left = self.stack.pop().ok_or_else(|| {
                        WqError::VmError("stack underflow: missing left operand".into())
                    })?;

                    let result = match op {
                        BinaryOperator::Add => left.add(&right),
                        BinaryOperator::Subtract => left.subtract(&right),
                        BinaryOperator::Multiply => left.multiply(&right),
                        BinaryOperator::Power => left.power(&right),
                        BinaryOperator::Divide => left.divide(&right),
                        BinaryOperator::DivideDot => left.divide_dot(&right),
                        BinaryOperator::Modulo => left.modulo(&right),
                        BinaryOperator::ModuloDot => left.modulo_dot(&right),
                        BinaryOperator::Equal => left.eq(&right),
                        BinaryOperator::NotEqual => left.neq(&right),
                        BinaryOperator::LessThan => left.lt(&right),
                        BinaryOperator::LessThanOrEqual => left.leq(&right),
                        BinaryOperator::GreaterThan => left.gt(&right),
                        BinaryOperator::GreaterThanOrEqual => left.geq(&right),
                    };

                    match result {
                        Ok(v) => self.stack.push(v),
                        Err(e) => return Err(e),
                    }
                }
                Instruction::UnaryOp(op) => {
                    let val = self.stack.pop().ok_or_else(|| {
                        WqError::VmError(
                            "stack underflow: missing operand for unary operator".into(),
                        )
                    })?;

                    let result = match op {
                        UnaryOperator::Negate => val.neg(),
                        UnaryOperator::Count => Ok(Value::Int(val.len() as i64)),
                    }?;
                    self.stack.push(result);
                }
                Instruction::CallBuiltin(name, argc) => {
                    let argc_val = *argc;
                    if self.stack.len() < argc_val {
                        return Err(WqError::VmError(format!(
                            "stack underflow: expected {argc_val} args for builtin '{name}'",
                        )));
                    }
                    let base = self.stack.len() - argc_val;
                    let result = self.builtins.call(name, &self.stack[base..])?;
                    self.stack.truncate(base);
                    self.stack.push(result);
                }
                Instruction::CallBuiltinId(id, argc) => {
                    if self.stack.len() < *argc {
                        return Err(WqError::VmError(format!(
                            "stack underflow: expected {argc} args for builtin ID {id}",
                        )));
                    }
                    let base = self.stack.len() - *argc;
                    let result = self.builtins.call_id(*id as usize, &self.stack[base..])?;
                    self.stack.truncate(base);
                    self.stack.push(result);
                }
                Instruction::CallLocal(slot, argc) => {
                    if self.stack.len() < *argc {
                        return Err(WqError::VmError(format!(
                            "stack underflow: expected {argc} args for local function at slot {slot}",
                        )));
                    }
                    let base = self.stack.len() - *argc;
                    let args = self.stack.split_off(base);

                    // Walk frames top-down looking for the callable value
                    enum LocalCallable {
                        Func {
                            params: Option<Vec<String>>,
                            locals: u16,
                            instructions: Vec<Instruction>,
                            captured: Vec<Value>,
                            dbg_chunk: Option<ChunkId>,
                            name_hint: Option<String>,
                        },
                        Builtin(String),
                    }

                    let callable = {
                        let mut found: Option<LocalCallable> = None;
                        let base_offs = self.debug_src_offset();
                        for frame in self.locals.iter_mut().rev() {
                            if let Some(v) = frame.get_mut(*slot as usize) {
                                match v {
                                    Value::CompiledFunction {
                                        params,
                                        locals,
                                        instructions,
                                        dbg_chunk,
                                        dbg_stmt_spans,
                                        dbg_local_names,
                                    } => {
                                        // ensure chunk exists and is mapped (wqdb or bt mode)
                                        if (self.wqdb.enabled || self.bt_mode)
                                            && dbg_chunk.is_none()
                                        {
                                            let file_id =
                                                self.debug_info.chunk(self.current_chunk).file_id;
                                            let id = self.debug_info.new_chunk(
                                                "<fn>",
                                                file_id,
                                                instructions.len(),
                                            );
                                            {
                                                let table =
                                                    &mut self.debug_info.chunk_mut(id).line_table;
                                                if let Some(spans) = dbg_stmt_spans.as_ref() {
                                                    apply_stmt_spans_exact_offs(
                                                        table,
                                                        instructions,
                                                        file_id,
                                                        spans,
                                                        base_offs,
                                                    );
                                                } else {
                                                    mark_stmt_heuristic(table, instructions);
                                                }
                                            }
                                            // Record local names if available
                                            if let Some(names) = dbg_local_names.as_ref() {
                                                self.debug_info.chunk_mut(id).local_names =
                                                    Some(names.clone());
                                            } else if let Some(ps) = params.as_ref() {
                                                self.debug_info.chunk_mut(id).local_names =
                                                    Some(ps.clone());
                                            }
                                            *dbg_chunk = Some(id);
                                        }
                                        found = Some(LocalCallable::Func {
                                            params: params.clone(),
                                            locals: *locals,
                                            instructions: instructions.clone(),
                                            captured: Vec::new(),
                                            dbg_chunk: *dbg_chunk,
                                            name_hint: None,
                                        });
                                    }
                                    Value::Closure {
                                        params,
                                        locals,
                                        captured,
                                        instructions,
                                        dbg_chunk,
                                        dbg_stmt_spans,
                                        dbg_local_names,
                                    } => {
                                        if (self.wqdb.enabled || self.bt_mode)
                                            && dbg_chunk.is_none()
                                        {
                                            let file_id =
                                                self.debug_info.chunk(self.current_chunk).file_id;
                                            let id = self.debug_info.new_chunk(
                                                "<fn>",
                                                file_id,
                                                instructions.len(),
                                            );
                                            {
                                                let table =
                                                    &mut self.debug_info.chunk_mut(id).line_table;
                                                if let Some(spans) = dbg_stmt_spans.as_ref() {
                                                    apply_stmt_spans_exact_offs(
                                                        table,
                                                        instructions,
                                                        file_id,
                                                        spans,
                                                        base_offs,
                                                    );
                                                } else {
                                                    mark_stmt_heuristic(table, instructions);
                                                }
                                            }
                                            if let Some(names) = dbg_local_names.as_ref() {
                                                self.debug_info.chunk_mut(id).local_names =
                                                    Some(names.clone());
                                            } else if let Some(ps) = params.as_ref() {
                                                self.debug_info.chunk_mut(id).local_names =
                                                    Some(ps.clone());
                                            }
                                            *dbg_chunk = Some(id);
                                        }
                                        found = Some(LocalCallable::Func {
                                            params: params.clone(),
                                            locals: *locals,
                                            instructions: instructions.clone(),
                                            captured: captured.clone(),
                                            dbg_chunk: *dbg_chunk,
                                            name_hint: None,
                                        });
                                    }
                                    Value::Function { params, body } => {
                                        let mut c = Compiler::new();
                                        c.compile(body)?;
                                        c.instructions.push(Instruction::Return);
                                        let locals_cnt = c.local_count();
                                        let instrs = std::mem::take(&mut c.instructions);
                                        let compiled_params = params.clone();
                                        // Register a chunk for this newly compiled function
                                        let file_id =
                                            self.debug_info.chunk(self.current_chunk).file_id;
                                        let id = self.debug_info.new_chunk(
                                            "<fn>",
                                            file_id,
                                            instrs.len(),
                                        );
                                        {
                                            let table =
                                                &mut self.debug_info.chunk_mut(id).line_table;
                                            mark_stmt_heuristic(table, &instrs);
                                        }
                                        *v = Value::CompiledFunction {
                                            params: compiled_params.clone(),
                                            locals: locals_cnt,
                                            instructions: instrs.clone(),
                                            dbg_chunk: Some(id),
                                            dbg_stmt_spans: None,
                                            dbg_local_names: None,
                                        };
                                        found = Some(LocalCallable::Func {
                                            params: compiled_params,
                                            locals: locals_cnt,
                                            instructions: instrs,
                                            captured: Vec::new(),
                                            dbg_chunk: Some(id),
                                            name_hint: None,
                                        });
                                    }
                                    Value::BuiltinFunction(name) => {
                                        found = Some(LocalCallable::Builtin(name.clone()));
                                    }
                                    other => {
                                        return Err(WqError::VmError(format!(
                                            "cannot call local slot {slot}: expected function, found {}",
                                            other.type_name(),
                                        )));
                                    }
                                }
                                break;
                            }
                        }
                        found
                    };

                    let callable = callable
                        .ok_or_else(|| WqError::VmError(format!("invalid local slot {slot}")))?;

                    match callable {
                        LocalCallable::Func {
                            params,
                            locals,
                            instructions,
                            captured,
                            dbg_chunk,
                            name_hint,
                        } => {
                            let res = self.call_function(
                                instructions,
                                params,
                                locals,
                                captured,
                                args,
                                name_hint.as_deref(),
                                dbg_chunk,
                            )?;
                            self.stack.push(res);
                        }
                        LocalCallable::Builtin(name) => {
                            let result = self.builtins.call(&name, &args)?;
                            self.stack.push(result);
                        }
                    }
                }
                Instruction::CallUser(name, argc) => {
                    let name_owned = name.clone();
                    let argc_val = *argc;
                    if self.stack.len() < argc_val {
                        return Err(WqError::VmError(format!(
                            "stack underflow: expected {argc_val} args for fn '{name_owned}'",
                        )));
                    }
                    let base = self.stack.len() - argc_val;
                    let args = self.stack.split_off(base);

                    let (cver, cval) = {
                        let c = &self.inline_cache[idx];
                        (c.version, c.value.clone())
                    };
                    let func_val = if cver == self.global_version {
                        cval
                    } else {
                        None
                    };
                    let func_val = if let Some(v) = func_val {
                        v
                    } else {
                        let v = self.lookup_global(&name_owned).ok_or_else(|| {
                            WqError::ValueError(format!("fn `{name_owned}` is not defined"))
                        })?;
                        {
                            let c = &mut self.inline_cache[idx];
                            c.version = self.global_version;
                            c.value = Some(v.clone());
                        }
                        v
                    };
                    match func_val {
                        Value::CompiledFunction {
                            params,
                            locals,
                            instructions,
                            dbg_chunk,
                            dbg_stmt_spans,
                            dbg_local_names,
                        } => {
                            let mut chunk_for_call = dbg_chunk;
                            if chunk_for_call.is_none() {
                                // Try to reuse a pre-registered chunk by function name (from compilation phase)
                                if let Some(&id) = self.debug_info.by_name.get(name_owned.as_str())
                                {
                                    chunk_for_call = Some(id);
                                } else if self.wqdb.enabled || self.bt_mode {
                                    let file_id = self.debug_info.chunk(self.current_chunk).file_id;
                                    let id = self.debug_info.new_chunk(
                                        name_owned.clone(),
                                        file_id,
                                        instructions.len(),
                                    );
                                    {
                                        let base_offs = self.debug_src_offset();
                                        let table = &mut self.debug_info.chunk_mut(id).line_table;
                                        if let Some(spans) = dbg_stmt_spans.as_ref() {
                                            apply_stmt_spans_exact_offs(
                                                table,
                                                &instructions,
                                                file_id,
                                                spans,
                                                base_offs,
                                            );
                                        } else {
                                            mark_stmt_heuristic(table, &instructions);
                                        }
                                    }
                                    if let Some(names) = dbg_local_names.as_ref() {
                                        self.debug_info.chunk_mut(id).local_names =
                                            Some(names.clone());
                                    } else if let Some(ps) = params.as_ref() {
                                        self.debug_info.chunk_mut(id).local_names =
                                            Some(ps.clone());
                                    }
                                    // Update globals and inline cache to persist chunk id for future calls
                                    if let Some(slot) = self.globals.get_mut(&name_owned) {
                                        *slot = Value::CompiledFunction {
                                            params: params.clone(),
                                            locals,
                                            instructions: instructions.clone(),
                                            dbg_chunk: Some(id),
                                            dbg_stmt_spans: dbg_stmt_spans.clone(),
                                            dbg_local_names: dbg_local_names.clone(),
                                        };
                                    }
                                    {
                                        let entry = &mut self.inline_cache[idx];
                                        entry.version = self.global_version;
                                        entry.value = Some(Value::CompiledFunction {
                                            params: params.clone(),
                                            locals,
                                            instructions: instructions.clone(),
                                            dbg_chunk: Some(id),
                                            dbg_stmt_spans: dbg_stmt_spans.clone(),
                                            dbg_local_names: dbg_local_names.clone(),
                                        });
                                    }
                                    // Also record by name for future reuse
                                    let name_arc: std::sync::Arc<str> =
                                        std::sync::Arc::from(name_owned.as_str());
                                    self.debug_info.by_name.insert(name_arc, id);
                                    chunk_for_call = Some(id);
                                }
                            }
                            let res = self.call_function(
                                instructions,
                                params,
                                locals,
                                Vec::new(),
                                args,
                                Some(&name_owned),
                                chunk_for_call,
                            )?;
                            self.stack.push(res);
                        }
                        Value::Closure {
                            params,
                            locals,
                            captured,
                            instructions,
                            dbg_chunk,
                            dbg_stmt_spans,
                            dbg_local_names,
                        } => {
                            let mut chunk_for_call = dbg_chunk;
                            if (self.wqdb.enabled || self.bt_mode) && chunk_for_call.is_none() {
                                let file_id = self.debug_info.chunk(self.current_chunk).file_id;
                                let id = self.debug_info.new_chunk(
                                    name_owned.clone(),
                                    file_id,
                                    instructions.len(),
                                );
                                {
                                    let base_offs = self.debug_src_offset();
                                    let table = &mut self.debug_info.chunk_mut(id).line_table;
                                    if let Some(spans) = dbg_stmt_spans.as_ref() {
                                        apply_stmt_spans_exact_offs(
                                            table,
                                            &instructions,
                                            file_id,
                                            spans,
                                            base_offs,
                                        );
                                    } else {
                                        mark_stmt_heuristic(table, &instructions);
                                    }
                                }
                                if let Some(names) = dbg_local_names.as_ref() {
                                    self.debug_info.chunk_mut(id).local_names = Some(names.clone());
                                } else if let Some(ps) = params.as_ref() {
                                    self.debug_info.chunk_mut(id).local_names = Some(ps.clone());
                                }
                                chunk_for_call = Some(id);
                            }
                            let res = self.call_function(
                                instructions,
                                params,
                                locals,
                                captured,
                                args,
                                Some(&name_owned),
                                chunk_for_call,
                            )?;
                            self.stack.push(res);
                        }
                        Value::Function { params, body } => {
                            let mut c = Compiler::new();
                            c.compile(&body)?;
                            c.instructions.push(Instruction::Return);
                            let locals = c.local_count();
                            let instrs = c.instructions.clone();
                            let mut id_for_dbg: Option<ChunkId> = None;
                            if self.wqdb.enabled || self.bt_mode {
                                // Register a chunk for the newly compiled function
                                let file_id = self.debug_info.chunk(self.current_chunk).file_id;
                                let id = self.debug_info.new_chunk(
                                    name_owned.clone(),
                                    file_id,
                                    instrs.len(),
                                );
                                {
                                    let table = &mut self.debug_info.chunk_mut(id).line_table;
                                    mark_stmt_heuristic(table, &instrs);
                                }
                                // Replace in globals to avoid recompilation on next lookup
                                if let Some(slot) = self.globals.get_mut(&name_owned) {
                                    *slot = Value::CompiledFunction {
                                        params: params.clone(),
                                        locals,
                                        instructions: instrs.clone(),
                                        dbg_chunk: Some(id),
                                        dbg_stmt_spans: None,
                                        dbg_local_names: None,
                                    };
                                }
                                {
                                    let entry = &mut self.inline_cache[idx];
                                    entry.version = self.global_version;
                                    entry.value = Some(Value::CompiledFunction {
                                        params: params.clone(),
                                        locals,
                                        instructions: instrs.clone(),
                                        dbg_chunk: Some(id),
                                        dbg_stmt_spans: None,
                                        dbg_local_names: None,
                                    });
                                }
                                id_for_dbg = Some(id);
                            }
                            let res = self.call_function(
                                instrs,
                                params,
                                locals,
                                Vec::new(),
                                args,
                                Some(&name_owned),
                                id_for_dbg,
                            )?;
                            self.stack.push(res);
                        }
                        Value::BuiltinFunction(name) => {
                            let result = self.builtins.call(&name, &args)?;
                            self.stack.push(result);
                        }
                        other => {
                            return Err(WqError::VmError(format!(
                                "cannot call '{name_owned}': expected fn, found {}",
                                other.type_name(),
                            )));
                        }
                    }
                }
                Instruction::CallAnon(argc) => {
                    if self.stack.len() < *argc + 1 {
                        return Err(WqError::VmError(format!(
                            "stack underflow: expected {argc} args and a function",
                        )));
                    }
                    let base = self.stack.len() - *argc;
                    let args = self.stack.split_off(base);
                    let func_val = self.stack.pop().ok_or_else(|| {
                        WqError::VmError("stack underflow: missing function value for call".into())
                    })?;

                    match func_val {
                        Value::CompiledFunction {
                            params,
                            locals,
                            instructions,
                            dbg_chunk,
                            dbg_stmt_spans,
                            dbg_local_names,
                        } => {
                            let mut chunk_for_call = dbg_chunk;
                            if (self.wqdb.enabled || self.bt_mode) && chunk_for_call.is_none() {
                                let file_id = self.debug_info.chunk(self.current_chunk).file_id;
                                let id = self.debug_info.new_chunk(
                                    "<anon>".to_string(),
                                    file_id,
                                    instructions.len(),
                                );
                                {
                                    let base_offs = self.debug_src_offset();
                                    let table = &mut self.debug_info.chunk_mut(id).line_table;
                                    if let Some(spans) = dbg_stmt_spans.as_ref() {
                                        apply_stmt_spans_exact_offs(
                                            table,
                                            &instructions,
                                            file_id,
                                            spans,
                                            base_offs,
                                        );
                                    } else {
                                        mark_stmt_heuristic(table, &instructions);
                                    }
                                }
                                if let Some(names) = dbg_local_names.as_ref() {
                                    self.debug_info.chunk_mut(id).local_names = Some(names.clone());
                                } else if let Some(ps) = params.as_ref() {
                                    self.debug_info.chunk_mut(id).local_names = Some(ps.clone());
                                }
                                chunk_for_call = Some(id);
                            }
                            let res = self.call_function(
                                instructions,
                                params,
                                locals,
                                Vec::new(),
                                args,
                                None,
                                chunk_for_call,
                            )?;
                            self.stack.push(res);
                        }
                        Value::Closure {
                            params,
                            locals,
                            captured,
                            instructions,
                            dbg_chunk,
                            dbg_stmt_spans,
                            dbg_local_names,
                        } => {
                            let mut chunk_for_call = dbg_chunk;
                            if (self.wqdb.enabled || self.bt_mode) && chunk_for_call.is_none() {
                                let file_id = self.debug_info.chunk(self.current_chunk).file_id;
                                let id = self.debug_info.new_chunk(
                                    "<anon>".to_string(),
                                    file_id,
                                    instructions.len(),
                                );
                                {
                                    let base_offs = self.debug_src_offset();
                                    let table = &mut self.debug_info.chunk_mut(id).line_table;
                                    if let Some(spans) = dbg_stmt_spans.as_ref() {
                                        apply_stmt_spans_exact_offs(
                                            table,
                                            &instructions,
                                            file_id,
                                            spans,
                                            base_offs,
                                        );
                                    } else {
                                        mark_stmt_heuristic(table, &instructions);
                                    }
                                }
                                if let Some(names) = dbg_local_names.as_ref() {
                                    self.debug_info.chunk_mut(id).local_names = Some(names.clone());
                                } else if let Some(ps) = params.as_ref() {
                                    self.debug_info.chunk_mut(id).local_names = Some(ps.clone());
                                }
                                chunk_for_call = Some(id);
                            }
                            let res = self.call_function(
                                instructions,
                                params,
                                locals,
                                captured,
                                args,
                                None,
                                chunk_for_call,
                            )?;
                            self.stack.push(res);
                        }
                        Value::Function { params, body } => {
                            let mut c = Compiler::new();
                            c.compile(&body)?;
                            c.instructions.push(Instruction::Return);
                            let locals = c.local_count();
                            let instrs = c.instructions;
                            let mut id_for_dbg: Option<ChunkId> = None;
                            if self.wqdb.enabled || self.bt_mode {
                                let file_id = self.debug_info.chunk(self.current_chunk).file_id;
                                let id = self.debug_info.new_chunk("<anon>", file_id, instrs.len());
                                {
                                    let table = &mut self.debug_info.chunk_mut(id).line_table;
                                    mark_stmt_heuristic(table, &instrs);
                                }
                                id_for_dbg = Some(id);
                            }
                            let res = self.call_function(
                                instrs,
                                params,
                                locals,
                                Vec::new(),
                                args,
                                Some("<anon>"),
                                id_for_dbg,
                            )?;
                            self.stack.push(res);
                        }
                        Value::BuiltinFunction(name) => {
                            let result = self.builtins.call(&name, &args)?;
                            self.stack.push(result);
                        }
                        other => {
                            return Err(WqError::VmError(format!(
                                "cannot call value of type {:?} as a function",
                                other.type_name(),
                            )));
                        }
                    }
                }
                Instruction::MakeList(n) => {
                    if self.stack.len() < *n {
                        return Err(WqError::VmError(format!(
                            "stack underflow: expected {n} list items",
                        )));
                    }
                    let base = self.stack.len() - *n;
                    let items = self.stack.split_off(base);
                    let all_ints = items.iter().all(|v| matches!(v, Value::Int(_)));
                    if all_ints {
                        let ints: Vec<i64> = items
                            .into_iter()
                            .map(|v| match v {
                                Value::Int(i) => i,
                                _ => unreachable!(),
                            })
                            .collect();
                        self.stack.push(Value::IntList(ints));
                    } else {
                        self.stack.push(Value::List(items));
                    }
                }
                Instruction::MakeDict(n) => {
                    let mut pairs = Vec::with_capacity(*n);
                    for _ in 0..*n {
                        let val = self.stack.pop().ok_or_else(|| {
                            WqError::VmError("stack underflow: expected value for dict".into())
                        })?;
                        let key = match self.stack.pop().ok_or_else(|| {
                            WqError::VmError("stack underflow: expected key for dict".into())
                        })? {
                            Value::Symbol(k) => k,
                            other => {
                                return Err(WqError::VmError(format!(
                                    "invalid dict key: expected symbol, got {other:?}"
                                )));
                            }
                        };
                        pairs.push((key, val));
                    }
                    let mut map = IndexMap::with_capacity(*n);
                    while let Some((k, v)) = pairs.pop() {
                        // reverse the pop order
                        map.insert(k, v);
                    }
                    self.stack.push(Value::Dict(map));
                }
                Instruction::Index => {
                    // Hot path: IntList/List with integer index
                    match (self.stack.pop().unwrap(), self.stack.pop().unwrap()) {
                        (Value::Int(i), Value::IntList(items)) => {
                            let len = items.len() as i64;
                            let ii = if i < 0 { len + i } else { i };
                            if ii >= 0 && ii < len {
                                let idx = ii as usize;
                                self.stack.push(Value::Int(items[idx]));
                            } else {
                                return Err(WqError::IndexError(format!(
                                    "invalid index: attempted to access index {i} in intlist of len {}",
                                    items.len()
                                )));
                            }
                        }
                        (Value::Int(i), Value::List(items)) => {
                            let len = items.len() as i64;
                            let ii = if i < 0 { len + i } else { i };
                            if ii >= 0 && ii < len {
                                let idx = ii as usize;
                                let v = items.get(idx).cloned().unwrap();
                                self.stack.push(v);
                            } else {
                                return Err(WqError::IndexError(format!(
                                    "invalid index: attempted to access index {i} in list of len {}",
                                    items.len()
                                )));
                            }
                        }
                        (idx, obj) => match obj.index(&idx) {
                            Some(v) => self.stack.push(v),
                            None => {
                                return Err(WqError::IndexError(format!(
                                    "invalid index: attempted to access index {idx} in `{obj}`"
                                )));
                            }
                        },
                    }
                }
                Instruction::IndexAssign => {
                    let val = self.stack.pop().ok_or_else(|| {
                        WqError::VmError(
                            "stack underflow: missing value for index assignment".into(),
                        )
                    })?;

                    let idx = self.stack.pop().ok_or_else(|| {
                        WqError::VmError("stack underflow: missing index for assignment".into())
                    })?;

                    let obj_name = self.stack.pop().ok_or_else(|| {
                        WqError::VmError(
                            "stack underflow: missing target object name for index assignment"
                                .into(),
                        )
                    })?;

                    match obj_name {
                        Value::Symbol(name) => match self.globals.get_mut(&name) {
                            Some(obj) => {
                                // Fast path: intlist/list with int index
                                match (&mut *obj, &idx, &val) {
                                    (Value::IntList(items), Value::Int(i), Value::Int(v)) => {
                                        let len = items.len() as i64;
                                        let ii = if *i < 0 { len + *i } else { *i };
                                        if ii >= 0 && ii < len {
                                            items[ii as usize] = *v;
                                            self.stack.push(val);
                                        } else {
                                            return Err(WqError::IndexError(format!(
                                                "failed to assign to {name}[{i}], out of bounds",
                                            )));
                                        }
                                    }
                                    (Value::List(items), Value::Int(i), _) => {
                                        let len = items.len() as i64;
                                        let ii = if *i < 0 { len + *i } else { *i };
                                        if ii >= 0 && ii < len {
                                            items[ii as usize] = val.clone();
                                            self.stack.push(val);
                                        } else {
                                            return Err(WqError::IndexError(format!(
                                                "failed to assign to {name}[{i}], out of bounds",
                                            )));
                                        }
                                    }
                                    _ => {
                                        if (*obj).set_index(&idx, val.clone()).is_some() {
                                            self.stack.push(val);
                                        } else {
                                            return Err(WqError::IndexError(format!(
                                                "failed to assign to {name}[{idx}], index invalid or not supported",
                                            )));
                                        }
                                    }
                                }
                            }
                            None => {
                                return Err(WqError::ValueError(format!(
                                    "cannot assign to {name}[{idx}], variable not found",
                                )));
                            }
                        },
                        other => {
                            return Err(WqError::VmError(format!(
                                "invalid index assignment target: expected symbol, got {}",
                                other.type_name(),
                            )));
                        }
                    }
                }
                Instruction::IndexAssignDrop => {
                    let val = self.stack.pop().ok_or_else(|| {
                        WqError::VmError(
                            "stack underflow: missing value for index assignment".into(),
                        )
                    })?;

                    let idx = self.stack.pop().ok_or_else(|| {
                        WqError::VmError("stack underflow: missing index for assignment".into())
                    })?;

                    let obj_name = self.stack.pop().ok_or_else(|| {
                        WqError::VmError(
                            "stack underflow: missing target object name for index assignment"
                                .into(),
                        )
                    })?;

                    match obj_name {
                        Value::Symbol(name) => match self.globals.get_mut(&name) {
                            Some(obj) => match (&mut *obj, &idx, &val) {
                                (Value::IntList(items), Value::Int(i), Value::Int(v)) => {
                                    let len = items.len() as i64;
                                    let ii = if *i < 0 { len + *i } else { *i };
                                    if ii >= 0 && ii < len {
                                        items[ii as usize] = *v;
                                    } else {
                                        return Err(WqError::IndexError(format!(
                                            "failed to assign to {name}[{i}], out of bounds",
                                        )));
                                    }
                                }
                                (Value::List(items), Value::Int(i), _) => {
                                    let len = items.len() as i64;
                                    let ii = if *i < 0 { len + *i } else { *i };
                                    if ii >= 0 && ii < len {
                                        items[ii as usize] = val.clone();
                                    } else {
                                        return Err(WqError::IndexError(format!(
                                            "failed to assign to {name}[{i}], out of bounds",
                                        )));
                                    }
                                }
                                _ => {
                                    if (*obj).set_index(&idx, val.clone()).is_none() {
                                        return Err(WqError::IndexError(format!(
                                            "failed to assign to {name}[{idx}], index invalid or not supported",
                                        )));
                                    }
                                }
                            },
                            None => {
                                return Err(WqError::ValueError(format!(
                                    "cannot assign to {name}[{idx}], variable not found",
                                )));
                            }
                        },
                        other => {
                            return Err(WqError::DomainError(format!(
                                "invalid index assignment target: expected symbol, got {}",
                                other.type_name(),
                            )));
                        }
                    }
                }
                Instruction::IndexAssignLocal(slot) => {
                    let val = self.stack.pop().ok_or_else(|| {
                        WqError::VmError(
                            "stack underflow: missing value for index assignment".into(),
                        )
                    })?;
                    let idx = self.stack.pop().ok_or_else(|| {
                        WqError::VmError("stack underflow: missing index for assignment".into())
                    })?;
                    if let Some(frame) = self.locals.last_mut() {
                        if let Some(obj) = frame.get_mut(*slot as usize) {
                            match (&mut *obj, &idx, &val) {
                                (Value::IntList(items), Value::Int(i), Value::Int(v)) => {
                                    let len = items.len() as i64;
                                    let ii = if *i < 0 { len + *i } else { *i };
                                    if ii >= 0 && ii < len {
                                        items[ii as usize] = *v;
                                        self.stack.push(val);
                                    } else {
                                        return Err(WqError::IndexError(format!(
                                            "failed to assign to local[{slot}][{i}], out of bounds",
                                        )));
                                    }
                                }
                                (Value::List(items), Value::Int(i), _) => {
                                    let len = items.len() as i64;
                                    let ii = if *i < 0 { len + *i } else { *i };
                                    if ii >= 0 && ii < len {
                                        items[ii as usize] = val.clone();
                                        self.stack.push(val);
                                    } else {
                                        return Err(WqError::IndexError(format!(
                                            "failed to assign to local[{slot}][{i}], out of bounds",
                                        )));
                                    }
                                }
                                _ => {
                                    if (*obj).set_index(&idx, val.clone()).is_some() {
                                        self.stack.push(val);
                                    } else {
                                        return Err(WqError::IndexError(format!(
                                            "failed to assign to local[{slot}][{idx}], index invalid or not supported",
                                        )));
                                    }
                                }
                            }
                        } else {
                            return Err(WqError::VmError(format!("invalid local slot {slot}")));
                        }
                    } else {
                        return Err(WqError::VmError("no local frame".into()));
                    }
                }
                Instruction::IndexAssignLocalDrop(slot) => {
                    let val = self.stack.pop().ok_or_else(|| {
                        WqError::VmError(
                            "stack underflow: missing value for index assignment".into(),
                        )
                    })?;
                    let idx = self.stack.pop().ok_or_else(|| {
                        WqError::VmError("stack underflow: missing index for assignment".into())
                    })?;
                    if let Some(frame) = self.locals.last_mut() {
                        if let Some(obj) = frame.get_mut(*slot as usize) {
                            match (&mut *obj, &idx, &val) {
                                (Value::IntList(items), Value::Int(i), Value::Int(v)) => {
                                    let len = items.len() as i64;
                                    let ii = if *i < 0 { len + *i } else { *i };
                                    if ii >= 0 && ii < len {
                                        items[ii as usize] = *v;
                                    } else {
                                        return Err(WqError::IndexError(format!(
                                            "failed to assign to local[{slot}][{i}], out of bounds",
                                        )));
                                    }
                                }
                                (Value::List(items), Value::Int(i), _) => {
                                    let len = items.len() as i64;
                                    let ii = if *i < 0 { len + *i } else { *i };
                                    if ii >= 0 && ii < len {
                                        items[ii as usize] = val.clone();
                                    } else {
                                        return Err(WqError::IndexError(format!(
                                            "failed to assign to local[{slot}][{i}], out of bounds",
                                        )));
                                    }
                                }
                                _ => {
                                    if (*obj).set_index(&idx, val.clone()).is_none() {
                                        return Err(WqError::IndexError(format!(
                                            "failed to assign to local[{slot}][{idx}], index invalid or not supported",
                                        )));
                                    }
                                }
                            }
                        } else {
                            return Err(WqError::VmError(format!("invalid local slot {slot}")));
                        }
                    } else {
                        return Err(WqError::VmError("no local frame".into()));
                    }
                }
                Instruction::Jump(pos) => self.pc = *pos,
                Instruction::JumpIfFalse(pos) => {
                    let v = self.stack.pop().ok_or_else(|| {
                        WqError::VmError(
                            "stack underflow: missing value for conditional jump".into(),
                        )
                    })?;

                    let is_false = match v {
                        Value::Bool(b) => !b,
                        // only bool is accepted
                        // Value::Int(n) => n == 0,
                        // Value::Float(f) => f == 0.0,
                        _ => {
                            return Err(WqError::DomainError(format!(
                                "control flow: invalid condition type, expected bool, got {}",
                                v.type_name()
                            )));
                        }
                    };

                    if is_false {
                        self.pc = *pos;
                    }
                }
                Instruction::JumpIfGE(pos) => {
                    // Pop right then left, jump if left >= right
                    let right = self.stack.pop().ok_or_else(|| {
                        WqError::VmError(
                            "stack underflow: missing right operand for compare-jump".into(),
                        )
                    })?;
                    let left = self.stack.pop().ok_or_else(|| {
                        WqError::VmError(
                            "stack underflow: missing left operand for compare-jump".into(),
                        )
                    })?;
                    // emulate !(left < right)
                    let lt = left.lt(&right);
                    let cond = match lt {
                        Ok(Value::Bool(b)) => !b,
                        _ => {
                            return Err(WqError::VmError(
                                "invalid condition type in control flow, expected bool, int, or float".into(),
                            ));
                        }
                    };
                    if cond {
                        self.pc = *pos;
                    }
                }
                Instruction::JumpIfLEZLocal(slot, pos) => {
                    // Jump if local[slot] <= 0
                    let v = self
                        .locals
                        .last()
                        .and_then(|f| f.get(*slot as usize))
                        .cloned()
                        .ok_or_else(|| WqError::VmError(format!("invalid local slot {slot}")))?;
                    let is_le_zero = match v {
                        Value::Int(n) => n <= 0,
                        Value::Float(f) => f <= 0.0,
                        _ => {
                            return Err(WqError::DomainError(
                                "invalid condition type in control flow, expected bool, int, or float".into(),
                            ));
                        }
                    };
                    if is_le_zero {
                        self.pc = *pos;
                    }
                }
                Instruction::Pop => {
                    self.stack.pop();
                }
                Instruction::Assert => {
                    let v = self.stack.pop().ok_or_else(|| {
                        WqError::VmError("stack underflow: missing value for assert".into())
                    })?;
                    let ok = match v {
                        Value::Bool(b) => b,
                        _ => {
                            return Err(WqError::DomainError("`@a`: expected bool".into()));
                        }
                    };
                    if !ok {
                        return Err(WqError::AssertionError("assertion failed".into()));
                    }
                    self.stack.push(Value::unit());
                }
                Instruction::Return => break,
                Instruction::Try(len) => {
                    let start_pc = self.pc;
                    let end_pc = start_pc + len;
                    let stack_start = self.stack.len();
                    match self.execute_until(end_pc) {
                        Ok(v) => {
                            self.stack.truncate(stack_start);
                            self.stack.push(Value::List(vec![v, Value::Int(0)]));
                        }
                        Err(e) => {
                            self.stack.truncate(stack_start);
                            // Push the formatted error string (label(code): message)
                            let msg: Vec<Value> = e.to_string().chars().map(Value::Char).collect();
                            self.stack.push(Value::List(vec![
                                Value::List(msg),
                                Value::Int(e.code() as i64),
                            ]));
                        }
                    }
                    self.pc = end_pc;
                }
                Instruction::CallOrIndex(argc) => {
                    if self.stack.len() < *argc + 1 {
                        return Err(WqError::VmError(format!(
                            "stack underflow: expected {argc} args and an object",
                        )));
                    }
                    let base = self.stack.len() - *argc;
                    let mut args = self.stack.split_off(base);
                    let obj = self.stack.pop().unwrap();

                    match obj {
                        Value::CompiledFunction {
                            params,
                            locals,
                            instructions,
                            dbg_chunk,
                            dbg_stmt_spans,
                            dbg_local_names,
                        } => {
                            let mut chunk_for_call = dbg_chunk;
                            if (self.wqdb.enabled || self.bt_mode) && chunk_for_call.is_none() {
                                let file_id = self.debug_info.chunk(self.current_chunk).file_id;
                                let id = self.debug_info.new_chunk(
                                    "<fn>".to_string(),
                                    file_id,
                                    instructions.len(),
                                );
                                {
                                    let base_offs = self.debug_src_offset();
                                    let table = &mut self.debug_info.chunk_mut(id).line_table;
                                    if let Some(spans) = dbg_stmt_spans.as_ref() {
                                        apply_stmt_spans_exact_offs(
                                            table,
                                            &instructions,
                                            file_id,
                                            spans,
                                            base_offs,
                                        );
                                    } else {
                                        mark_stmt_heuristic(table, &instructions);
                                    }
                                }
                                if let Some(names) = dbg_local_names.as_ref() {
                                    self.debug_info.chunk_mut(id).local_names = Some(names.clone());
                                } else if let Some(ps) = params.as_ref() {
                                    self.debug_info.chunk_mut(id).local_names = Some(ps.clone());
                                }
                                chunk_for_call = Some(id);
                            }
                            let res = self.call_function(
                                instructions,
                                params,
                                locals,
                                Vec::new(),
                                args,
                                None,
                                chunk_for_call,
                            )?;
                            self.stack.push(res);
                        }
                        Value::Closure {
                            params,
                            locals,
                            captured,
                            instructions,
                            dbg_chunk,
                            dbg_stmt_spans,
                            dbg_local_names,
                        } => {
                            let mut chunk_for_call = dbg_chunk;
                            if (self.wqdb.enabled || self.bt_mode) && chunk_for_call.is_none() {
                                let file_id = self.debug_info.chunk(self.current_chunk).file_id;
                                let id = self.debug_info.new_chunk(
                                    "<fn>".to_string(),
                                    file_id,
                                    instructions.len(),
                                );
                                {
                                    let base_offs = self.debug_src_offset();
                                    let table = &mut self.debug_info.chunk_mut(id).line_table;
                                    mark_stmt_heuristic(table, &instructions);
                                    if let Some(spans) = dbg_stmt_spans.as_ref() {
                                        let meta_len = instructions.len();
                                        let mut pcs = Vec::new();
                                        for pc in 0..meta_len {
                                            if table.is_stmt(pc) {
                                                pcs.push(pc);
                                            }
                                        }
                                        let n = pcs.len().min(spans.len());
                                        for i in 0..n {
                                            table.mark_stmt(
                                                pcs[i],
                                                crate::wqdb::Span {
                                                    file_id,
                                                    start: (spans[i].0 + base_offs) as u32,
                                                    end: (spans[i].1 + base_offs) as u32,
                                                },
                                            );
                                        }
                                    }
                                }
                                if let Some(names) = dbg_local_names.as_ref() {
                                    self.debug_info.chunk_mut(id).local_names = Some(names.clone());
                                } else if let Some(ps) = params.as_ref() {
                                    self.debug_info.chunk_mut(id).local_names = Some(ps.clone());
                                }
                                chunk_for_call = Some(id);
                            }
                            let res = self.call_function(
                                instructions,
                                params,
                                locals,
                                captured,
                                args,
                                None,
                                chunk_for_call,
                            )?;
                            self.stack.push(res);
                        }
                        Value::Function { params, body } => {
                            let mut c = Compiler::new();
                            c.compile(&body)?;
                            c.instructions.push(Instruction::Return);
                            let locals = c.local_count();
                            let instrs = c.instructions;
                            let mut id_opt: Option<ChunkId> = None;
                            if self.wqdb.enabled || self.bt_mode {
                                let file_id = self.debug_info.chunk(self.current_chunk).file_id;
                                let id = self.debug_info.new_chunk("<anon>", file_id, instrs.len());
                                {
                                    let table = &mut self.debug_info.chunk_mut(id).line_table;
                                    mark_stmt_heuristic(table, &instrs);
                                }
                                id_opt = Some(id);
                            }
                            let res = self.call_function(
                                instrs,
                                params,
                                locals,
                                Vec::new(),
                                args,
                                Some("<anon>"),
                                id_opt,
                            )?;
                            self.stack.push(res);
                        }
                        Value::BuiltinFunction(name) => {
                            let result = self.builtins.call(&name, &args)?;
                            self.stack.push(result);
                        }
                        other => {
                            // treat as index
                            let idx_val = if args.len() == 1 {
                                args.pop().unwrap()
                            } else {
                                let all_ints = args.iter().all(|v| matches!(v, Value::Int(_)));
                                if all_ints {
                                    let ints: Vec<i64> = args
                                        .into_iter()
                                        .map(|v| match v {
                                            Value::Int(i) => i,
                                            _ => unreachable!(),
                                        })
                                        .collect();
                                    Value::IntList(ints)
                                } else {
                                    Value::List(args)
                                }
                            };
                            match (idx_val, other) {
                                (Value::Int(i), Value::IntList(items)) => {
                                    let len = items.len() as i64;
                                    let ii = if i < 0 { len + i } else { i };
                                    if ii >= 0 && ii < len {
                                        let idx = ii as usize;
                                        self.stack.push(Value::Int(items[idx]));
                                    } else {
                                        return Err(WqError::IndexError(format!(
                                            "invalid index: attempted to access index {i} in intlist of len {}",
                                            items.len()
                                        )));
                                    }
                                }
                                (Value::Int(i), Value::List(items)) => {
                                    let len = items.len() as i64;
                                    let ii = if i < 0 { len + i } else { i };
                                    if ii >= 0 && ii < len {
                                        let idx = ii as usize;
                                        let v = items.get(idx).cloned().unwrap();
                                        self.stack.push(v);
                                    } else {
                                        return Err(WqError::IndexError(format!(
                                            "invalid index: attempted to access index {i} in list of len {}",
                                            items.len()
                                        )));
                                    }
                                }
                                (idx, objv) => match objv.index(&idx) {
                                    Some(v) => self.stack.push(v),
                                    None => {
                                        return Err(WqError::IndexError(format!(
                                            "invalid index: attempted to access index {idx} in `{objv}`"
                                        )));
                                    }
                                },
                            }
                        }
                    }
                }
                Instruction::LoadCapture(i) => {
                    let cap = self
                        .captures
                        .last()
                        .and_then(|c| c.get(*i as usize))
                        .ok_or_else(|| WqError::VmError(format!("invalid capture slot {i}")))?;
                    self.stack.push(cap.clone());
                }
                Instruction::LoadClosure {
                    params,
                    locals,
                    captures,
                    instructions,
                    dbg_stmt_spans,
                    dbg_local_names,
                } => {
                    let mut captured_vals = Vec::with_capacity(captures.len());
                    for cap in captures {
                        match cap {
                            Capture::Local(slot) => {
                                if let Some(parent) = self.locals.last() {
                                    captured_vals.push(
                                        parent
                                            .get(*slot as usize)
                                            .cloned()
                                            .unwrap_or(Value::unit()),
                                    );
                                } else {
                                    captured_vals.push(Value::unit());
                                }
                            }
                            Capture::FromCapture(i) => {
                                let val = self
                                    .captures
                                    .last()
                                    .and_then(|c| c.get(*i as usize).cloned())
                                    .unwrap_or(Value::unit());
                                captured_vals.push(val);
                            }
                            Capture::Global(name) => {
                                let val = if let Some(v) = self.lookup_global(name) {
                                    v
                                }
                                // do not capture builtins for now
                                // else if self.builtins.get_id(name).is_some() {
                                //     Value::BuiltinFunction(name.clone())
                                // }
                                else {
                                    return Err(WqError::ValueError(format!(
                                        "`{name}` is not defined"
                                    )));
                                };
                                captured_vals.push(val);
                            }
                        }
                    }
                    // Register a debug chunk for this closure's code (wqdb or bt mode)
                    let mut chunk_opt: Option<ChunkId> = None;
                    if self.wqdb.enabled || self.bt_mode {
                        let file_id = self.debug_info.chunk(self.current_chunk).file_id;
                        let id = self
                            .debug_info
                            .new_chunk("<fn>", file_id, instructions.len());
                        {
                            let base_offs = self.debug_src_offset();
                            let table = &mut self.debug_info.chunk_mut(id).line_table;
                            // Prefer exact span mapping when available, shifted by the
                            // current script's base byte offset so backtraces point to
                            // the correct location in the full source.
                            if !dbg_stmt_spans.is_empty() {
                                apply_stmt_spans_exact_offs(
                                    table,
                                    instructions,
                                    file_id,
                                    dbg_stmt_spans,
                                    base_offs,
                                );
                            } else {
                                mark_stmt_heuristic(table, instructions);
                            }
                        }
                        if !dbg_local_names.is_empty() {
                            self.debug_info.chunk_mut(id).local_names =
                                Some(dbg_local_names.clone());
                        } else if let Some(ps) = params.as_ref() {
                            self.debug_info.chunk_mut(id).local_names = Some(ps.clone());
                        }
                        chunk_opt = Some(id);
                    }
                    self.stack.push(Value::Closure {
                        params: params.clone(),
                        locals: *locals,
                        captured: captured_vals,
                        instructions: instructions.clone(),
                        dbg_chunk: chunk_opt,
                        dbg_stmt_spans: Some(dbg_stmt_spans.clone()),
                        dbg_local_names: Some(dbg_local_names.clone()),
                    });
                }
            }
        }
        Ok(self.stack.pop().unwrap_or(Value::unit()))
    }
}

impl Vm {
    fn func_name_for_chunk(&self, id: ChunkId) -> String {
        self.debug_info
            .chunks
            .get(&id)
            .map(|m| m.name.to_string())
            .unwrap_or_else(|| "<unknown>".to_string())
    }
}

impl DebugHost for Vm {
    fn loc(&self) -> CodeLoc {
        CodeLoc {
            chunk: self.current_chunk,
            pc: self.pc,
        }
    }
    fn call_depth(&self) -> usize {
        self.call_stack.len()
    }
    fn di(&self) -> &DebugInfo {
        &self.debug_info
    }

    fn dbg_continue(&mut self) {
        self.wqdb.clear_mode();
    }
    fn dbg_step_in(&mut self) {
        if crate::repl::get_debug_level() >= 2 {
            eprintln!("[wqdb]: dbg_step_in called at PC {}", self.pc);
        }
        self.wqdb.req_in(self.call_depth());

        // For step-in, we rely purely on the step mode logic in should_pause_at
        // No temporary breakpoints needed - the step mode will pause at the next statement
        if crate::repl::get_debug_level() >= 2 {
            eprintln!("[wqdb]: step-in mode on, will pause at next statement");
        }
    }

    fn dbg_step_over(&mut self) {
        // Step over: pause at the next statement encountered in the
        // current or outer frames (do not step into deeper frames).
        // Heuristic: also place a temporary breakpoint at the first
        // statement inside a forward-branch loop body (e.g. W[...]) so
        // 'next' on a loop header does not jump past the entire loop.
        self.wqdb.req_over(self.call_depth());

        let here = CodeLoc {
            chunk: self.current_chunk,
            pc: self.pc,
        };
        let meta = self.debug_info.chunk(here.chunk);

        // If we're at a Return instruction, set up temp breaks in the caller
        if self.is_at_return() {
            if !self.call_stack.is_empty() {
                let caller_frame = &self.call_stack[self.call_stack.len() - 1];
                let caller_meta = self.debug_info.chunk(caller_frame.chunk);
                // Look for the next statement after the call site
                for pc in caller_frame.pc + 1..caller_meta.len {
                    if caller_meta.line_table.is_stmt(pc) {
                        self.wqdb.add_temp_break(CodeLoc {
                            chunk: caller_frame.chunk,
                            pc,
                        });
                        break;
                    }
                }
            }
            return;
        }

        // 1) Always add a forward-only temp break at the next stmt in this chunk
        //    to guarantee progress when we're at the last stmt of a function.
        for pc in here.pc + 1..meta.len {
            if meta.line_table.is_stmt(pc) {
                self.wqdb.add_temp_break(CodeLoc {
                    chunk: here.chunk,
                    pc,
                });
                break;
            }
        }

        // If we look like we're on a loop header (cond -> exit),
        // also pause at the first stmt *inside* the body.
        let code = &self.instructions;

        // Find a nearby conditional jump with a forward target (typical loop header)
        let mut cond_pc_and_exit: Option<(usize, usize)> = None;
        for k in (here.pc.saturating_sub(16))..((here.pc + 32).min(code.len().saturating_sub(1))) {
            use crate::vm::instruction::Instruction::*;
            let hit = match code.get(k) {
                Some(JumpIfFalse(t)) if *t > k + 1 => Some((k, *t)),
                Some(JumpIfGE(t)) if *t > k + 1 => Some((k, *t)),
                Some(JumpIfLEZLocal(_, t)) if *t > k + 1 => Some((k, *t)),
                _ => None,
            };
            if let Some(pair) = hit {
                cond_pc_and_exit = Some(pair);
                break;
            }
        }

        // If we're at a Return instruction, set up temp breaks in the caller
        // and clear step mode so we can continue properly
        if self.is_at_return() {
            if !self.call_stack.is_empty() {
                let caller_frame = &self.call_stack[self.call_stack.len() - 1];
                let caller_meta = self.debug_info.chunk(caller_frame.chunk);
                // Look for the next statement after the call site
                for pc in caller_frame.pc..caller_meta.len {
                    if caller_meta.line_table.is_stmt(pc) {
                        self.wqdb.add_temp_break(CodeLoc {
                            chunk: caller_frame.chunk,
                            pc,
                        });
                        break;
                    }
                }
            }
            // Clear step mode since we're about to return
            self.wqdb.clear_mode();
            return;
        }

        if let Some((cond_pc, exit_pc)) = cond_pc_and_exit {
            // First stmt in [cond_pc+1, exit_pc)
            for pc in (cond_pc + 1)..exit_pc {
                if meta.line_table.is_stmt(pc) {
                    self.wqdb.add_temp_break(crate::wqdb::CodeLoc {
                        chunk: here.chunk,
                        pc,
                    });
                    break;
                }
            }
        }
    }
    fn dbg_step_out(&mut self) {
        // If we're at a Return instruction, set up temp breaks in the caller
        // and clear step mode so we can continue properly
        if self.is_at_return() {
            if !self.call_stack.is_empty() {
                let caller_frame = &self.call_stack[self.call_stack.len() - 1];
                let caller_meta = self.debug_info.chunk(caller_frame.chunk);
                // Look for the next statement after the call site
                for pc in caller_frame.pc..caller_meta.len {
                    if caller_meta.line_table.is_stmt(pc) {
                        self.wqdb.add_temp_break(CodeLoc {
                            chunk: caller_frame.chunk,
                            pc,
                        });
                        break;
                    }
                }
            }
            // Clear step mode since we're about to return
            self.wqdb.clear_mode();
            return;
        }

        self.wqdb.req_out(self.call_depth());

        // Add temp break in caller frame if we have one
        if !self.call_stack.is_empty() {
            let caller_frame = &self.call_stack[self.call_stack.len() - 1];
            let caller_meta = self.debug_info.chunk(caller_frame.chunk);
            // Look for the next statement after the call site
            for pc in caller_frame.pc..caller_meta.len {
                if caller_meta.line_table.is_stmt(pc) {
                    self.wqdb.add_temp_break(CodeLoc {
                        chunk: caller_frame.chunk,
                        pc,
                    });
                    break;
                }
            }
        }
    }
    fn dbg_set_break(&mut self, loc: CodeLoc) {
        self.wqdb.breaks.insert(loc);
    }
    fn dbg_clear_break(&mut self, loc: CodeLoc) {
        self.wqdb.breaks.remove(&loc);
    }
    fn dbg_breakpoints(&self) -> Vec<CodeLoc> {
        self.wqdb.breaks.iter().cloned().collect()
    }
    fn dbg_reset_breaks(&mut self) {
        self.wqdb.breaks.clear();
    }

    fn bt_frames(&self) -> Vec<(CodeLoc, std::sync::Arc<str>)> {
        let mut v = Vec::new();
        v.push((
            CodeLoc {
                chunk: self.current_chunk,
                pc: self.pc,
            },
            std::sync::Arc::from(self.func_name_for_chunk(self.current_chunk)),
        ));
        for fr in self.call_stack.iter().rev() {
            v.push((
                CodeLoc {
                    chunk: fr.chunk,
                    pc: fr.pc.saturating_sub(1),
                },
                fr.func_name.clone(),
            ));
        }
        v
    }

    fn dbg_globals(&self) -> Vec<(String, Value)> {
        self.globals
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
    fn dbg_locals(&self) -> Vec<(usize, Value)> {
        if let Some(frame) = self.locals.last() {
            frame.iter().cloned().enumerate().collect()
        } else {
            Vec::new()
        }
    }
    fn is_at_return(&self) -> bool {
        if self.pc < self.instructions.len() {
            matches!(
                self.instructions[self.pc],
                crate::vm::instruction::Instruction::Return
            )
        } else {
            false
        }
    }
    fn dbg_ins_at(&self, pc: usize) -> Option<String> {
        self.instructions.get(pc).map(|ins| format!("{ins:?}"))
    }
}
