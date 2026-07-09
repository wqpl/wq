pub mod build;
pub mod data;
pub mod model;

use std::collections::{HashMap, HashSet};

use unicode_width::UnicodeWidthChar as _;

use crate::session::dbglog::{DebugLogFlags, get_debug_log_flags};
use crate::style::{AnsiColor, ColorMode, TextStyle, paint};
use crate::vm::Vm;
use crate::wqdb::data::{CodeLoc, DebugInfo};
use crate::wqdb::model::{
    Breakpoint, BreakpointKind, StepGranularity, StepMode, StopHook, SymbolTrackTarget,
    SymbolTracker,
};

pub struct Wqdb {
    pub enabled: bool,
    pub breaks: HashMap<CodeLoc, Breakpoint>,
    pub next_break_id: usize,
    temps: HashSet<CodeLoc>,
    mode: StepMode,
    granularity: StepGranularity,
    base_depth: usize,
    last_pause: Option<CodeLoc>,
    current_pause: Option<CodeLoc>,
    symbol_trackers: Vec<SymbolTracker>,
    next_symbol_tracker_id: usize,
    stop_hooks: Vec<StopHook>,
    next_stop_hook_id: usize,
    pub on_pause: Option<fn(&mut Vm)>,
    pub batch_cmds: Vec<String>,
}

impl Default for Wqdb {
    fn default() -> Self {
        Self {
            enabled: false,
            breaks: HashMap::new(),
            next_break_id: 1,
            temps: HashSet::new(),
            mode: StepMode::None,
            granularity: StepGranularity::default(),
            base_depth: 0,
            last_pause: None,
            current_pause: None,
            symbol_trackers: Vec::new(),
            next_symbol_tracker_id: 1,
            stop_hooks: Vec::new(),
            next_stop_hook_id: 1,
            on_pause: None,
            batch_cmds: Vec::new(),
        }
    }
}

impl Wqdb {
    pub fn clear_mode(&mut self) {
        self.mode = StepMode::None;
        self.current_pause = None;
    }

    fn alloc_breakpoint(&mut self, kind: BreakpointKind) -> Breakpoint {
        let id = self.next_break_id;
        self.next_break_id += 1;
        Breakpoint {
            id,
            enabled: true,
            kind,
        }
    }

    pub fn ensure_breakpoint(&mut self, loc: CodeLoc, kind: BreakpointKind) -> &mut Breakpoint {
        if !self.breaks.contains_key(&loc) {
            let bp = self.alloc_breakpoint(kind);
            self.breaks.insert(loc, bp);
        }
        self.breaks
            .get_mut(&loc)
            .expect("breakpoint must exist after insertion")
    }

    pub fn pause_break_enabled(&mut self, loc: CodeLoc) -> bool {
        self.ensure_breakpoint(loc, BreakpointKind::Pause).enabled
    }

    #[inline]
    pub fn should_pause_at(&self, di: &DebugInfo, here: CodeLoc, call_depth: usize) -> bool {
        if !self.enabled {
            return false;
        }
        if self.last_pause.is_none() && here.pc == 0 && call_depth == 0 {
            return true;
        }
        if let Some(bp) = self.breaks.get(&here) {
            if !bp.enabled {
                return false;
            }
            if bp.kind == BreakpointKind::Pause {
                return false;
            }
            if get_debug_log_flags().contains(DebugLogFlags::WQDB) {
                eprintln!(
                    "[wqdb]: pausing at persistent breakpoint {}: chunk {:?} pc {}",
                    bp.id, here.chunk, here.pc
                );
            }
            return true;
        }
        if self.temps.contains(&here) {
            if get_debug_log_flags().contains(DebugLogFlags::WQDB) {
                eprintln!(
                    "[wqdb]: pausing at temp breakpoint: chunk {:?} pc {}",
                    here.chunk, here.pc
                );
            }
            return true;
        }
        let meta = di.chunk(here.chunk);
        let is_stmt = meta.line_table.is_stmt(here.pc);
        let is_step_point = match self.granularity {
            StepGranularity::Line => {
                is_stmt && !self.is_same_line_as_last_pause(di, here, call_depth)
            }
            StepGranularity::Expr => is_stmt,
            StepGranularity::Inst => true,
        };
        match self.mode {
            StepMode::None => false,
            StepMode::In => is_step_point,
            StepMode::Over => call_depth <= self.base_depth && is_step_point,
            StepMode::Out => call_depth < self.base_depth && is_step_point,
        }
    }

    fn is_same_line_as_last_pause(&self, di: &DebugInfo, here: CodeLoc, call_depth: usize) -> bool {
        if call_depth != self.base_depth {
            return false;
        }
        let Some(last) = self.last_pause else {
            return false;
        };
        if last.chunk != here.chunk {
            return false;
        }
        match (
            Self::source_line_at(di, last),
            Self::source_line_at(di, here),
        ) {
            (Some(last_line), Some(here_line)) => last_line == here_line,
            _ => last == here,
        }
    }

    fn source_line_at(di: &DebugInfo, loc: CodeLoc) -> Option<(u32, usize)> {
        let meta = di.chunk_opt(loc.chunk)?;
        let mut span = meta.line_table.context_span_at(loc.pc);
        if span.file_id == u32::MAX {
            for &pc in &meta.line_table.stmt_pcs {
                if pc >= loc.pc {
                    span = meta.line_table.span_at(pc);
                    if span.file_id != u32::MAX {
                        break;
                    }
                }
            }
        }
        let file = di.file(span.file_id)?;
        Some((span.file_id, file.line_col(span.start).0))
    }

    pub fn note_pause(&mut self, loc: CodeLoc) {
        self.last_pause = Some(loc);
        self.current_pause = Some(loc);
        self.temps.clear();
        // Don't clear step mode here - let the stepping methods manage mode
        // transitions
    }

    pub fn req_in(&mut self, depth: usize) {
        self.mode = StepMode::In;
        self.base_depth = depth;
        self.current_pause = None;
    }

    pub fn req_over(&mut self, depth: usize) {
        self.mode = StepMode::Over;
        self.base_depth = depth;
        self.current_pause = None;
    }

    pub fn req_out(&mut self, depth: usize) {
        self.mode = StepMode::Out;
        self.base_depth = depth;
        self.current_pause = None;
    }

    pub fn pause_loc(&self) -> Option<CodeLoc> {
        self.current_pause
    }

    pub fn add_temp_break(&mut self, loc: CodeLoc) {
        if get_debug_log_flags().contains(DebugLogFlags::WQDB) {
            eprintln!(
                "[wqdb]: adding temp break at chunk {:?} pc {}",
                loc.chunk, loc.pc
            );
        }
        self.temps.insert(loc);
    }

    pub fn mode(&self) -> StepMode {
        self.mode
    }

    pub fn granularity(&self) -> StepGranularity {
        self.granularity
    }

    pub fn set_granularity(&mut self, granularity: StepGranularity) {
        self.granularity = granularity;
    }

    pub fn ensure_symbol_tracker(&mut self, target: SymbolTrackTarget) -> (&SymbolTracker, bool) {
        if let Some(index) = self
            .symbol_trackers
            .iter()
            .position(|tracker| tracker.target == target)
        {
            return (&self.symbol_trackers[index], false);
        }
        let id = self.next_symbol_tracker_id;
        self.next_symbol_tracker_id += 1;
        self.symbol_trackers.push(SymbolTracker {
            id,
            enabled: true,
            target,
        });
        (
            self.symbol_trackers
                .last()
                .expect("tracker was just inserted"),
            true,
        )
    }

    pub fn symbol_trackers(&self) -> &[SymbolTracker] {
        &self.symbol_trackers
    }

    #[inline]
    pub fn has_symbol_trackers(&self) -> bool {
        self.symbol_trackers.iter().any(|tracker| tracker.enabled)
    }

    pub fn remove_symbol_tracker(&mut self, id: usize) -> bool {
        let old_len = self.symbol_trackers.len();
        self.symbol_trackers.retain(|tracker| tracker.id != id);
        self.symbol_trackers.len() != old_len
    }

    pub fn clear_symbol_trackers(&mut self) {
        self.symbol_trackers.clear();
    }

    pub fn add_stop_hook(&mut self, command: String) -> &StopHook {
        let id = self.next_stop_hook_id;
        self.next_stop_hook_id += 1;
        self.stop_hooks.push(StopHook {
            id,
            enabled: true,
            command,
        });
        self.stop_hooks.last().expect("stop hook was just inserted")
    }

    pub fn stop_hooks(&self) -> &[StopHook] {
        &self.stop_hooks
    }

    pub fn stop_hook_commands(&self) -> Vec<(usize, String)> {
        self.stop_hooks
            .iter()
            .filter(|hook| hook.enabled)
            .map(|hook| (hook.id, hook.command.clone()))
            .collect()
    }

    pub fn remove_stop_hook(&mut self, id: usize) -> bool {
        let old_len = self.stop_hooks.len();
        self.stop_hooks.retain(|hook| hook.id != id);
        self.stop_hooks.len() != old_len
    }

    pub fn clear_stop_hooks(&mut self) {
        self.stop_hooks.clear();
    }
}

pub fn format_span_snippet(
    sf: &crate::wqdb::data::SourceFile,
    start_byte: usize,
    end_byte: usize,
) -> String {
    format_span_snippet_with_color_mode(sf, start_byte, end_byte, ColorMode::Auto)
}

pub fn format_span_snippet_with_color_mode(
    sf: &crate::wqdb::data::SourceFile,
    start_byte: usize,
    end_byte: usize,
    color_mode: ColorMode,
) -> String {
    let end_byte = end_byte.max(start_byte.saturating_add(1));
    let (l, _) = sf.line_col(start_byte);
    let line_text = sf.line_text(l);
    let (line_start, line_end_raw) = sf.line_bounds(l);
    let line_end = line_end_raw.min(line_start + line_text.len());
    let span_start = start_byte.max(line_start).min(line_end);
    let span_end = end_byte.min(line_end);
    let rel_start = span_start.saturating_sub(line_start);
    let rel_end = span_end.saturating_sub(line_start);
    let use_color = color_mode.should_colorize();
    let mut out = String::new();

    let prefix_gut = format!("  {l} ->");

    let line_display = if use_color {
        let before_span = &line_text[..rel_start];
        let span_text = &line_text[rel_start..rel_end];
        let after_span = &line_text[rel_end..];
        let mut line_display = String::new();
        line_display.push_str(before_span);
        line_display.push_str(&paint(
            span_text,
            TextStyle::new().fg(AnsiColor::Green).bold().underline(),
            color_mode,
        ));
        line_display.push_str(after_span);
        line_display
    } else {
        line_text.to_string()
    };

    out.push_str(&format!("{prefix_gut} {line_display}"));
    out.push('\n');

    if !use_color {
        let prefix_len = prefix_gut.chars().count();
        let source_column = prefix_len + 1;
        let underline_start =
            source_column + terminal_text_width(&line_text[..rel_start], source_column);
        out.push_str(&" ".repeat(underline_start));
        let width = terminal_text_width(&line_text[rel_start..rel_end], underline_start).max(1);
        let underline = "~".repeat(width);
        out.push_str(&underline);
    }
    out
}

fn terminal_text_width(text: &str, start_column: usize) -> usize {
    const TAB_STOP: usize = 8;
    let mut column = start_column;
    for ch in text.chars() {
        if ch == '\t' {
            column += TAB_STOP - column % TAB_STOP;
        } else {
            column += ch.width().unwrap_or(0);
        }
    }
    column - start_column
}

pub fn format_frame(di: &DebugInfo, loc: CodeLoc, name: &str, is_last: bool) -> String {
    format_frame_with_color_mode(di, loc, name, is_last, ColorMode::Auto)
}

pub fn format_frame_with_color_mode(
    di: &DebugInfo,
    loc: CodeLoc,
    name: &str,
    is_last: bool,
    color_mode: ColorMode,
) -> String {
    let meta = match di.chunk_opt(loc.chunk) {
        Some(m) => m,
        None => {
            let bullet = if is_last { '*' } else { '+' };
            let gutter = paint(
                "| ",
                TextStyle::new().fg(AnsiColor::BrightYellow),
                color_mode,
            );
            let mut out = paint(
                &format!("{bullet} at {name}, ?:?:?"),
                TextStyle::new().fg(AnsiColor::BrightYellow),
                color_mode,
            );
            out.push('\n');
            out.push_str(&gutter);
            out.push_str(&format!("{:>4} -> ?", "?"));
            return out;
        }
    };
    let mut span = meta.line_table.span_at(loc.pc);
    if span.file_id == u32::MAX {
        span = meta.line_table.context_span_at(loc.pc);
    }
    let bullet = if is_last { '*' } else { '+' };
    let gutter = paint(
        "| ",
        TextStyle::new().fg(AnsiColor::BrightYellow),
        color_mode,
    );
    let use_inline_underline = color_mode.should_colorize();
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
        if span.file_id == u32::MAX {
            if let Some(sf) = di.file(meta.file_id) {
                let mut out = paint(
                    &format!("{bullet} at {name}, {}:?:?", sf.path),
                    TextStyle::new().fg(AnsiColor::BrightYellow),
                    color_mode,
                );
                out.push('\n');
                out.push_str(&gutter);
                out.push_str(&format!("{:>4} -> ?", "?"));
                return out;
            }
            let mut out = paint(
                &format!("{bullet} at {name}, ?:?:?"),
                TextStyle::new().fg(AnsiColor::BrightYellow),
                color_mode,
            );
            out.push('\n');
            out.push_str(&gutter);
            out.push_str(&format!("{:>4} -> ?", "?"));
            return out;
        }
    }
    if let Some(sf) = di.file(span.file_id) {
        let start_byte = span.start;
        let end_byte = span.end.max(start_byte.saturating_add(1));
        let (l, c) = sf.line_col(start_byte);
        let end_lookup = end_byte
            .saturating_sub(1)
            .min(sf.text.len().saturating_sub(1));
        let (end_line, _) = sf.line_col(end_lookup);
        let mut out = paint(
            &format!("{bullet} at {name}, {}:{}:{}", sf.path, l, c),
            TextStyle::new().fg(AnsiColor::BrightYellow),
            color_mode,
        );
        out.push('\n');
        // Clamp 1-based line numbers correctly within [1, total]
        let total = sf.line_starts.len();
        let lo_ln = if l > 1 { l - 1 } else { 1 };
        let hi_ln = if end_line < total {
            end_line + 1
        } else {
            total
        };
        for ln in lo_ln..=hi_ln {
            let line_text = sf.line_text(ln);
            let (line_start, line_end_raw) = sf.line_bounds(ln);
            let line_end = line_end_raw.min(line_start + line_text.len());
            let span_start = start_byte.max(line_start);
            let span_end = end_byte.min(line_end);
            let has_overlap = span_start < line_end && span_end > line_start;
            out.push_str(&gutter);
            if ln == l {
                out.push_str(&paint(
                    &format!("{:>4} -> ", ln),
                    TextStyle::new().fg(AnsiColor::Green).bold(),
                    color_mode,
                ));
            } else {
                out.push_str(&format!("{:>4}    ", ln));
            }
            if has_overlap && use_inline_underline {
                let rel_start = span_start - line_start;
                let rel_end = span_end - line_start;
                out.push_str(&line_text[..rel_start]);
                out.push_str(&paint(
                    &line_text[rel_start..rel_end],
                    TextStyle::new().fg(AnsiColor::Green).bold().underline(),
                    color_mode,
                ));
                out.push_str(&line_text[rel_end..]);
            } else {
                out.push_str(line_text);
            }
            out.push('\n');
            if has_overlap && !use_inline_underline {
                let pointer_start = if ln == l { c.saturating_sub(1) } else { 0 };
                let pointer_width = sf.text[span_start..span_end].chars().count().max(1);
                out.push_str(&gutter);
                out.push_str("        ");
                out.push_str(&paint(
                    &format!("{}{}", " ".repeat(pointer_start), "~".repeat(pointer_width)),
                    TextStyle::new().fg(AnsiColor::Green).bold(),
                    color_mode,
                ));
                out.push('\n');
            }
        }
        if out.ends_with('\n') {
            out.truncate(out.len() - 1);
        }
        out
    } else {
        // Unknown file but known chunk
        let mut out = paint(
            &format!("{bullet} at {}, chunk {:?}, pc {}", name, meta.id, loc.pc),
            TextStyle::new().fg(AnsiColor::Yellow),
            color_mode,
        );
        out.push('\n');
        out.push_str(&gutter);
        out.push_str(&format!("{:>4} -> ?", "?"));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wqdb::build::apply_stmt_debug_exact_offs;
    use crate::wqdb::data::{ChunkId, LineTable, Span};
    use crate::wqdb::model::StepGranularity;

    fn granularity_debug_info() -> (DebugInfo, ChunkId) {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("wq[test]", "a:1;b:2\nc:3\n");
        let chunk = di.new_chunk("<script>", file_id, 5);
        let table = &mut di.chunk_mut(chunk).line_table;
        table.set_stmt_mark(
            0,
            Span {
                file_id,
                start: 0,
                end: 3,
            },
        );
        table.set_exact_span(
            1,
            Span {
                file_id,
                start: 0,
                end: 3,
            },
        );
        table.set_stmt_mark(
            2,
            Span {
                file_id,
                start: 4,
                end: 7,
            },
        );
        table.set_stmt_mark(
            3,
            Span {
                file_id,
                start: 8,
                end: 11,
            },
        );
        (di, chunk)
    }

    #[test]
    fn expression_granularity_is_the_compatible_default() {
        assert_eq!(Wqdb::default().granularity(), StepGranularity::Expr);
    }

    #[test]
    fn line_granularity_coalesces_expressions_on_the_same_line() {
        let (di, chunk) = granularity_debug_info();
        let mut wqdb = Wqdb {
            enabled: true,
            ..Wqdb::default()
        };
        wqdb.set_granularity(StepGranularity::Line);
        wqdb.note_pause(CodeLoc { chunk, pc: 0 });
        wqdb.req_in(0);

        assert!(!wqdb.should_pause_at(&di, CodeLoc { chunk, pc: 2 }, 0));
        assert!(wqdb.should_pause_at(&di, CodeLoc { chunk, pc: 3 }, 0));
    }

    #[test]
    fn line_step_in_stops_in_a_deeper_frame_on_the_same_source_line() {
        let (di, chunk) = granularity_debug_info();
        let mut wqdb = Wqdb {
            enabled: true,
            ..Wqdb::default()
        };
        wqdb.set_granularity(StepGranularity::Line);
        wqdb.note_pause(CodeLoc { chunk, pc: 0 });
        wqdb.req_in(0);

        assert!(wqdb.should_pause_at(&di, CodeLoc { chunk, pc: 2 }, 1));
    }

    #[test]
    fn expression_granularity_stops_at_each_expression_and_on_revisit() {
        let (di, chunk) = granularity_debug_info();
        let mut wqdb = Wqdb {
            enabled: true,
            ..Wqdb::default()
        };
        wqdb.note_pause(CodeLoc { chunk, pc: 0 });
        wqdb.req_in(0);

        assert!(!wqdb.should_pause_at(&di, CodeLoc { chunk, pc: 1 }, 0));
        assert!(wqdb.should_pause_at(&di, CodeLoc { chunk, pc: 2 }, 0));
        assert!(wqdb.should_pause_at(&di, CodeLoc { chunk, pc: 0 }, 0));
    }

    #[test]
    fn instruction_granularity_stops_at_every_instruction() {
        let (di, chunk) = granularity_debug_info();
        let mut wqdb = Wqdb {
            enabled: true,
            ..Wqdb::default()
        };
        wqdb.set_granularity(StepGranularity::Inst);
        wqdb.note_pause(CodeLoc { chunk, pc: 0 });
        wqdb.req_in(0);

        assert!(wqdb.should_pause_at(&di, CodeLoc { chunk, pc: 1 }, 0));
    }

    #[test]
    fn step_over_applies_depth_to_instruction_granularity() {
        let (di, chunk) = granularity_debug_info();
        let mut wqdb = Wqdb {
            enabled: true,
            ..Wqdb::default()
        };
        wqdb.set_granularity(StepGranularity::Inst);
        wqdb.note_pause(CodeLoc { chunk, pc: 0 });
        wqdb.req_over(0);

        assert!(!wqdb.should_pause_at(&di, CodeLoc { chunk, pc: 1 }, 1));
        assert!(wqdb.should_pause_at(&di, CodeLoc { chunk, pc: 1 }, 0));
    }

    #[test]
    fn format_frame_underlines_exact_columns() {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("wq[test]", "a:{\n  1/0\n}\n");
        let chunk = di.new_chunk("a", file_id, 3);
        {
            let table = &mut di.chunk_mut(chunk).line_table;
            let spans = apply_stmt_debug_exact_offs(
                table,
                file_id,
                &[None, Some((6, 9)), None],
                &[crate::vm::inst::DebugStmtMark {
                    pc: 1,
                    start: 6,
                    end: 9,
                }],
                0,
            );
            di.chunk_mut(chunk).note_debug_spans(spans.0, spans.1);
        }

        let rendered = format_frame_with_color_mode(
            &di,
            CodeLoc { chunk, pc: 1 },
            "a",
            true,
            ColorMode::Always,
        );

        assert!(rendered.contains("2 -> "), "frame was: {rendered}");
        assert!(rendered.contains("1/0"), "frame was: {rendered}");
        assert!(!rendered.ends_with('\n'), "frame was: {rendered:?}");
    }

    #[test]
    fn span_snippet_plain_underline_aligns_with_source() {
        let source = crate::wqdb::data::SourceFile::new(0, "wq[test]", "abc\n");

        let rendered = format_span_snippet_with_color_mode(&source, 1, 2, ColorMode::Never);

        assert_eq!(rendered, "  1 -> abc\n        ~");
    }

    #[test]
    fn span_snippet_plain_underline_counts_unicode_columns() {
        let source = crate::wqdb::data::SourceFile::new(0, "wq[test]", "αβγ\n");

        let rendered = format_span_snippet_with_color_mode(&source, 2, 4, ColorMode::Never);

        assert_eq!(rendered, "  1 -> αβγ\n        ~");
    }

    #[test]
    fn span_snippet_plain_underline_uses_terminal_width() {
        let source = crate::wqdb::data::SourceFile::new(0, "wq[test]", "界a\n");

        let rendered = format_span_snippet_with_color_mode(&source, 3, 4, ColorMode::Never);

        assert_eq!(rendered, "  1 -> 界a\n         ~");
    }

    #[test]
    fn span_snippet_clamps_multiline_span_to_the_first_displayed_line() {
        let source = crate::wqdb::data::SourceFile::new(0, "wq[test]", "a:1\nb:2\n");

        let rendered = format_span_snippet_with_color_mode(&source, 0, 7, ColorMode::Never);

        assert_eq!(rendered, "  1 -> a:1\n       ~~~");
    }

    #[test]
    fn span_snippet_plain_underline_expands_tabs_from_the_source_column() {
        let source = crate::wqdb::data::SourceFile::new(0, "wq[test]", "  \ta:1\n");

        let rendered = format_span_snippet_with_color_mode(&source, 3, 6, ColorMode::Never);

        assert_eq!(rendered, "  1 ->   \ta:1\n                ~~~");
    }

    #[test]
    fn source_file_reports_display_columns_for_debugger_cards() {
        let source = crate::wqdb::data::SourceFile::new(0, "wq[test]", "α界\tz\n");

        assert_eq!(source.display_line_col(6), (1, 9));
    }

    #[test]
    fn context_span_prefers_previous_exact_span_before_older_stmt_span() {
        let mut table = LineTable::default();
        table.set_stmt_mark(
            1,
            Span {
                file_id: 1,
                start: 0,
                end: 4,
            },
        );
        table.set_exact_span(
            3,
            Span {
                file_id: 1,
                start: 8,
                end: 10,
            },
        );

        assert_eq!(
            table.context_span_at(4),
            Span {
                file_id: 1,
                start: 8,
                end: 10,
            }
        );
    }

    #[test]
    fn pause_breakpoints_do_not_pause_before_executing_pause_instruction() {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("wq[@p]", "@p\n");
        let chunk = di.new_chunk("<script>", file_id, 2);
        di.chunk_mut(chunk).line_table.set_stmt_mark(
            1,
            Span {
                file_id,
                start: 0,
                end: 2,
            },
        );

        let loc = CodeLoc { chunk, pc: 1 };
        let mut wqdb = Wqdb {
            enabled: true,
            ..Wqdb::default()
        };

        wqdb.pause_break_enabled(loc);
        assert!(!wqdb.should_pause_at(&di, loc, 0));

        let persistent = CodeLoc { chunk, pc: 0 };
        wqdb.ensure_breakpoint(persistent, BreakpointKind::Persistent);
        assert!(wqdb.should_pause_at(&di, persistent, 1));
    }
}
