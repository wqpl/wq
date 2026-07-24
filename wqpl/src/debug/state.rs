use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[cfg(test)]
use unicode_width::UnicodeWidthChar as _;

use crate::debug::data::{CodeLoc, CrashFrame, DebugInfo, Span};
use crate::debug::model::{
    Breakpoint, BreakpointKind, SourceBreakpoint, StepGranularity, StepMode, SymbolTrackTarget,
    SymbolTracker,
};
use crate::debug::{DebugNotification, PauseReason};
use crate::session::dbglog::{DebugLog, DebugLogFlags};
use crate::style::{AnsiColor, ColorMode, TextStyle, paint};

pub(crate) struct DebugState {
    enabled: bool,
    breaks: HashMap<CodeLoc, Breakpoint>,
    source_breakpoints: HashMap<String, Vec<SourceBreakpoint>>,
    resolved_source_breakpoints: Vec<SourceBreakpoint>,
    next_break_id: usize,
    temps: HashSet<CodeLoc>,
    mode: StepMode,
    granularity: StepGranularity,
    base_depth: usize,
    last_pause: Option<CodeLoc>,
    current_pause: Option<CodeLoc>,
    symbol_trackers: Vec<SymbolTracker>,
    next_symbol_tracker_id: usize,
    notifications: Vec<DebugNotification>,
}

impl Default for DebugState {
    fn default() -> Self {
        Self {
            enabled: false,
            breaks: HashMap::new(),
            source_breakpoints: HashMap::new(),
            resolved_source_breakpoints: Vec::new(),
            next_break_id: 1,
            temps: HashSet::new(),
            mode: StepMode::None,
            granularity: StepGranularity::default(),
            base_depth: 0,
            last_pause: None,
            current_pause: None,
            symbol_trackers: Vec::new(),
            next_symbol_tracker_id: 1,
            notifications: Vec::new(),
        }
    }
}

impl DebugState {
    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub(crate) fn reset_execution_state(&mut self) {
        let enabled = self.enabled;
        *self = Self {
            enabled,
            ..Self::default()
        };
    }

    pub(crate) fn clear_mode(&mut self) {
        self.mode = StepMode::None;
        self.current_pause = None;
    }

    fn alloc_breakpoint_id(&mut self) -> usize {
        let id = self.next_break_id;
        self.next_break_id += 1;
        id
    }

    fn alloc_breakpoint(&mut self, kind: BreakpointKind) -> Breakpoint {
        Breakpoint {
            id: self.alloc_breakpoint_id(),
            enabled: true,
            kind,
        }
    }

    pub(crate) fn ensure_breakpoint(
        &mut self,
        loc: CodeLoc,
        kind: BreakpointKind,
    ) -> &mut Breakpoint {
        if !self.breaks.contains_key(&loc) {
            let bp = self.alloc_breakpoint(kind);
            self.breaks.insert(loc, bp);
        }
        self.breaks
            .get_mut(&loc)
            .expect("breakpoint must exist after insertion")
    }

    pub(crate) fn clear_breakpoint(&mut self, loc: CodeLoc) {
        self.breaks.remove(&loc);
    }

    pub(crate) fn replace_source_breakpoints(
        &mut self,
        debug_info: &DebugInfo,
        source_path: &str,
        lines: &[usize],
    ) -> Vec<SourceBreakpoint> {
        if let Some(previous) = self.source_breakpoints.remove(source_path) {
            let previous_ids = previous
                .into_iter()
                .map(|breakpoint| breakpoint.id)
                .collect::<HashSet<_>>();
            self.resolved_source_breakpoints
                .retain(|breakpoint| !previous_ids.contains(&breakpoint.id));
        }

        let mut breakpoints = Vec::new();
        for &requested_line in lines {
            if breakpoints
                .iter()
                .any(|breakpoint: &SourceBreakpoint| breakpoint.requested_line == requested_line)
            {
                continue;
            }
            breakpoints.push(SourceBreakpoint {
                id: self.alloc_breakpoint_id(),
                source_path: source_path.to_string(),
                requested_line,
                location: None,
            });
        }
        if !breakpoints.is_empty() {
            self.source_breakpoints
                .insert(source_path.to_string(), breakpoints);
            self.resolve_source_breakpoints_inner(debug_info, false);
        }

        let Some(breakpoints) = self.source_breakpoints.get(source_path) else {
            return Vec::new();
        };
        lines
            .iter()
            .filter_map(|line| {
                breakpoints
                    .iter()
                    .find(|breakpoint| breakpoint.requested_line == *line)
                    .cloned()
            })
            .collect()
    }

    pub(crate) fn resolve_source_breakpoints(&mut self, debug_info: &DebugInfo) {
        self.resolve_source_breakpoints_inner(debug_info, true);
    }

    fn resolve_source_breakpoints_inner(&mut self, debug_info: &DebugInfo, notify: bool) {
        let resolutions = self
            .source_breakpoints
            .values()
            .flatten()
            .filter(|breakpoint| breakpoint.location.is_none())
            .filter_map(|breakpoint| {
                resolve_source_line(
                    debug_info,
                    &breakpoint.source_path,
                    breakpoint.requested_line,
                )
                .map(|location| (breakpoint.id, location))
            })
            .collect::<Vec<_>>();

        for (id, location) in resolutions {
            let Some(breakpoint) = self
                .source_breakpoints
                .values_mut()
                .flatten()
                .find(|breakpoint| breakpoint.id == id)
            else {
                continue;
            };
            breakpoint.location = Some(location);
            if notify {
                self.resolved_source_breakpoints.push(breakpoint.clone());
            }
        }
    }

    pub(crate) fn take_resolved_source_breakpoints(&mut self) -> Vec<SourceBreakpoint> {
        std::mem::take(&mut self.resolved_source_breakpoints)
    }

    pub(crate) fn toggle_breakpoint_at(&mut self, loc: CodeLoc) -> bool {
        if let Some(breakpoint) = self.breaks.get_mut(&loc) {
            breakpoint.enabled = !breakpoint.enabled;
            breakpoint.enabled
        } else {
            self.ensure_breakpoint(loc, BreakpointKind::Persistent);
            true
        }
    }

    pub(crate) fn toggle_breakpoint_by_id(&mut self, id: usize) -> Option<bool> {
        let breakpoint = self
            .breaks
            .values_mut()
            .find(|breakpoint| breakpoint.id == id)?;
        breakpoint.enabled = !breakpoint.enabled;
        Some(breakpoint.enabled)
    }

    pub(crate) fn toggle_all_breakpoints(&mut self) -> bool {
        let new_state = self.breaks.values().any(|breakpoint| !breakpoint.enabled);
        for breakpoint in self.breaks.values_mut() {
            breakpoint.enabled = new_state;
        }
        new_state
    }

    pub(crate) fn breakpoints(&self) -> Vec<(usize, bool, CodeLoc)> {
        let mut breakpoints = self
            .breaks
            .iter()
            .map(|(location, breakpoint)| (breakpoint.id, breakpoint.enabled, *location))
            .collect::<Vec<_>>();
        breakpoints.sort_by_key(|(id, _, _)| *id);
        breakpoints
    }

    pub(crate) fn explicit_pause_id(&mut self, loc: CodeLoc) -> Option<usize> {
        let breakpoint = self.ensure_breakpoint(loc, BreakpointKind::Pause);
        breakpoint.enabled.then_some(breakpoint.id)
    }

    #[inline]
    pub(crate) fn pause_reason_at(
        &self,
        di: &DebugInfo,
        here: CodeLoc,
        call_depth: usize,
        debug_log: Option<&DebugLog>,
    ) -> Option<PauseReason> {
        if !self.enabled {
            return None;
        }
        if self.last_pause.is_none() && here.pc == 0 && call_depth == 0 {
            return Some(PauseReason::Entry);
        }
        if let Some(bp) = self.breaks.get(&here)
            && bp.enabled
            && bp.kind != BreakpointKind::Pause
        {
            if let Some(debug_log) = debug_log.filter(|log| log.enabled(DebugLogFlags::WQDB)) {
                debug_log.emit_line(format!(
                    "[wqdb]: pausing at persistent breakpoint {}: chunk {:?} pc {}",
                    bp.id, here.chunk, here.pc
                ));
            }
            return Some(PauseReason::Breakpoint { id: bp.id });
        }
        if let Some(breakpoint) = self
            .source_breakpoints
            .values()
            .flatten()
            .find(|breakpoint| breakpoint.location == Some(here))
        {
            return Some(PauseReason::Breakpoint { id: breakpoint.id });
        }
        if self.temps.contains(&here) {
            if let Some(debug_log) = debug_log.filter(|log| log.enabled(DebugLogFlags::WQDB)) {
                debug_log.emit_line(format!(
                    "[wqdb]: pausing at temp breakpoint: chunk {:?} pc {}",
                    here.chunk, here.pc
                ));
            }
            return Some(PauseReason::TemporaryBreakpoint);
        }
        let meta = di.get_chunk(here.chunk)?;
        let is_stmt = meta.line_table.is_stmt(here.pc);
        let is_step_point = match self.granularity {
            StepGranularity::Line => {
                is_stmt && !self.is_same_line_as_last_pause(di, here, call_depth)
            }
            StepGranularity::Expr => is_stmt,
            StepGranularity::Inst => true,
        };
        let should_pause = match self.mode {
            StepMode::None => false,
            StepMode::In => is_step_point,
            StepMode::Over => call_depth <= self.base_depth && is_step_point,
            StepMode::Out => call_depth < self.base_depth && is_step_point,
        };
        should_pause.then_some(PauseReason::Step)
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
        let meta = di.get_chunk(loc.chunk)?;
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

    pub(crate) fn note_pause(&mut self, loc: CodeLoc) {
        self.last_pause = Some(loc);
        self.current_pause = Some(loc);
        self.temps.clear();
        // Don't clear step mode here - let the stepping methods manage mode
        // transitions
    }

    pub(crate) fn req_in(&mut self, depth: usize) {
        self.mode = StepMode::In;
        self.base_depth = depth;
        self.current_pause = None;
    }

    pub(crate) fn req_over(&mut self, depth: usize) {
        self.mode = StepMode::Over;
        self.base_depth = depth;
        self.current_pause = None;
    }

    pub(crate) fn req_out(&mut self, depth: usize) {
        self.mode = StepMode::Out;
        self.base_depth = depth;
        self.current_pause = None;
    }

    pub(crate) fn pause_loc(&self) -> Option<CodeLoc> {
        self.current_pause
    }

    pub(crate) fn add_temp_break(&mut self, loc: CodeLoc, debug_log: Option<&DebugLog>) {
        if let Some(debug_log) = debug_log.filter(|log| log.enabled(DebugLogFlags::WQDB)) {
            debug_log.emit_line(format!(
                "[wqdb]: adding temp break at chunk {:?} pc {}",
                loc.chunk, loc.pc
            ));
        }
        self.temps.insert(loc);
    }

    pub(crate) fn granularity(&self) -> StepGranularity {
        self.granularity
    }

    pub(crate) fn set_granularity(&mut self, granularity: StepGranularity) {
        self.granularity = granularity;
    }

    pub(crate) fn ensure_symbol_tracker(
        &mut self,
        target: SymbolTrackTarget,
    ) -> (&SymbolTracker, bool) {
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

    pub(crate) fn symbol_trackers(&self) -> &[SymbolTracker] {
        &self.symbol_trackers
    }

    #[inline]
    pub(crate) fn has_symbol_trackers(&self) -> bool {
        self.symbol_trackers.iter().any(|tracker| tracker.enabled)
    }

    pub(crate) fn remove_symbol_tracker(&mut self, id: usize) -> bool {
        let old_len = self.symbol_trackers.len();
        self.symbol_trackers.retain(|tracker| tracker.id != id);
        self.symbol_trackers.len() != old_len
    }

    pub(crate) fn clear_symbol_trackers(&mut self) {
        self.symbol_trackers.clear();
    }

    pub(crate) fn push_notification(&mut self, notification: DebugNotification) {
        self.notifications.push(notification);
    }

    pub(crate) fn take_notifications(&mut self) -> Vec<DebugNotification> {
        std::mem::take(&mut self.notifications)
    }
}

fn resolve_source_line(
    debug_info: &DebugInfo,
    source_path: &str,
    requested_line: usize,
) -> Option<CodeLoc> {
    let mut file_ids = debug_info.file_ids_by_path(source_path);
    file_ids.sort_unstable();
    file_ids.into_iter().find_map(|file_id| {
        debug_info
            .resolve_line(file_id, requested_line)
            .first()
            .copied()
    })
}

#[cfg(test)]
fn format_span_snippet(
    sf: &crate::debug::data::SourceFile,
    start_byte: usize,
    end_byte: usize,
    color_mode: ColorMode,
) -> String {
    let start_byte = sf.clamp_byte_offset(start_byte);
    let mut end_byte = sf.clamp_byte_offset(end_byte.max(start_byte.saturating_add(1)));
    if end_byte <= start_byte {
        end_byte = sf.text()[start_byte..]
            .chars()
            .next()
            .map_or(start_byte, |ch| start_byte + ch.len_utf8());
    }
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

#[cfg(test)]
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

pub fn format_frame(
    di: &DebugInfo,
    loc: CodeLoc,
    name: &str,
    is_last: bool,
    color_mode: ColorMode,
) -> String {
    let meta = match di.get_chunk(loc.chunk).filter(|meta| loc.pc < meta.len) {
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
                    &format!("{bullet} at {name}, {}:?:?", sf.path()),
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
        let start_byte = sf.clamp_byte_offset(span.start);
        let mut end_byte = sf.clamp_byte_offset(span.end.max(start_byte.saturating_add(1)));
        if end_byte <= start_byte {
            end_byte = sf.text()[start_byte..]
                .chars()
                .next()
                .map_or(start_byte, |ch| start_byte + ch.len_utf8());
        }
        let (l, c) = sf.line_col(start_byte);
        let end_lookup = end_byte
            .saturating_sub(1)
            .min(sf.text().len().saturating_sub(1));
        let (end_line, _) = sf.line_col(end_lookup);
        let mut out = paint(
            &format!("{bullet} at {name}, {}:{}:{}", sf.path(), l, c),
            TextStyle::new().fg(AnsiColor::BrightYellow),
            color_mode,
        );
        out.push('\n');
        // Clamp 1-based line numbers correctly within [1, total]
        let total = sf.line_count();
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
                let marker = if line_text.is_empty() {
                    format!("{:>4} ->", ln)
                } else {
                    format!("{:>4} -> ", ln)
                };
                out.push_str(&paint(
                    &marker,
                    TextStyle::new().fg(AnsiColor::Green).bold(),
                    color_mode,
                ));
            } else if line_text.is_empty() {
                out.push_str(&format!("{:>4}", ln));
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
                let pointer_width = sf.text()[span_start..span_end].chars().count().max(1);
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

pub fn format_crash_frame(frame: &CrashFrame, is_current: bool, color_mode: ColorMode) -> String {
    match frame {
        CrashFrame::TailCallsOmitted => {
            let bullet = if is_current { '*' } else { '+' };
            paint(
                &format!("{bullet} at {}", frame.function()),
                TextStyle::new().fg(AnsiColor::BrightYellow),
                color_mode,
            )
        }
        CrashFrame::Located {
            function,
            location,
            source: None,
            ..
        } => {
            let bullet = if is_current { '*' } else { '+' };
            paint(
                &format!("{bullet} at {function}, ?:?:? (pc {})", location.pc),
                TextStyle::new().fg(AnsiColor::BrightYellow),
                color_mode,
            )
        }
        CrashFrame::Located {
            function,
            location,
            source: Some(source),
            ..
        } => {
            let mut debug_info = DebugInfo::default();
            let file_id = debug_info.new_file(Arc::clone(&source.path), Arc::clone(&source.source));
            let chunk =
                debug_info.new_chunk(Arc::clone(function), file_id, location.pc.saturating_add(1));
            if !debug_info.set_exact_span(
                CodeLoc {
                    chunk,
                    pc: location.pc,
                },
                Span {
                    file_id,
                    start: source.span.start,
                    end: source.span.end,
                },
            ) {
                let bullet = if is_current { '*' } else { '+' };
                return paint(
                    &format!("{bullet} at {function}, ?:?:? (pc {})", location.pc),
                    TextStyle::new().fg(AnsiColor::BrightYellow),
                    color_mode,
                );
            }
            format_frame(
                &debug_info,
                CodeLoc {
                    chunk,
                    pc: location.pc,
                },
                function,
                is_current,
                color_mode,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::build::apply_stmt_debug_exact_offs;
    use crate::debug::data::{ChunkId, LineTable, Span};
    use crate::debug::model::StepGranularity;

    fn granularity_debug_info() -> (DebugInfo, ChunkId) {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("wq[test]", "a:1;b:2\nc:3\n");
        let chunk = di.new_chunk("<script>", file_id, 5);
        let table = &mut di.expect_chunk_mut(chunk).line_table;
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
        assert_eq!(DebugState::default().granularity(), StepGranularity::Expr);
    }

    #[test]
    fn line_granularity_coalesces_expressions_on_the_same_line() {
        let (di, chunk) = granularity_debug_info();
        let mut wqdb = DebugState {
            enabled: true,
            ..DebugState::default()
        };
        wqdb.set_granularity(StepGranularity::Line);
        wqdb.note_pause(CodeLoc { chunk, pc: 0 });
        wqdb.req_in(0);

        assert!(
            wqdb.pause_reason_at(&di, CodeLoc { chunk, pc: 2 }, 0, None)
                .is_none()
        );
        assert_eq!(
            wqdb.pause_reason_at(&di, CodeLoc { chunk, pc: 3 }, 0, None),
            Some(PauseReason::Step)
        );
    }

    #[test]
    fn line_step_in_stops_in_a_deeper_frame_on_the_same_source_line() {
        let (di, chunk) = granularity_debug_info();
        let mut wqdb = DebugState {
            enabled: true,
            ..DebugState::default()
        };
        wqdb.set_granularity(StepGranularity::Line);
        wqdb.note_pause(CodeLoc { chunk, pc: 0 });
        wqdb.req_in(0);

        assert_eq!(
            wqdb.pause_reason_at(&di, CodeLoc { chunk, pc: 2 }, 1, None),
            Some(PauseReason::Step)
        );
    }

    #[test]
    fn expression_granularity_stops_at_each_expression_and_on_revisit() {
        let (di, chunk) = granularity_debug_info();
        let mut wqdb = DebugState {
            enabled: true,
            ..DebugState::default()
        };
        wqdb.note_pause(CodeLoc { chunk, pc: 0 });
        wqdb.req_in(0);

        assert!(
            wqdb.pause_reason_at(&di, CodeLoc { chunk, pc: 1 }, 0, None)
                .is_none()
        );
        assert_eq!(
            wqdb.pause_reason_at(&di, CodeLoc { chunk, pc: 2 }, 0, None),
            Some(PauseReason::Step)
        );
        assert_eq!(
            wqdb.pause_reason_at(&di, CodeLoc { chunk, pc: 0 }, 0, None),
            Some(PauseReason::Step)
        );
    }

    #[test]
    fn instruction_granularity_stops_at_every_instruction() {
        let (di, chunk) = granularity_debug_info();
        let mut wqdb = DebugState {
            enabled: true,
            ..DebugState::default()
        };
        wqdb.set_granularity(StepGranularity::Inst);
        wqdb.note_pause(CodeLoc { chunk, pc: 0 });
        wqdb.req_in(0);

        assert_eq!(
            wqdb.pause_reason_at(&di, CodeLoc { chunk, pc: 1 }, 0, None),
            Some(PauseReason::Step)
        );
    }

    #[test]
    fn step_over_applies_depth_to_instruction_granularity() {
        let (di, chunk) = granularity_debug_info();
        let mut wqdb = DebugState {
            enabled: true,
            ..DebugState::default()
        };
        wqdb.set_granularity(StepGranularity::Inst);
        wqdb.note_pause(CodeLoc { chunk, pc: 0 });
        wqdb.req_over(0);

        assert!(
            wqdb.pause_reason_at(&di, CodeLoc { chunk, pc: 1 }, 1, None)
                .is_none()
        );
        assert_eq!(
            wqdb.pause_reason_at(&di, CodeLoc { chunk, pc: 1 }, 0, None),
            Some(PauseReason::Step)
        );
    }

    #[test]
    fn format_frame_underlines_exact_columns() {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("wq[test]", "a:{\n  1/0\n}\n");
        let chunk = di.new_chunk("a", file_id, 3);
        {
            let table = &mut di.expect_chunk_mut(chunk).line_table;
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
                None,
            );
            di.expect_chunk_mut(chunk)
                .note_debug_spans(spans.0, spans.1);
        }

        let rendered = format_frame(&di, CodeLoc { chunk, pc: 1 }, "a", true, ColorMode::Always);

        assert!(rendered.contains("2 -> "), "frame was: {rendered}");
        assert!(rendered.contains("1/0"), "frame was: {rendered}");
        assert!(!rendered.ends_with('\n'), "frame was: {rendered:?}");
    }

    #[test]
    fn format_frame_does_not_pad_blank_context_lines() {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("wq[test]", "x\n\n1/0\n\n");
        let chunk = di.new_chunk("<script>", file_id, 1);
        di.expect_chunk_mut(chunk).line_table.set_exact_span(
            0,
            Span {
                file_id,
                start: 3,
                end: 6,
            },
        );

        let rendered = format_frame(
            &di,
            CodeLoc { chunk, pc: 0 },
            "<script>",
            true,
            ColorMode::Never,
        );

        assert!(
            rendered.lines().all(|line| !line.ends_with(' ')),
            "frame was: {rendered:?}"
        );
    }

    #[test]
    fn span_snippet_plain_underline_aligns_with_source() {
        let source = crate::debug::data::SourceFile::new(0, "wq[test]", "abc\n");

        let rendered = format_span_snippet(&source, 1, 2, ColorMode::Never);

        assert_eq!(rendered, "  1 -> abc\n        ~");
    }

    #[test]
    fn span_snippet_plain_underline_counts_unicode_columns() {
        let source = crate::debug::data::SourceFile::new(0, "wq[test]", "αβγ\n");

        let rendered = format_span_snippet(&source, 2, 4, ColorMode::Never);

        assert_eq!(rendered, "  1 -> αβγ\n        ~");
    }

    #[test]
    fn span_snippet_clamps_malformed_unicode_offsets() {
        let source = crate::debug::data::SourceFile::new(0, "wq[test]", "aéz\n");

        let rendered = format_span_snippet(&source, 2, 2, ColorMode::Never);

        assert_eq!(rendered, "  1 -> aéz\n        ~");
    }

    #[test]
    fn span_snippet_plain_underline_uses_terminal_width() {
        let source = crate::debug::data::SourceFile::new(0, "wq[test]", "界a\n");

        let rendered = format_span_snippet(&source, 3, 4, ColorMode::Never);

        assert_eq!(rendered, "  1 -> 界a\n         ~");
    }

    #[test]
    fn span_snippet_clamps_multiline_span_to_the_first_displayed_line() {
        let source = crate::debug::data::SourceFile::new(0, "wq[test]", "a:1\nb:2\n");

        let rendered = format_span_snippet(&source, 0, 7, ColorMode::Never);

        assert_eq!(rendered, "  1 -> a:1\n       ~~~");
    }

    #[test]
    fn span_snippet_plain_underline_expands_tabs_from_the_source_column() {
        let source = crate::debug::data::SourceFile::new(0, "wq[test]", "  \ta:1\n");

        let rendered = format_span_snippet(&source, 3, 6, ColorMode::Never);

        assert_eq!(rendered, "  1 ->   \ta:1\n                ~~~");
    }

    #[test]
    fn source_file_reports_display_columns_for_debugger_cards() {
        let source = crate::debug::data::SourceFile::new(0, "wq[test]", "α界\tz\n");

        assert_eq!(source.display_line_col(6), (1, 9));
    }

    #[test]
    fn format_frame_rejects_an_out_of_range_program_counter() {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("wq[test]", "1/0");
        let chunk = di.new_chunk("<script>", file_id, 1);
        di.expect_chunk_mut(chunk).line_table.set_exact_span(
            0,
            Span {
                file_id,
                start: 0,
                end: 3,
            },
        );

        let rendered = format_frame(
            &di,
            CodeLoc { chunk, pc: 99 },
            "<script>",
            true,
            ColorMode::Never,
        );

        assert!(rendered.contains("?:?:?"), "frame was: {rendered}");
        assert!(!rendered.contains("1/0"), "frame was: {rendered}");
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
        di.expect_chunk_mut(chunk).line_table.set_stmt_mark(
            1,
            Span {
                file_id,
                start: 0,
                end: 2,
            },
        );

        let loc = CodeLoc { chunk, pc: 1 };
        let mut wqdb = DebugState {
            enabled: true,
            ..DebugState::default()
        };

        assert!(wqdb.explicit_pause_id(loc).is_some());
        assert!(wqdb.pause_reason_at(&di, loc, 0, None).is_none());

        let persistent = CodeLoc { chunk, pc: 0 };
        let id = wqdb
            .ensure_breakpoint(persistent, BreakpointKind::Persistent)
            .id;
        assert_eq!(
            wqdb.pause_reason_at(&di, persistent, 1, None),
            Some(PauseReason::Breakpoint { id })
        );
    }

    #[test]
    fn temporary_breakpoint_wins_over_disabled_persistent_breakpoint_at_same_location() {
        let location = CodeLoc {
            chunk: ChunkId(2),
            pc: 3,
        };
        let mut wqdb = DebugState {
            enabled: true,
            ..DebugState::default()
        };
        wqdb.ensure_breakpoint(location, BreakpointKind::Persistent);
        assert!(!wqdb.toggle_breakpoint_at(location));
        wqdb.add_temp_break(location, None);

        assert_eq!(
            wqdb.pause_reason_at(&DebugInfo::default(), location, 1, None),
            Some(PauseReason::TemporaryBreakpoint)
        );
    }

    #[test]
    fn temporary_breakpoint_wins_over_pause_marker_at_same_location() {
        let location = CodeLoc {
            chunk: ChunkId(2),
            pc: 3,
        };
        let mut wqdb = DebugState {
            enabled: true,
            ..DebugState::default()
        };
        assert!(wqdb.explicit_pause_id(location).is_some());
        wqdb.add_temp_break(location, None);

        assert_eq!(
            wqdb.pause_reason_at(&DebugInfo::default(), location, 1, None),
            Some(PauseReason::TemporaryBreakpoint)
        );
    }

    #[test]
    fn reset_execution_state_preserves_enabled_state() {
        let loc = CodeLoc {
            chunk: ChunkId(7),
            pc: 11,
        };
        let mut wqdb = DebugState {
            enabled: true,
            ..DebugState::default()
        };
        wqdb.ensure_breakpoint(loc, BreakpointKind::Persistent);
        wqdb.add_temp_break(loc, None);
        wqdb.set_granularity(StepGranularity::Inst);
        wqdb.req_over(4);
        wqdb.note_pause(loc);
        wqdb.ensure_symbol_tracker(SymbolTrackTarget::Global {
            name: "value".to_string(),
        });

        wqdb.reset_execution_state();

        assert!(wqdb.enabled);
        assert!(wqdb.breaks.is_empty());
        assert!(wqdb.temps.is_empty());
        assert_eq!(wqdb.mode, StepMode::None);
        assert_eq!(wqdb.granularity, StepGranularity::Expr);
        assert_eq!(wqdb.base_depth, 0);
        assert_eq!(wqdb.last_pause, None);
        assert_eq!(wqdb.current_pause, None);
        assert!(wqdb.symbol_trackers.is_empty());
        assert_eq!(wqdb.next_symbol_tracker_id, 1);
        assert_eq!(wqdb.next_break_id, 1);
    }
}
