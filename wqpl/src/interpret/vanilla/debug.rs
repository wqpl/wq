use crate::astnode::BinaryOperator;
use crate::highlight::Highlighter;
use crate::value::{Excerpt, Value};
use crate::vm::Vm;
use crate::vm::inst::{BinaryOpData, Instruction, Operand};
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
    let highlighter = Highlighter::new();
    render_debug_line_with_highlighter(vm, pc, value, &highlighter)
}

fn render_debug_line_with_highlighter(
    vm: &Vm,
    pc: usize,
    value: &Value,
    highlighter: &Highlighter,
) -> String {
    let chunk = vm.current_chunk;
    let meta = vm.debug_info.chunk(chunk);
    let span = meta.line_table.span_at(pc);
    let file = vm.debug_info.file(span.file_id);

    if let Some(file) = file {
        let start = span.start as usize;
        let end = span.end as usize;
        let (line, col) = file.line_col(start);
        let expr = format_debug_expr(file.text.as_ref(), start, end);
        let expr = highlighter.highlight_ansi(&expr);
        format!(
            "[{path}:{line}:{col}]\n{expr} = {value} ({type})",
            path = file.path,
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
    if is_synthetic_n_loop_probe(vm, pc) {
        return;
    }

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

fn is_synthetic_n_loop_probe(vm: &Vm, pc: usize) -> bool {
    let Some(Instruction::BinaryOp(data)) = vm.instructions.get(pc) else {
        return false;
    };

    is_synthetic_n_loop_guard(vm, data) || is_synthetic_n_loop_increment(vm, data)
}

fn is_synthetic_n_loop_guard(vm: &Vm, data: &BinaryOpData) -> bool {
    data.op == BinaryOperator::Lt
        && trace_operand_name(vm, &data.left) == Some("_n")
        && trace_operand_name(vm, &data.right).is_some_and(is_n_loop_count_name)
}

fn is_synthetic_n_loop_increment(vm: &Vm, data: &BinaryOpData) -> bool {
    data.op == BinaryOperator::Add
        && trace_operand_name(vm, &data.left).is_some_and(is_n_loop_old_name)
        && operand_is_const_int(&data.right, 1)
}

fn trace_operand_name<'a>(vm: &'a Vm, operand: &'a Operand) -> Option<&'a str> {
    match operand {
        Operand::Local(slot) => vm.local_slot_name(usize::from(*slot)),
        Operand::Var(name) => Some(name.as_ref()),
        _ => None,
    }
}

fn operand_is_const_int(operand: &Operand, expected: i64) -> bool {
    matches!(
        operand,
        Operand::Const(value) if matches!(value.as_ref(), Value::Int(n) if *n == expected)
    )
}

fn is_n_loop_count_name(name: &str) -> bool {
    name.starts_with("--vm-n-loop-count-")
}

fn is_n_loop_old_name(name: &str) -> bool {
    name.starts_with("--vm-n-loop-old-")
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
/// header, so its own line is not repeated -- we descend straight into its
/// children.
pub(super) fn render_trace_line(
    vm: &Vm,
    debug_pc: usize,
    final_value: &Value,
    records: &[TraceRecord],
) -> String {
    let highlighter = Highlighter::new();
    let head = render_debug_line_with_highlighter(vm, debug_pc, final_value, &highlighter);
    if records.is_empty() {
        return head;
    }
    let mut roots = build_trace_tree(records);
    // A single root with children is the @d expression's outermost evaluation
    // probe -- its `expr = value` would duplicate the header line, so inline
    // its children directly under the header.  Single childless roots (e.g.
    // when short-circuit evaluation skipped the outermost op) are kept so the
    // user can still see what actually ran.
    let children: Vec<TraceNode> = if roots.len() == 1
        && !roots[0].children.is_empty()
        && root_matches_debug_operand(vm, debug_pc, &roots[0].record)
    {
        roots.pop().expect("len == 1").children
    } else {
        roots
    };
    if children.is_empty() {
        return head;
    }
    let mut out = head;
    render_children(vm, &children, "", &mut out, &highlighter);
    out
}

fn root_matches_debug_operand(vm: &Vm, debug_pc: usize, rec: &TraceRecord) -> bool {
    let chunk = vm.current_chunk;
    let meta = vm.debug_info.chunk(chunk);
    let span = meta.line_table.span_at(debug_pc);
    let Some(file) = vm.debug_info.file(span.file_id) else {
        return false;
    };
    if rec.span.file_id != span.file_id {
        return false;
    }

    let debug_expr = format_debug_expr(file.text.as_ref(), span.start as usize, span.end as usize);
    let Some(operand) = debug_operand_text(&debug_expr) else {
        return false;
    };
    let root = format_debug_expr(
        file.text.as_ref(),
        rec.span.start as usize,
        rec.span.end as usize,
    );
    root == operand
}

fn debug_operand_text(debug_expr: &str) -> Option<String> {
    let operand = debug_expr.strip_prefix("@d")?.trim();
    let stripped = strip_outer_parens(operand);
    Some(stripped.to_string())
}

fn strip_outer_parens(text: &str) -> &str {
    let text = text.trim();
    if !text.starts_with('(') || !text.ends_with(')') {
        return text;
    }

    let mut depth = 0usize;
    for (idx, ch) in text.char_indices() {
        match ch {
            '(' => depth = depth.saturating_add(1),
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && idx != text.len() - 1 {
                    return text;
                }
            }
            _ => {}
        }
    }

    text[1..text.len() - 1].trim()
}

fn render_children(
    vm: &Vm,
    nodes: &[TraceNode],
    prefix: &str,
    out: &mut String,
    highlighter: &Highlighter,
) {
    let last_i = nodes.len().saturating_sub(1);
    for (i, node) in nodes.iter().enumerate() {
        let is_last = i == last_i;
        let connector = if is_last { "└─ " } else { "├─ " };
        let child_prefix = if is_last { "   " } else { "│  " };
        out.push('\n');
        out.push_str(prefix);
        out.push_str(connector);
        out.push_str(&format_trace_node(vm, &node.record, highlighter));
        let mut next_prefix = String::with_capacity(prefix.len() + child_prefix.len());
        next_prefix.push_str(prefix);
        next_prefix.push_str(child_prefix);
        render_children(vm, &node.children, &next_prefix, out, highlighter);
    }
}

fn format_trace_node(vm: &Vm, rec: &TraceRecord, highlighter: &Highlighter) -> String {
    let file = vm.debug_info().file(rec.span.file_id);
    let expr = match file {
        Some(f) => {
            let expr = format_debug_expr(
                f.text.as_ref(),
                rec.span.start as usize,
                rec.span.end as usize,
            );
            highlighter.highlight_ansi(&expr)
        }
        None => "<expr>".to_string(),
    };
    format!("{} = {} ({})", expr, rec.value_excerpt, rec.type_name)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wqdb::data::Span;

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    fn vm_with_local_names(inst: Instruction, names: &[&str]) -> Vm {
        let mut vm = Vm::new(vec![inst]);
        let file_id = vm.debug_info.new_file("<trace-test>", "");
        let chunk = vm.debug_info.new_chunk("<trace-test>", file_id, 1);
        vm.debug_info.chunk_mut(chunk).local_names =
            Some(names.iter().map(|name| (*name).to_string()).collect());
        vm.current_chunk = chunk;
        vm
    }

    #[test]
    fn synthetic_n_loop_guard_is_not_traced() {
        let inst = Instruction::binary_op(BinaryOperator::Lt, Operand::Local(0), Operand::Local(1));
        let vm = vm_with_local_names(inst, &["_n", "--vm-n-loop-count-0"]);

        assert!(is_synthetic_n_loop_probe(&vm, 0));
    }

    #[test]
    fn top_level_synthetic_n_loop_guard_is_not_traced() {
        let inst = Instruction::binary_op(
            BinaryOperator::Lt,
            Operand::Var("_n".into()),
            Operand::Var("--vm-n-loop-count-0".into()),
        );
        let vm = Vm::new(vec![inst]);

        assert!(is_synthetic_n_loop_probe(&vm, 0));
    }

    #[test]
    fn synthetic_n_loop_increment_is_not_traced() {
        let inst = Instruction::binary_op(
            BinaryOperator::Add,
            Operand::Local(0),
            Operand::const_val(Value::Int(1)),
        );
        let vm = vm_with_local_names(inst, &["--vm-n-loop-old-0"]);

        assert!(is_synthetic_n_loop_probe(&vm, 0));
    }

    #[test]
    fn user_comparison_with_n_is_still_traced() {
        let inst = Instruction::binary_op(BinaryOperator::Lt, Operand::Local(0), Operand::Local(1));
        let vm = vm_with_local_names(inst, &["_n", "limit"]);

        assert!(!is_synthetic_n_loop_probe(&vm, 0));
    }

    #[test]
    fn render_trace_line_highlights_snippets_without_underlines() {
        let source = "@d 1+2";
        let mut vm = Vm::new(vec![Instruction::Return]);
        let file_id = vm.debug_info.new_file("<trace-test>", source);
        let chunk = vm.debug_info.new_chunk("<trace-test>", file_id, 1);
        vm.current_chunk = chunk;
        vm.debug_info.chunk_mut(chunk).line_table.set_exact_span(
            0,
            Span {
                file_id,
                start: 3,
                end: 6,
            },
        );

        let records = [
            TraceRecord {
                span: Span {
                    file_id,
                    start: 3,
                    end: 4,
                },
                value_excerpt: "1".to_string(),
                type_name: "int",
                call_depth: 0,
            },
            TraceRecord {
                span: Span {
                    file_id,
                    start: 5,
                    end: 6,
                },
                value_excerpt: "2".to_string(),
                type_name: "int",
                call_depth: 0,
            },
            TraceRecord {
                span: Span {
                    file_id,
                    start: 3,
                    end: 6,
                },
                value_excerpt: "3".to_string(),
                type_name: "int",
                call_depth: 0,
            },
        ];

        let rendered = render_trace_line(&vm, 0, &Value::Int(3), &records);

        assert!(
            rendered.contains("\x1b[38;5;220m1\x1b[0m\x1b[38;5;208m+\x1b[0m"),
            "expected highlighted expression snippet, got: {rendered:?}"
        );
        assert!(
            rendered.contains("├─ \x1b[38;5;220m1\x1b[0m = 1 (int)"),
            "expected highlighted child trace snippet, got: {rendered:?}"
        );
        assert!(
            !rendered.contains("\x1b[4m") && !rendered.contains("\x1b[4;"),
            "trace snippets should not be underlined, got: {rendered:?}"
        );
        assert!(
            strip_ansi(&rendered).contains("[<trace-test>:1:4]\n1+2 = 3 (int)"),
            "visible header changed, got: {rendered:?}"
        );
        assert!(
            strip_ansi(&rendered).contains("├─ 1 = 1 (int)"),
            "visible trace tree changed, got: {rendered:?}"
        );
    }

    #[test]
    fn debug_operand_text_strips_one_balanced_group() {
        assert_eq!(debug_operand_text("@d b").as_deref(), Some("b"));
        assert_eq!(debug_operand_text("@d (b*3)").as_deref(), Some("b*3"));
        assert_eq!(
            debug_operand_text("@d (A[a<0;10/b])").as_deref(),
            Some("A[a<0;10/b]")
        );
    }
}
