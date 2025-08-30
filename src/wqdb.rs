use colored::Colorize;

use crate::repl::get_debug_level;
use crate::value::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone)]
pub struct SourceFile {
    pub id: u32,
    pub path: Arc<str>,
    pub text: Arc<str>,
    pub line_starts: Arc<Vec<usize>>,
}
impl SourceFile {
    pub fn new(id: u32, path: impl Into<Arc<str>>, text: impl Into<Arc<str>>) -> Self {
        let text: Arc<str> = text.into();
        let mut offs = vec![0usize];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                offs.push(i + 1);
            }
        }
        Self {
            id,
            path: path.into(),
            text,
            line_starts: Arc::new(offs),
        }
    }
    pub fn line_col(&self, byte_off: usize) -> (usize, usize) {
        let i = match self.line_starts.binary_search(&byte_off) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let start = self.line_starts[i];
        (i + 1, byte_off - start + 1)
    }
    pub fn line_snippet(&self, line1: usize) -> &str {
        let i = line1.saturating_sub(1);
        let s = *self.line_starts.get(i).unwrap_or(&0);
        let e = *self.line_starts.get(i + 1).unwrap_or(&self.text.len());
        &self.text[s..e]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub file_id: u32,
    pub start: u32,
    pub end: u32,
}
impl Span {
    pub const NONE: Span = Span {
        file_id: u32::MAX,
        start: 0,
        end: 0,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ChunkId(pub u32);

#[derive(Default, Clone)]
pub struct LineTable {
    pub pc_to_stmt_span: Vec<Span>,
    pub is_stmt_pc: Vec<bool>,
}
impl LineTable {
    pub fn ensure(&mut self, n: usize) {
        if self.pc_to_stmt_span.len() < n {
            self.pc_to_stmt_span.resize(n, Span::NONE);
            self.is_stmt_pc.resize(n, false);
        }
    }
    pub fn mark_stmt(&mut self, last_pc: usize, span: Span) {
        self.ensure(last_pc + 1);
        self.pc_to_stmt_span[last_pc] = span;
        self.is_stmt_pc[last_pc] = true;
    }
    pub fn span_at(&self, mut pc: usize) -> Span {
        if pc >= self.pc_to_stmt_span.len() {
            pc = self.pc_to_stmt_span.len().saturating_sub(1);
        }
        loop {
            if let Some(s) = self.pc_to_stmt_span.get(pc) {
                if s.file_id != u32::MAX {
                    return *s;
                }
            }
            if pc == 0 {
                return Span::NONE;
            }
            pc -= 1;
        }
    }
    pub fn is_stmt(&self, pc: usize) -> bool {
        self.is_stmt_pc.get(pc).copied().unwrap_or(false)
    }
}

#[derive(Clone)]
pub struct ChunkMeta {
    pub id: ChunkId,
    pub name: Arc<str>,
    pub file_id: u32,
    pub len: usize,
    pub line_table: LineTable,
    pub local_names: Option<Vec<String>>, // slot-indexed local names
}

#[derive(Default)]
pub struct DebugInfo {
    pub files: HashMap<u32, Arc<SourceFile>>,
    pub chunks: HashMap<ChunkId, ChunkMeta>,
    pub by_name: HashMap<Arc<str>, ChunkId>,
    next_chunk: u32,
    next_file: u32,
}
impl DebugInfo {
    pub fn register_file(&mut self, file: SourceFile) {
        self.files.insert(file.id, Arc::new(file));
    }
    pub fn new_file(&mut self, path: impl Into<Arc<str>>, text: impl Into<Arc<str>>) -> u32 {
        let id = self.next_file;
        self.next_file += 1;
        let sf = SourceFile::new(id, path, text);
        self.register_file(sf);
        id
    }
    pub fn new_chunk(&mut self, name: impl Into<Arc<str>>, file_id: u32, len: usize) -> ChunkId {
        let id = ChunkId(self.next_chunk);
        self.next_chunk += 1;
        let name_arc: Arc<str> = name.into();
        self.chunks.insert(
            id,
            ChunkMeta {
                id,
                name: name_arc.clone(),
                file_id,
                len,
                line_table: LineTable::default(),
                local_names: None,
            },
        );
        // Record name->chunk mapping for convenience
        self.by_name.insert(name_arc, id);
        id
    }
    pub fn chunk(&self, id: ChunkId) -> &ChunkMeta {
        &self.chunks[&id]
    }
    pub fn chunk_mut(&mut self, id: ChunkId) -> &mut ChunkMeta {
        self.chunks.get_mut(&id).expect("chunk exists")
    }
    pub fn file(&self, id: u32) -> Option<&Arc<SourceFile>> {
        self.files.get(&id)
    }

    pub fn resolve_line(&self, file_id: u32, line_1based: usize) -> Vec<CodeLoc> {
        let mut out = Vec::new();
        let mut heuristic_candidates = Vec::new();

        for meta in self.chunks.values() {
            for pc in 0..meta.len {
                if meta.line_table.is_stmt(pc) {
                    let sp = meta.line_table.span_at(pc);
                    if sp.file_id == file_id {
                        if let Some(sf) = self.files.get(&file_id) {
                            let (ln, _) = sf.line_col(sp.start as usize);
                            if ln == line_1based {
                                out.push(CodeLoc { chunk: meta.id, pc });
                            }
                        }
                    } else if sp.file_id == u32::MAX && meta.file_id == file_id {
                        // This is a heuristically marked statement - collect all for fallback
                        heuristic_candidates.push((CodeLoc { chunk: meta.id, pc }, meta));
                    }
                }
            }
        }

        // If we found exact matches, return them
        if !out.is_empty() {
            return out;
        }

        // Fallback for heuristic statements (those with Span::NONE): use a simple approach
        // based on the fact that statements roughly correspond to source lines in order
        for (candidate, meta) in heuristic_candidates {
            // Count statements before this one in the same chunk
            let mut stmt_index = 0;
            for pc in 0..candidate.pc {
                if meta.line_table.is_stmt(pc) {
                    stmt_index += 1;
                }
            }

            // Simple mapping: statement N roughly corresponds to line N+2
            // (accounting for function header at line 1)
            let estimated_line = stmt_index + 2;

            // Allow some tolerance for the mapping
            if estimated_line >= line_1based.saturating_sub(1) && estimated_line <= line_1based + 1
            {
                out.push(candidate);
            }
        }

        out
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct CodeLoc {
    pub chunk: ChunkId,
    pub pc: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepMode {
    None,
    In,
    Over,
    Out,
}

pub struct Wqdb {
    pub enabled: bool,
    pub breaks: HashSet<CodeLoc>,
    temps: HashSet<CodeLoc>,
    mode: StepMode,
    base_depth: usize,
    last_stmt: Option<CodeLoc>,
    step_count: u64,
    pub on_pause: Option<fn(&mut dyn DebugHost)>,
}
impl Default for Wqdb {
    fn default() -> Self {
        Self {
            enabled: false,
            breaks: HashSet::new(),
            temps: HashSet::new(),
            mode: StepMode::None,
            base_depth: 0,
            last_stmt: None,
            step_count: 0,
            on_pause: None,
        }
    }
}

pub trait DebugHost {
    fn loc(&self) -> CodeLoc;
    fn call_depth(&self) -> usize;
    fn di(&self) -> &DebugInfo;

    fn dbg_continue(&mut self);
    fn dbg_step_in(&mut self);
    fn dbg_step_over(&mut self);
    fn dbg_step_out(&mut self);
    fn dbg_set_break(&mut self, loc: CodeLoc);
    fn dbg_clear_break(&mut self, loc: CodeLoc);
    /// List all persistent breakpoints.
    fn dbg_breakpoints(&self) -> Vec<CodeLoc>;
    /// Remove all persistent breakpoints.
    fn dbg_reset_breaks(&mut self);
    fn bt_frames(&self) -> Vec<(CodeLoc, Arc<str>)>;

    // Introspection helpers
    fn dbg_globals(&self) -> Vec<(String, Value)>;
    fn dbg_locals(&self) -> Vec<(usize, Value)>;
    /// Check if current instruction is a Return
    fn is_at_return(&self) -> bool;
    /// Stringifies instruction at program counter in current chunk, if available
    fn dbg_ins_at(&self, pc: usize) -> Option<String>;
}

impl Wqdb {
    pub fn clear_mode(&mut self) {
        self.mode = StepMode::None;
    }
    #[inline]
    pub fn should_pause_at(&self, di: &DebugInfo, here: CodeLoc, call_depth: usize) -> bool {
        if !self.enabled {
            return false;
        }

        if self.last_stmt.is_none() && here.pc == 0 && call_depth == 0 {
            return true;
        }
        if self.breaks.contains(&here) {
            if crate::repl::get_debug_level() >= 2 {
                eprintln!(
                    "[wqdb]: pausing at persistent breakpoint: chunk {:?} pc {}",
                    here.chunk, here.pc
                );
            }
            return true;
        }
        if self.temps.contains(&here) {
            if crate::repl::get_debug_level() >= 2 {
                eprintln!(
                    "[wqdb]: pausing at temp breakpoint: chunk {:?} pc {}",
                    here.chunk, here.pc
                );
            }
            return true;
        }
        let meta = di.chunk(here.chunk);
        let is_stmt = meta.line_table.is_stmt(here.pc);

        match self.mode {
            StepMode::None => false,
            StepMode::In => is_stmt && Some(here) != self.last_stmt,
            StepMode::Over => {
                call_depth <= self.base_depth && is_stmt && Some(here) != self.last_stmt
            }
            StepMode::Out => {
                call_depth < self.base_depth && is_stmt && Some(here) != self.last_stmt
            }
        }
    }
    pub fn pause(&mut self, host: &mut dyn DebugHost) {
        let loc = host.loc();
        self.last_stmt = Some(loc);
        self.step_count += 1;

        // For step-in, clear mode after each step to implement single-step behavior
        // For step-over and step-out, use the helper method
        if self.mode == StepMode::In {
            self.mode = StepMode::None;
        } else {
            self.maybe_clear_step_mode(host);
        }

        if let Some(cb) = self.on_pause {
            cb(host);
        }
        self.temps.clear();
    }

    pub fn note_pause(&mut self, loc: CodeLoc) {
        self.last_stmt = Some(loc);
        self.step_count += 1;
        self.temps.clear();
        // Don't clear step mode here - let the stepping methods manage mode transitions
    }
    pub fn req_in(&mut self, depth: usize) {
        self.mode = StepMode::In;
        self.base_depth = depth;
        // Keep last_stmt so subsequent step-in advances to a new source span
        self.step_count = 0;
    }
    pub fn req_over(&mut self, depth: usize) {
        self.mode = StepMode::Over;
        self.base_depth = depth;
        self.step_count = 0;
        // Don't reset last_stmt for step-over to avoid revisiting same statement
    }
    pub fn req_out(&mut self, depth: usize) {
        self.mode = StepMode::Out;
        self.base_depth = depth;
        self.step_count = 0;
    }

    pub fn temp_break_next_stmt_in_chunk(&mut self, host: &dyn DebugHost) {
        let here = host.loc();
        let meta = host.di().chunk(here.chunk);
        for pc in here.pc + 1..meta.len {
            if meta.line_table.is_stmt(pc) {
                self.temps.insert(CodeLoc {
                    chunk: here.chunk,
                    pc,
                });
                break;
            }
        }
    }

    pub fn add_temp_break(&mut self, loc: CodeLoc) {
        if crate::repl::get_debug_level() >= 2 {
            eprintln!(
                "[wqdb]: adding temp break at chunk {:?} pc {}",
                loc.chunk, loc.pc
            );
        }
        self.temps.insert(loc);
    }

    /// Check if we should maintain step-in mode based on current context
    pub fn should_maintain_step_mode(&self, host: &dyn DebugHost) -> bool {
        if self.mode != StepMode::In {
            return false;
        }

        // Always maintain step-in mode unless we're at a return at the original call depth
        if host.is_at_return() {
            host.call_depth() > self.base_depth
        } else {
            true
        }
    }

    /// Clear step mode only if appropriate based on context
    pub fn maybe_clear_step_mode(&mut self, host: &dyn DebugHost) {
        if !self.should_maintain_step_mode(host) {
            self.mode = StepMode::None;
        }
    }
}

pub fn format_frame(di: &DebugInfo, loc: CodeLoc, name: &str) -> String {
    let meta = di.chunk(loc.chunk);
    let mut span = meta.line_table.span_at(loc.pc);

    // Check if this is an uncertain location before trying file lookup
    if span.file_id == u32::MAX {
        // Try a more helpful fallback: use the first statement in this chunk
        let mut first_stmt_pc: Option<usize> = None;
        for pc in 0..meta.len {
            if meta.line_table.is_stmt(pc) {
                first_stmt_pc = Some(pc);
                break;
            }
        }
        if let Some(pc0) = first_stmt_pc {
            let s2 = meta.line_table.span_at(pc0);
            if s2.file_id != u32::MAX {
                span = s2;
            }
        }
        // If still unknown, but we know the file for this chunk, show its path
        if span.file_id == u32::MAX {
            if let Some(sf) = di.file(meta.file_id) {
                return format!("At {name} ({}:?:?)\n   ? -> ?\n", sf.path)
                    .underline()
                    .bright_yellow()
                    .to_string();
            }
            // Last resort
            return format!("At {name} (?:?:?)\n   ? -> ?\n")
                .underline()
                .bright_yellow()
                .to_string();
        }
    }

    if let Some(sf) = di.file(span.file_id) {
        let (l, c) = sf.line_col(span.start as usize);
        let mut out = format!("At {} ({}:{}:{})\n", name, sf.path, l, c)
            .underline()
            .bright_yellow()
            .to_string();
        // Clamp 1-based line numbers correctly within [1, total]
        let total = sf.line_starts.len();
        let lo_ln = if l > 1 { l - 1 } else { 1 };
        let hi_ln = if l < total { l + 1 } else { total };
        for ln in lo_ln..=hi_ln {
            if ln == l {
                out.push_str(
                    &format!("{:>4} -> {}\n", ln, sf.line_snippet(ln).trim())
                        .green()
                        .bold()
                        .to_string(),
                );
            } else {
                out.push_str(&format!("{:>4}    {}\n", ln, sf.line_snippet(ln).trim()));
            }
        }
        out.to_string()
    } else {
        format!("At {} (chunk {:?} pc {})\n", name, meta.id, loc.pc)
            .underline()
            .yellow()
            .to_string()
    }
}

/// Heuristic statement markers: mark likely statement boundaries even without precise spans.
pub fn mark_stmt_heuristic(table: &mut LineTable, code: &[crate::vm::instruction::Instruction]) {
    use crate::vm::instruction::Instruction as I;
    if get_debug_level() >= 4 {
        eprintln!(
            "[wqdb]: mark_stmt_heuristic called with {} instructions",
            code.len()
        );
    }
    for (pc, op) in code.iter().enumerate() {
        let is_stmt = matches!(
            op,
            I::StoreVar(_)
                | I::StoreVarKeep(_)
                | I::StoreLocal(_)
                | I::StoreLocalKeep(_)
                | I::CallBuiltin(_, _)
                | I::CallBuiltinId(_, _)
                | I::CallLocal(_, _)
                | I::CallUser(_, _)
                | I::CallAnon(_)
                | I::CallOrIndex(_)
                | I::Index
                | I::IndexAssign
                | I::IndexAssignLocal(_)
                | I::IndexAssignDrop
                | I::IndexAssignLocalDrop(_)
                | I::JumpIfFalse(_)
                | I::JumpIfGE(_)
                | I::JumpIfLEZLocal(_, _)
                | I::Return
                | I::Assert
                // Avoid marking plain stack pops as separate statements to reduce duplicates in loops
                | I::Try(_)
                | I::BinaryOp(_)
                | I::UnaryOp(_)
                | I::LoadVar(_)
                | I::LoadConst(_)
                | I::LoadClosure { .. }
        );
        if is_stmt {
            if get_debug_level() >= 4 {
                eprintln!("[wqdb]: marking PC {pc} as statement: {op:?}");
            }
            table.mark_stmt(pc, Span::NONE);
        }
    }
}

/// Replace statement markers with exact mapping from provided spans.
/// Uses heuristics only to pick candidate PCs, then clears all markers and
/// marks exactly one PC per span in order. Falls back to heuristic marking
/// when spans are insufficient for complex control structures.
pub fn apply_stmt_spans_exact(
    table: &mut LineTable,
    code: &[crate::vm::instruction::Instruction],
    file_id: u32,
    spans: &[(usize, usize)],
) {
    // Normalize spans: sort by start ascending and deduplicate
    let mut spans_sorted: Vec<(usize, usize)> = spans.to_vec();
    spans_sorted.sort_by_key(|(s, _)| *s);
    spans_sorted.dedup();
    if get_debug_level() >= 4 {
        eprintln!(
            "[wqdb]: apply_stmt_spans_exact called with {} instructions, {} spans ({} unique, sorted)",
            code.len(),
            spans.len(),
            spans_sorted.len(),
        );
        eprintln!("[wqdb]: spans(sorted) = {spans_sorted:?}");
    }

    // If we have insufficient spans for the instruction count, use hybrid approach:
    // Keep all heuristic statements but add span info where available
    if spans_sorted.is_empty() || spans_sorted.len() * 10 < code.len() {
        if get_debug_level() >= 4 {
            eprintln!(
                "[wqdb]: using heuristic fallback (spans.len() * 10 = {} < code.len() = {})",
                spans_sorted.len() * 10,
                code.len()
            );
        }
        // Mark all heuristic candidates first
        mark_stmt_heuristic(table, code);

        // Then add span information for the statements we do have spans for
        let mut cand: Vec<usize> = Vec::new();
        for pc in 0..code.len() {
            if table.is_stmt(pc) {
                cand.push(pc);
            }
        }

        if !spans_sorted.is_empty() && !cand.is_empty() {
            // Best-effort mapping even with sparse spans: distribute spans across candidates
            for (i, &pc) in cand.iter().enumerate() {
                let span_idx = (i * spans_sorted.len()) / cand.len();
                let span_idx = span_idx.min(spans_sorted.len() - 1);
                let (start, end) = spans_sorted[span_idx];
                table.pc_to_stmt_span[pc] = Span {
                    file_id,
                    start: start as u32,
                    end: end as u32,
                };
            }
        }

        return;
    }

    if get_debug_level() >= 4 {
        eprintln!("[wqdb]: proceeding with exact span mapping (overlay mode)");
    }

    // First, find candidate PCs using heuristics.
    // IMPORTANT: we DO NOT clear existing stmt markers here. We keep all
    // heuristic statement PCs so stepping can still pause on fine-grained
    // boundaries (e.g., inside unrolled loops). We only overlay span info
    // on top of existing candidates so the UI arrow points to useful lines.
    mark_stmt_heuristic(table, code);
    let len = code.len();
    let mut cand: Vec<usize> = Vec::new();
    for pc in 0..len {
        if table.is_stmt(pc) {
            cand.push(pc);
        }
    }

    table.ensure(len);

    if !spans_sorted.is_empty() && !cand.is_empty() {
        // Heuristic: detect container span (first span fully covering all others)
        let mut spans_for_map: Vec<(usize, usize)> = spans_sorted.clone();
        let mut has_container = false;
        if spans_for_map.len() >= 2 {
            let (s0, e0) = spans_for_map[0];
            let (_sn, en) = spans_for_map[spans_for_map.len() - 1];
            let contains_rest = s0 <= spans_for_map[1].0 && e0 >= en;
            if contains_rest {
                let container = spans_for_map.remove(0);
                spans_for_map.push(container);
                has_container = true;
                if get_debug_level() >= 4 {
                    eprintln!(
                        "[wqdb]: detected container span; remapped to end: {spans_for_map:?}"
                    );
                }
            }
        }

        if has_container {
            // Split into body spans and container span
            let container_span = Some(spans_for_map[spans_for_map.len() - 1]);
            let body_spans: Vec<(usize, usize)> = spans_for_map[..spans_for_map.len() - 1].to_vec();

            // Classify candidate PCs as call vs other to improve loop/body alignment.
            use crate::vm::instruction::Instruction as I;
            let mut call_idx: Vec<usize> = Vec::new();
            let mut other_idx: Vec<usize> = Vec::new();
            for (i, &pc) in cand.iter().enumerate() {
                let is_call = matches!(
                    code.get(pc),
                    Some(I::CallBuiltin(_, _))
                        | Some(I::CallBuiltinId(_, _))
                        | Some(I::CallLocal(_, _))
                        | Some(I::CallUser(_, _))
                        | Some(I::CallAnon(_))
                        | Some(I::CallOrIndex(_))
                );
                if is_call {
                    call_idx.push(i);
                } else {
                    other_idx.push(i);
                }
            }

            // Map calls round-robin across body spans to create a cyclic feel inside loops.
            if !body_spans.is_empty() {
                for (j, &i) in call_idx.iter().enumerate() {
                    let pc = cand[i];
                    let (start, end) = body_spans[j % body_spans.len()];
                    table.pc_to_stmt_span[pc] = Span {
                        file_id,
                        start: start as u32,
                        end: end as u32,
                    };
                }
            } else {
                // Fallback: map all calls to the only span available
                for &i in &call_idx {
                    let pc = cand[i];
                    let (start, end) = spans_for_map[0];
                    table.pc_to_stmt_span[pc] = Span {
                        file_id,
                        start: start as u32,
                        end: end as u32,
                    };
                }
            }

            // Map remaining (non-call) PCs to the container span.
            if let Some((start, end)) = container_span {
                for &i in &other_idx {
                    let pc = cand[i];
                    table.pc_to_stmt_span[pc] = Span {
                        file_id,
                        start: start as u32,
                        end: end as u32,
                    };
                }
            }
        } else {
            // No container scenario: distribute spans evenly across candidates (overlay)
            let nsp = spans_for_map.len();
            for (i, &pc) in cand.iter().enumerate() {
                let si = (i * nsp) / cand.len();
                let si = si.min(nsp - 1);
                let (start, end) = spans_for_map[si];
                table.pc_to_stmt_span[pc] = Span {
                    file_id,
                    start: start as u32,
                    end: end as u32,
                };
            }
        }
    }
}

/// Same as apply_stmt_spans_exact, but shifts all spans by a base byte offset
pub fn apply_stmt_spans_exact_offs(
    table: &mut crate::wqdb::LineTable,
    code: &[crate::vm::instruction::Instruction],
    file_id: u32,
    spans: &[(usize, usize)],
    base_offset: usize,
) {
    let shifted: Vec<(usize, usize)> = spans
        .iter()
        .map(|(s, e)| (s.saturating_add(base_offset), e.saturating_add(base_offset)))
        .collect();
    apply_stmt_spans_exact(table, code, file_id, &shifted);
}

/// Register chunks for nested non-capturing functions and mark their statement PCs.
pub fn register_function_chunks(
    di: &mut crate::wqdb::DebugInfo,
    file_id: u32,
    code: &[crate::vm::instruction::Instruction],
    base_offset: usize,
) {
    use crate::value::Value;
    use crate::vm::instruction::Instruction as I;
    let mut i = 0usize;
    while i < code.len() {
        let ins = &code[i];
        if let I::LoadConst(Value::CompiledFunction {
            instructions,
            dbg_chunk,
            dbg_stmt_spans,
            dbg_local_names,
            ..
        }) = ins
        {
            // Assign a chunk if missing
            if dbg_chunk.is_none() {
                let chunk = di.new_chunk("<fn>", file_id, instructions.len());
                let table = &mut di.chunk_mut(chunk).line_table;
                if let Some(spans) = dbg_stmt_spans {
                    apply_stmt_spans_exact_offs(table, instructions, file_id, spans, base_offset);
                } else {
                    mark_stmt_heuristic(table, instructions);
                }
                // Record local names if available
                if let Some(names) = dbg_local_names {
                    di.chunk_mut(chunk).local_names = Some(names.clone());
                }
                // Heuristic: if the next instruction stores this function into a variable,
                // use that name for the chunk and register it for lookup by name.
                if let Some(I::StoreVar(name) | I::StoreVarKeep(name)) = code.get(i + 1) {
                    let name_arc: std::sync::Arc<str> = std::sync::Arc::from(name.as_str());
                    di.chunk_mut(chunk).name = name_arc.clone();
                    di.by_name.insert(name_arc, chunk);
                }
                // Note: we cannot mutate dbg_chunk through &ins; will be set when this function is stored/used next compile pass
                // This will at least register the code for tracebacks/stepping by ChunkId.
                let _ = chunk; // silence unused if cfg changes
            }
        }
        i += 1;
    }
}
