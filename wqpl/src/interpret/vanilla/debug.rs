use colored::Colorize;

use crate::value::{Excerpt, Value};
use crate::vm::Vm;
use crate::vm::trace::TraceRecord;
use crate::wqerror::WqError;

pub(super) fn format_debug_expr(source: &str, start: usize, end: usize) -> String {
    const LIMIT: usize = 80;

    let Some(slice) = source.get(start..end) else {
        return "<expr>".to_string();
    };

    let mut out = String::new();
    let mut last_was_ws = false;
    let mut truncated = false;

    for ch in slice.chars() {
        let ch = if ch.is_whitespace() { ' ' } else { ch };
        if ch == ' ' {
            if out.is_empty() || last_was_ws {
                continue;
            }
            last_was_ws = true;
        } else {
            last_was_ws = false;
        }

        if out.len() >= LIMIT {
            truncated = true;
            break;
        }
        out.push(ch);
    }

    while out.ends_with(' ') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("<expr>");
    }
    if truncated {
        out.push_str("...");
    }
    out
}

pub(super) fn render_debug_line(vm: &Vm, pc: usize, value: &Value) -> String {
    let chunk = vm.current_chunk;
    let meta = vm.debug_info.chunk(chunk);
    let span = meta.line_table.span_at(pc);
    let file = vm.debug_info.file(span.file_id);

    if let Some(file) = file {
        let start = span.start as usize;
        let end = span.end as usize;
        let (line, col) = file.line_col(start);
        let expr = format_debug_expr(file.text.as_ref(), start, end);
        format!(
            "[{path}:{line}:{col}] {expr} = {value} ({type})",
            path = file.path,
            expr = expr.underline(),
            type = value.type_name()
        )
    } else {
        format!("[{}] {}", meta.name, value)
    }
}

/// Append a [`TraceRecord`] for the instruction at `pc`.
///
/// Called by the interpreter loop after any [`is_trace_interesting`] op has
/// finished, while `vm.trace_depth > 0`.  Reads `vm.stack.last()` as the
/// freshly-produced value, looks up the dbg span at `pc` against the current
/// chunk, and pushes a record onto `vm.trace_buf`.  Silently does nothing
/// when no debug span or stack value is available.
///
/// [`is_trace_interesting`]: crate::vm::inst::Instruction::is_trace_interesting
pub(super) fn record_trace_probe(vm: &mut Vm, pc: usize) {
    let Some(value) = vm.stack.last() else {
        return;
    };
    let chunk_id = vm.current_chunk_id();
    let span = vm.debug_info.chunk(chunk_id).line_table.span_at(pc);
    if span.file_id == u32::MAX {
        return;
    }
    let call_depth = vm.call_depth() as u32;
    let type_name = value.type_name();
    let value_excerpt = value.excerpt();
    vm.trace_buf.push(TraceRecord {
        span,
        value_excerpt,
        type_name,
        call_depth,
    });
}

struct TraceNode {
    record: TraceRecord,
    children: Vec<TraceNode>,
}

/// Rebuild the parent/child tree of a flat post-order trace slice.
///
/// A subtree on the stack becomes a child of the incoming record when either:
/// - its `call_depth` is strictly greater (sub-records produced inside a callee
///   always belong to that callee's call-site record), or
/// - it shares the incoming record's `call_depth` *and* the incoming record's
///   source span strictly contains the subtree's span (intra-frame nesting).
fn build_trace_tree(records: &[TraceRecord]) -> Vec<TraceNode> {
    let mut stack: Vec<TraceNode> = Vec::new();
    for rec in records {
        let mut children: Vec<TraceNode> = Vec::new();
        while let Some(top) = stack.last() {
            let t = &top.record;
            let is_child = if t.call_depth > rec.call_depth {
                true
            } else if t.call_depth == rec.call_depth
                && t.span.file_id == rec.span.file_id
                && rec.span.start <= t.span.start
                && rec.span.end >= t.span.end
            {
                rec.span.start < t.span.start || rec.span.end > t.span.end
            } else {
                false
            };
            if is_child {
                children.push(stack.pop().expect("just peeked"));
            } else {
                break;
            }
        }
        children.reverse();
        stack.push(TraceNode {
            record: rec.clone(),
            children,
        });
    }
    stack
}

/// Render a `@d` flush: the original single-line header plus, if any probes
/// were captured, a reverse tree (outer expression first, then nested
/// sub-results).  The outermost root record's information is already in the
/// header, so its own line is not repeated — we descend straight into its
/// children.
pub(super) fn render_trace_line(
    vm: &Vm,
    debug_pc: usize,
    final_value: &Value,
    records: &[TraceRecord],
) -> String {
    let head = render_debug_line(vm, debug_pc, final_value);
    if records.is_empty() {
        return head;
    }
    let mut roots = build_trace_tree(records);
    // A single root with children is the @d expression's outermost evaluation
    // probe — its `expr = value` would duplicate the header line, so inline
    // its children directly under the header.  Single childless roots (e.g.
    // when short-circuit evaluation skipped the outermost op) are kept so the
    // user can still see what actually ran.
    let children: Vec<TraceNode> = if roots.len() == 1 && !roots[0].children.is_empty() {
        roots.pop().expect("len == 1").children
    } else {
        roots
    };
    if children.is_empty() {
        return head;
    }
    let mut out = head;
    render_children(vm, &children, "", &mut out);
    out
}

fn render_children(vm: &Vm, nodes: &[TraceNode], prefix: &str, out: &mut String) {
    let last_i = nodes.len().saturating_sub(1);
    for (i, node) in nodes.iter().enumerate() {
        let is_last = i == last_i;
        let connector = if is_last { "└─ " } else { "├─ " };
        let child_prefix = if is_last { "   " } else { "│  " };
        out.push('\n');
        out.push_str(prefix);
        out.push_str(connector);
        out.push_str(&format_trace_node(vm, &node.record));
        let mut next_prefix = String::with_capacity(prefix.len() + child_prefix.len());
        next_prefix.push_str(prefix);
        next_prefix.push_str(child_prefix);
        render_children(vm, &node.children, &next_prefix, out);
    }
}

fn format_trace_node(vm: &Vm, rec: &TraceRecord) -> String {
    let file = vm.debug_info().file(rec.span.file_id);
    let expr = match file {
        Some(f) => format_debug_expr(
            f.text.as_ref(),
            rec.span.start as usize,
            rec.span.end as usize,
        ),
        None => "<expr>".to_string(),
    };
    format!(
        "{} = {} ({})",
        expr.underline(),
        rec.value_excerpt,
        rec.type_name
    )
}

/// Attach source context from the current PC to a `WqError`, if debug info is
/// available.
pub(super) fn attach_pc_source_ctx(vm: &Vm, pc: usize, err: WqError) -> WqError {
    let chunk = vm.current_chunk;
    let meta = vm.debug_info.chunk(chunk);
    let span = meta.line_table.span_at(pc);
    if span.file_id != u32::MAX
        && let Some(sf) = vm.debug_info.file(span.file_id)
    {
        return err
            .span(Some((span.start as usize, span.end as usize)))
            .source_ctx(sf.text.to_string(), sf.path.to_string());
    }
    err
}
