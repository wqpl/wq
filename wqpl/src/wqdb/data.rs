use std::collections::HashMap;
use std::sync::Arc;

use unicode_width::UnicodeWidthChar as _;

use crate::value::Value;
use crate::vm::inst::Instruction;

type DebugByteSpan = (usize, usize);
pub(crate) type DebugStmtSpans = Arc<[DebugByteSpan]>;
pub(crate) type DebugPcSpans = Arc<[Option<DebugByteSpan>]>;
pub(crate) type DebugProvenance = Arc<[(CodeLoc, Arc<str>)]>;

#[derive(Clone)]
pub struct SourceFile {
    id: u32,
    path: Arc<str>,
    text: Arc<str>,
    line_starts: Arc<Vec<usize>>,
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

    pub(crate) fn clamp_byte_offset(&self, byte_off: usize) -> usize {
        let mut byte_off = byte_off.min(self.text.len());
        while !self.text.is_char_boundary(byte_off) {
            byte_off = byte_off.saturating_sub(1);
        }
        byte_off
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Number of source line starts, including the empty trailing line after
    /// a final newline.
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    pub fn line_col(&self, byte_off: usize) -> (usize, usize) {
        let byte_off = self.clamp_byte_offset(byte_off);
        let i = match self.line_starts.binary_search(&byte_off) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let start = self.line_starts[i];
        (i + 1, byte_off - start + 1)
    }

    pub fn display_line_col(&self, byte_off: usize) -> (usize, usize) {
        const TAB_STOP: usize = 8;
        let byte_off = self.clamp_byte_offset(byte_off);
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
    by_name: HashMap<Arc<str>, ChunkId>,
    function_by_name: HashMap<Arc<str>, ChunkId>,
    next_chunk: u32,
    next_file: u32,
}

impl DebugInfo {
    fn register_file(&mut self, file: SourceFile) {
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
        self.insert_chunk(name.into(), file_id, len, true)
    }

    pub(crate) fn new_function_chunk(
        &mut self,
        name: Option<Arc<str>>,
        file_id: u32,
        len: usize,
    ) -> ChunkId {
        let display_name = name.as_ref().cloned().unwrap_or_else(|| Arc::from("<fn>"));
        let id = self.insert_chunk(display_name, file_id, len, false);
        if let Some(name) = name {
            self.by_name.insert(Arc::clone(&name), id);
            self.function_by_name.insert(name, id);
        }
        id
    }

    fn insert_chunk(
        &mut self,
        name_arc: Arc<str>,
        file_id: u32,
        len: usize,
        register_name: bool,
    ) -> ChunkId {
        let id = ChunkId(self.next_chunk);
        self.next_chunk += 1;
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
        if register_name {
            self.by_name.insert(name_arc, id);
        }
        id
    }

    /// Return a chunk when the identifier is still registered.
    ///
    /// Public consumers must treat code locations as staleable because a
    /// debugger can retain one across a workspace reset.
    pub fn get_chunk(&self, id: ChunkId) -> Option<&ChunkMeta> {
        self.chunks.get(&id)
    }

    pub(crate) fn get_chunk_mut(&mut self, id: ChunkId) -> Option<&mut ChunkMeta> {
        self.chunks.get_mut(&id)
    }

    /// Register an exact source span for one instruction.
    ///
    /// Returns `false` when the location or source span is not registered and
    /// valid for this debug-info workspace.
    pub fn set_exact_span(&mut self, location: CodeLoc, span: Span) -> bool {
        if !self.is_valid_source_span(span) {
            return false;
        }
        let Some(chunk) = self.chunks.get_mut(&location.chunk) else {
            return false;
        };
        if location.pc >= chunk.len {
            return false;
        }
        chunk.line_table.set_exact_span(location.pc, span);
        true
    }

    /// Register a statement source span for one instruction.
    ///
    /// Returns `false` when the location or source span is not registered and
    /// valid for this debug-info workspace.
    pub fn set_statement_span(&mut self, location: CodeLoc, span: Span) -> bool {
        if !self.is_valid_source_span(span) {
            return false;
        }
        let Some(chunk) = self.chunks.get_mut(&location.chunk) else {
            return false;
        };
        if location.pc >= chunk.len {
            return false;
        }
        chunk.line_table.set_stmt_mark(location.pc, span);
        true
    }

    fn is_valid_source_span(&self, span: Span) -> bool {
        if span == Span::NONE {
            return true;
        }
        self.files.get(&span.file_id).is_some_and(|file| {
            span.start <= span.end
                && span.end <= file.text.len()
                && file.text.is_char_boundary(span.start)
                && file.text.is_char_boundary(span.end)
        })
    }

    pub(crate) fn expect_chunk(&self, id: ChunkId) -> &ChunkMeta {
        self.get_chunk(id).expect("chunk must be registered")
    }

    pub(crate) fn expect_chunk_mut(&mut self, id: ChunkId) -> &mut ChunkMeta {
        self.get_chunk_mut(id).expect("chunk must be registered")
    }

    /// Resolve a code location to its registered chunk and source coordinates.
    ///
    /// Locations are intentionally fallible because debugger clients may hold
    /// locations across workspace resets or receive incomplete runtime state.
    pub fn resolve_location(&self, location: CodeLoc) -> Option<ResolvedCodeLoc> {
        let chunk = self.get_chunk(location.chunk)?;
        if location.pc >= chunk.len {
            return None;
        }
        let span = chunk.line_table.context_span_at(location.pc);
        let source = if span.file_id == u32::MAX {
            None
        } else {
            self.file(span.file_id).and_then(|file| {
                if span.start > span.end {
                    return None;
                }
                let mut byte = span.start.min(file.text.len());
                while !file.text.is_char_boundary(byte) {
                    byte = byte.saturating_sub(1);
                }
                let mut end = span.end.min(file.text.len());
                while !file.text.is_char_boundary(end) {
                    end = end.saturating_sub(1);
                }
                if end <= byte {
                    end = file.text[byte..]
                        .chars()
                        .next()
                        .map_or(byte, |ch| byte + ch.len_utf8());
                }
                let (line, column) = file.display_line_col(byte);
                Some(SourceLocation {
                    path: Arc::clone(&file.path),
                    source: Arc::clone(&file.text),
                    span: Span {
                        start: byte,
                        end,
                        ..span
                    },
                    line,
                    column,
                })
            })
        };
        Some(ResolvedCodeLoc {
            location,
            function: Arc::clone(&chunk.name),
            source,
        })
    }

    fn rename_chunk(&mut self, id: ChunkId, new_name: impl Into<Arc<str>>) {
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

    pub(crate) fn rename_function_chunk(&mut self, id: ChunkId, new_name: impl Into<Arc<str>>) {
        let new_name = new_name.into();
        self.rename_chunk(id, Arc::clone(&new_name));
        self.function_by_name.insert(new_name, id);
    }

    pub fn function_chunk(&self, name: &str) -> Option<ChunkId> {
        self.function_by_name.get(name).copied()
    }

    pub fn function_names(&self) -> impl Iterator<Item = &str> {
        self.function_by_name.keys().map(AsRef::as_ref)
    }

    pub(crate) fn remove_function_name(&mut self, name: &str) {
        self.function_by_name.remove(name);
    }

    pub(crate) fn clear_function_names(&mut self) {
        self.function_by_name.clear();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_registry_excludes_synthetic_chunks() {
        let mut info = DebugInfo::default();
        let file = info.new_file("test", "");
        info.new_chunk("<script>", file, 1);
        info.new_function_chunk(None, file, 1);
        let function = info.new_function_chunk(Some(Arc::from("f")), file, 1);

        assert_eq!(info.function_chunk("f"), Some(function));
        assert_eq!(info.function_chunk("<script>"), None);
        assert_eq!(info.function_chunk("<fn>"), None);
        assert!(!info.by_name.contains_key("<fn>"));
    }

    #[test]
    fn function_registry_preserves_aliases_when_chunk_is_renamed() {
        let mut info = DebugInfo::default();
        let file = info.new_file("test", "");
        let function = info.new_function_chunk(Some(Arc::from("f")), file, 1);

        info.rename_function_chunk(function, "g");

        assert_eq!(info.function_chunk("f"), Some(function));
        assert_eq!(info.function_chunk("g"), Some(function));
        assert_eq!(info.expect_chunk(function).name.as_ref(), "g");
    }

    #[test]
    fn resolve_location_rejects_stale_chunk_and_program_counter() {
        let mut info = DebugInfo::default();
        let file = info.new_file("test", "1/0");
        let chunk = info.new_chunk("<script>", file, 1);

        assert!(
            info.resolve_location(CodeLoc {
                chunk: ChunkId(u32::MAX),
                pc: 0,
            })
            .is_none()
        );
        assert!(info.resolve_location(CodeLoc { chunk, pc: 1 }).is_none());
    }

    #[test]
    fn resolve_location_keeps_missing_source_optional() {
        let mut info = DebugInfo::default();
        let chunk = info.new_chunk("<script>", 42, 1);

        let without_span = info
            .resolve_location(CodeLoc { chunk, pc: 0 })
            .expect("registered location");
        assert!(without_span.source.is_none());

        info.expect_chunk_mut(chunk).line_table.set_exact_span(
            0,
            Span {
                file_id: 42,
                start: 0,
                end: 1,
            },
        );
        let without_file = info
            .resolve_location(CodeLoc { chunk, pc: 0 })
            .expect("registered location");
        assert!(without_file.source.is_none());
    }

    #[test]
    fn resolve_location_clamps_spans_to_utf8_boundaries() {
        let mut info = DebugInfo::default();
        let file = info.new_file("test", "aéz");
        let chunk = info.new_chunk("<script>", file, 1);
        info.expect_chunk_mut(chunk).line_table.set_exact_span(
            0,
            Span {
                file_id: file,
                start: 2,
                end: 2,
            },
        );

        let source = info
            .resolve_location(CodeLoc { chunk, pc: 0 })
            .expect("registered location")
            .source
            .expect("registered source");
        assert_eq!((source.span.start, source.span.end), (1, 3));
        assert_eq!((source.line, source.column), (1, 2));
        assert_eq!(&source.source[source.span.start..source.span.end], "é");
    }

    #[test]
    fn public_span_registration_rejects_invalid_metadata() {
        let mut info = DebugInfo::default();
        let file = info.new_file("test", "é");
        let chunk = info.new_chunk("<script>", file, 1);
        let valid = Span {
            file_id: file,
            start: 0,
            end: 2,
        };

        assert!(!info.set_exact_span(
            CodeLoc {
                chunk: ChunkId(u32::MAX),
                pc: 0,
            },
            valid,
        ));
        assert!(!info.set_statement_span(CodeLoc { chunk, pc: 1 }, valid));
        assert!(!info.set_exact_span(
            CodeLoc { chunk, pc: 0 },
            Span {
                file_id: 99,
                start: 0,
                end: 1,
            },
        ));
        assert!(!info.set_exact_span(
            CodeLoc { chunk, pc: 0 },
            Span {
                file_id: file,
                start: 1,
                end: 2,
            },
        ));
        assert!(info.set_exact_span(CodeLoc { chunk, pc: 0 }, valid));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct CodeLoc {
    pub chunk: ChunkId,
    pub pc: usize,
}

#[derive(Clone, Debug)]
pub struct DebugLocalsFrame {
    pub loc: CodeLoc,
    pub name: Arc<str>,
    pub locals: Vec<(usize, Value)>,
}

#[derive(Clone, Debug)]
pub struct ResolvedCodeLoc {
    pub location: CodeLoc,
    pub function: Arc<str>,
    pub source: Option<SourceLocation>,
}

#[derive(Clone, Debug)]
pub struct SourceLocation {
    pub path: Arc<str>,
    pub source: Arc<str>,
    pub span: Span,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct CrashId(u64);

impl CrashId {
    pub fn get(self) -> u64 {
        self.0
    }

    pub(crate) fn new(id: u64) -> Self {
        Self(id)
    }
}

/// One logical frame captured at the point where execution failed.
///
/// Locals live on the same frame as its location so debugger frame indexes
/// cannot drift away from their corresponding local values.
#[derive(Clone, Debug)]
pub enum CrashFrame {
    Located {
        function: Arc<str>,
        location: CodeLoc,
        source: Option<SourceLocation>,
        locals: Option<Arc<[(usize, Value)]>>,
    },
    TailCallsOmitted,
}

impl CrashFrame {
    pub fn function(&self) -> &str {
        match self {
            Self::Located { function, .. } => function,
            Self::TailCallsOmitted => "(... tail calls omitted ...)",
        }
    }

    pub fn location(&self) -> Option<CodeLoc> {
        match self {
            Self::Located { location, .. } => Some(*location),
            Self::TailCallsOmitted => None,
        }
    }

    pub fn locals(&self) -> Option<&[(usize, Value)]> {
        match self {
            Self::Located { locals, .. } => locals.as_deref(),
            Self::TailCallsOmitted => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CrashSnapshot {
    id: CrashId,
    frames: Arc<[CrashFrame]>,
    instructions: Arc<[Option<Arc<[Instruction]>>]>,
}

impl CrashSnapshot {
    pub(crate) fn new(
        id: CrashId,
        frames: Vec<CrashFrame>,
        instructions: Vec<Option<Arc<[Instruction]>>>,
    ) -> Self {
        debug_assert_eq!(frames.len(), instructions.len());
        Self {
            id,
            frames: Arc::from(frames),
            instructions: Arc::from(instructions),
        }
    }

    pub fn id(&self) -> CrashId {
        self.id
    }

    pub fn frames(&self) -> &[CrashFrame] {
        &self.frames
    }

    pub(crate) fn instructions(&self, frame: usize) -> Option<&Arc<[Instruction]>> {
        self.instructions.get(frame)?.as_ref()
    }
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
