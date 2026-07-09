use std::collections::HashMap;
use std::sync::Arc;

use unicode_width::UnicodeWidthChar as _;

use crate::value::Value;
use crate::vm::inst::Instruction;

type DebugByteSpan = (usize, usize);
pub(crate) type DebugStmtSpans = Arc<[DebugByteSpan]>;
pub(crate) type DebugPcSpans = Arc<[Option<DebugByteSpan>]>;
pub(crate) type DebugProvenance = Arc<[(CodeLoc, Arc<str>)]>;

pub(crate) type Backtrace = Vec<(CodeLoc, std::sync::Arc<str>)>;

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

    pub fn display_line_col(&self, byte_off: usize) -> (usize, usize) {
        const TAB_STOP: usize = 8;
        let (line, _) = self.line_col(byte_off);
        let line_start = self.line_starts[line - 1];
        let mut column = 0usize;
        for ch in self.text[line_start..byte_off].chars() {
            if ch == '\t' {
                column += TAB_STOP - column % TAB_STOP;
            } else {
                column += ch.width().unwrap_or(0);
            }
        }
        (line, column + 1)
    }

    pub fn line_snippet(&self, line1: usize) -> &str {
        let i = line1.saturating_sub(1);
        let s = *self.line_starts.get(i).unwrap_or(&0);
        let e = *self.line_starts.get(i + 1).unwrap_or(&self.text.len());
        &self.text[s..e]
    }

    pub fn line_bounds(&self, line1: usize) -> (usize, usize) {
        let i = line1.saturating_sub(1);
        let s = *self.line_starts.get(i).unwrap_or(&0);
        let e = *self.line_starts.get(i + 1).unwrap_or(&self.text.len());
        (s, e)
    }

    pub fn line_text(&self, line1: usize) -> &str {
        self.line_snippet(line1).trim_end_matches(['\n', '\r'])
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub file_id: u32,
    pub start: usize,
    pub end: usize,
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
    pub exact_pc_span: Vec<Span>,
    pub stmt_pcs: Vec<usize>,
}

impl LineTable {
    pub fn ensure(&mut self, n: usize) {
        if self.pc_to_stmt_span.len() < n {
            self.pc_to_stmt_span.resize(n, Span::NONE);
            self.is_stmt_pc.resize(n, false);
            self.exact_pc_span.resize(n, Span::NONE);
        }
    }

    pub fn mark_stmt(&mut self, last_pc: usize, span: Span) {
        self.ensure(last_pc + 1);
        self.pc_to_stmt_span[last_pc] = span;
        self.is_stmt_pc[last_pc] = true;
        if !self.stmt_pcs.contains(&last_pc) {
            self.stmt_pcs.push(last_pc);
            self.stmt_pcs.sort_unstable();
        }
    }

    pub fn set_exact_span(&mut self, pc: usize, span: Span) {
        self.ensure(pc + 1);
        self.exact_pc_span[pc] = span;
    }

    pub fn set_stmt_mark(&mut self, pc: usize, span: Span) {
        self.ensure(pc + 1);
        self.pc_to_stmt_span[pc] = span;
        self.is_stmt_pc[pc] = true;
        if !self.stmt_pcs.contains(&pc) {
            self.stmt_pcs.push(pc);
            self.stmt_pcs.sort_unstable();
        }
    }

    pub fn span_at(&self, mut pc: usize) -> Span {
        if let Some(s) = self.exact_pc_span.get(pc)
            && s.file_id != u32::MAX
        {
            return *s;
        }
        if pc >= self.pc_to_stmt_span.len() {
            pc = self.pc_to_stmt_span.len().saturating_sub(1);
        }
        loop {
            if let Some(s) = self.pc_to_stmt_span.get(pc)
                && s.file_id != u32::MAX
            {
                return *s;
            }

            if pc == 0 {
                return Span::NONE;
            }
            pc -= 1;
        }
    }

    pub fn context_span_at(&self, mut pc: usize) -> Span {
        let len = self.exact_pc_span.len().max(self.pc_to_stmt_span.len());
        if len == 0 {
            return Span::NONE;
        }
        if pc >= len {
            pc = len.saturating_sub(1);
        }
        loop {
            if let Some(s) = self.exact_pc_span.get(pc)
                && s.file_id != u32::MAX
            {
                return *s;
            }
            if let Some(s) = self.pc_to_stmt_span.get(pc)
                && s.file_id != u32::MAX
            {
                return *s;
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

    pub fn stmt_start_pc(&self, pc: usize) -> Option<usize> {
        if self.stmt_pcs.is_empty() {
            return None;
        }
        match self.stmt_pcs.binary_search(&pc) {
            Ok(idx) => Some(self.stmt_pcs[idx]),
            Err(0) => None,
            Err(idx) => Some(self.stmt_pcs[idx - 1]),
        }
    }
}

#[derive(Clone)]
pub struct ChunkMeta {
    pub id: ChunkId,
    pub name: Arc<str>,
    pub file_id: u32,
    pub len: usize,
    pub line_table: LineTable,
    pub(crate) has_exact_spans: bool,
    pub(crate) has_real_spans: bool,
    pub local_names: Option<Vec<String>>, // slot-indexed local names
}

impl ChunkMeta {
    pub(crate) fn note_debug_spans(&mut self, has_exact: bool, has_real: bool) {
        self.has_exact_spans |= has_exact;
        self.has_real_spans |= has_real;
    }
}

#[derive(Default)]
pub struct DebugInfo {
    files: HashMap<u32, Arc<SourceFile>>,
    chunks: HashMap<ChunkId, ChunkMeta>,
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
                has_exact_spans: false,
                has_real_spans: false,
                local_names: None,
            },
        );
        // Record name->chunk mapping for convenience
        self.by_name.insert(name_arc, id);
        id
    }

    // SAFETY: chunk id must exist
    pub fn chunk(&self, id: ChunkId) -> &ChunkMeta {
        self.chunks.get(&id).expect("chunk exists")
    }

    pub fn chunk_opt(&self, id: ChunkId) -> Option<&ChunkMeta> {
        self.chunks.get(&id)
    }

    // SAFETY: chunk id must exist
    pub fn chunk_mut(&mut self, id: ChunkId) -> &mut ChunkMeta {
        self.chunks.get_mut(&id).expect("chunk exists")
    }

    // SAFETY: chunk id must exist
    pub fn rename_chunk(&mut self, id: ChunkId, new_name: impl Into<Arc<str>>) {
        let new_name: Arc<str> = new_name.into();
        let meta = self.chunks.get_mut(&id).expect("chunk exists");
        if meta.name.as_ref() == new_name.as_ref() {
            return;
        }
        let old_name = meta.name.clone();
        meta.name = new_name.clone();
        if let Some(chunk_id) = self.by_name.get(old_name.as_ref())
            && *chunk_id == id
        {
            self.by_name.remove(old_name.as_ref());
        }
        self.by_name.insert(new_name, id);
    }

    pub fn file(&self, id: u32) -> Option<&Arc<SourceFile>> {
        self.files.get(&id)
    }

    pub fn file_id_by_path(&self, path: &str) -> Option<u32> {
        self.files
            .values()
            .find(|sf| sf.path.as_ref() == path)
            .map(|sf| sf.id)
    }

    pub fn file_ids_by_path(&self, path: &str) -> Vec<u32> {
        self.files
            .values()
            .filter(|sf| sf.path.as_ref() == path)
            .map(|sf| sf.id)
            .collect()
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
                            let (ln, _) = sf.line_col(sp.start);
                            if ln == line_1based {
                                out.push(CodeLoc { chunk: meta.id, pc });
                                // Evaluates an expression string in the current
                                // scope
                                // fn dbg_eval_expr(&mut self, expr: &str) ->
                                // Result<String, String>;
                            }
                        }
                    } else if sp.file_id == u32::MAX && meta.file_id == file_id {
                        // This is a heuristically marked statement - collect all for fallback
                        heuristic_candidates.push((CodeLoc { chunk: meta.id, pc }, meta));
                    }
                }
            }
        }
        if !out.is_empty() {
            return out;
        }
        // Fallback for heuristic statements (those with Span::NONE): use a simple
        // approach based on the fact that statements roughly correspond to
        // source lines in order
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

#[derive(Clone)]
pub struct DebugLocalsFrame {
    pub loc: CodeLoc,
    pub name: Arc<str>,
    pub locals: Vec<(usize, Value)>,
}

#[derive(Clone, Copy, Default)]
pub struct DebugStepHints {
    pub previous: Option<CodeLoc>,
    pub step: Option<CodeLoc>,
    pub next: Option<CodeLoc>,
    pub finish: Option<CodeLoc>,
}

pub struct DebugChunkSpec<'a> {
    pub(crate) dbg_chunk: Option<ChunkId>,
    pub(crate) instructions: &'a [Instruction],
    pub(crate) dbg_stmt_spans: &'a Option<DebugStmtSpans>,
    pub(crate) source_base_offset: usize,
    pub(crate) dbg_pc_spans: &'a Option<DebugPcSpans>,
    pub(crate) dbg_stmt_marks: &'a Option<std::sync::Arc<[crate::vm::inst::DebugStmtMark]>>,
    pub(crate) dbg_local_names: &'a Option<std::sync::Arc<[String]>>,
    pub(crate) params: &'a Option<std::sync::Arc<[String]>>,
}
