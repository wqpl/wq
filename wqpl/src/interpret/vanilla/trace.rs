use crate::ast::BinaryOperator;
use crate::highlight::Highlighter;
use crate::value::{Excerpt, Value};
use crate::vm::Vm;
use crate::vm::inst::{BinaryOpData, Instruction, Operand};
use crate::vm::trace::TraceRecord;
use crate::wqerror::WqError;

fn format_debug_expr(source: &str, start: usize, end: usize) -> String {
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

fn render_debug_line(vm: &Vm, pc: usize, value: &Value, highlighter: &Highlighter) -> String {
    let chunk = vm.expect_current_chunk();
    let meta = vm.debug_info.expect_chunk(chunk);
    let span = meta.line_table.span_at(pc);
    let file = vm.debug_info.file(span.file_id);

    if let Some(file) = file {
        let start = span.start;
        let end = span.end;
        let (line, col) = file.line_col(start);
        let expr = format_debug_expr(file.text(), start, end);
        let expr = highlighter.highlight_ansi(&expr);
        let metadata = format_value_metadata(value.debug_kind().as_str(), value.strong_count());
        format!(
            "[{path}:{line}:{col}]\n{expr} = {value} ({metadata})",
            path = file.path(),
        )
    } else {
        let metadata = format_value_metadata(value.debug_kind().as_str(), value.strong_count());
        format!("[{}] {} ({metadata})", meta.name, value)
    }
}

fn format_value_metadata(debug_kind: &str, strong_count: Option<usize>) -> String {
    strong_count.map_or_else(
        || debug_kind.to_string(),
        |count| format!("{debug_kind}, strong={count}"),
    )
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
    let Some(chunk_id) = vm.current_chunk_id() else {
        return;
    };
    let span = vm.debug_info.expect_chunk(chunk_id).line_table.span_at(pc);
    if span.file_id == u32::MAX {
        return;
    }
    let call_depth = u32::try_from(vm.call_depth()).unwrap_or(u32::MAX);
    let debug_kind = value.debug_kind().as_str();
    let value_excerpt = value.excerpt();
    let strong_count = value.strong_count();
    vm.trace_buf.push(TraceRecord {
        span,
        value_excerpt,
        debug_kind,
        strong_count,
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

/// Render a `@d` flush with the original header and any captured probes.
///
/// Probe output uses a reverse tree, starting with the outer expression.
/// The root is already shown in the header, so only its children are rendered.
pub(super) fn render_trace_line(
    vm: &Vm,
    debug_pc: usize,
    final_value: &Value,
    records: &[TraceRecord],
) -> String {
    let highlighter = Highlighter::new();
    let head = render_debug_line(vm, debug_pc, final_value, &highlighter);
    if records.is_empty() {
        return head;
    }
    let mut roots = build_trace_tree(records);
    // Inline the children of a single root to avoid duplicating the header.
    // Keep childless roots so the user can see what ran.
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
    let chunk = vm.expect_current_chunk();
    let meta = vm.debug_info.expect_chunk(chunk);
    let span = meta.line_table.span_at(debug_pc);
    let Some(file) = vm.debug_info.file(span.file_id) else {
        return false;
    };
    if rec.span.file_id != span.file_id {
        return false;
    }

    let debug_expr = format_debug_expr(file.text(), span.start, span.end);
    let Some(operand) = debug_operand_text(&debug_expr) else {
        return false;
    };
    let root = format_debug_expr(file.text(), rec.span.start, rec.span.end);
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
    for node in nodes {
        out.push('\n');
        out.push_str(prefix);
        out.push_str("- ");
        out.push_str(&format_trace_node(vm, &node.record, highlighter));

        let mut next_prefix = String::with_capacity(prefix.len() + 2);
        next_prefix.push_str(prefix);
        next_prefix.push_str("  ");

        render_children(vm, &node.children, &next_prefix, out, highlighter);
    }
}

fn format_trace_node(vm: &Vm, rec: &TraceRecord, highlighter: &Highlighter) -> String {
    let file = vm.debug_info().file(rec.span.file_id);
    let expr = match file {
        Some(f) => {
            let expr = format_debug_expr(f.text(), rec.span.start, rec.span.end);
            highlighter.highlight_ansi(&expr)
        }
        None => "<expr>".to_string(),
    };
    let metadata = format_value_metadata(rec.debug_kind, rec.strong_count);
    format!("{} = {} ({metadata})", expr, rec.value_excerpt)
}

/// Attach source context from the current PC to a `WqError`, if debug info is
/// available.
pub(crate) fn attach_pc_source_ctx(vm: &Vm, pc: usize, err: WqError) -> WqError {
    if err.source_ctx.is_some() || !vm.debug_artifacts_enabled() {
        return err;
    }
    let Some(chunk) = vm.current_chunk else {
        return err;
    };
    let Some(source) = vm
        .debug_info
        .resolve_location(crate::debug::data::CodeLoc { chunk, pc })
        .and_then(|resolved| resolved.source)
    else {
        return err;
    };
    err.span(Some((source.span.start, source.span.end)))
        .source_ctx(source.source.to_string(), source.path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::data::Span;

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

    fn has_highlighted_visible_line(s: &str, visible_line: &str) -> bool {
        s.lines()
            .any(|line| line.contains('\x1b') && strip_ansi(line) == visible_line)
    }

    fn has_ansi_underline(s: &str) -> bool {
        let mut rest = s;
        while let Some(start) = rest.find("\x1b[") {
            rest = &rest[start + 2..];
            let Some(end) = rest.find('m') else {
                return false;
            };
            if rest[..end].split(';').any(|code| code == "4") {
                return true;
            }
            rest = &rest[end + 1..];
        }
        false
    }

    fn vm_with_local_names(inst: Instruction, names: &[&str]) -> Vm {
        let mut vm = Vm::new(vec![inst]);
        let file_id = vm.debug_info.new_file("<trace-test>", "");
        let chunk = vm.debug_info.new_chunk("<trace-test>", file_id, 1);
        vm.debug_info.expect_chunk_mut(chunk).local_names =
            Some(names.iter().map(|name| (*name).to_string()).collect());
        vm.current_chunk = Some(chunk);
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
        vm.current_chunk = Some(chunk);
        vm.debug_info
            .expect_chunk_mut(chunk)
            .line_table
            .set_exact_span(
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
                debug_kind: "int",
                strong_count: None,
                call_depth: 0,
            },
            TraceRecord {
                span: Span {
                    file_id,
                    start: 5,
                    end: 6,
                },
                value_excerpt: "2".to_string(),
                debug_kind: "int",
                strong_count: None,
                call_depth: 0,
            },
            TraceRecord {
                span: Span {
                    file_id,
                    start: 3,
                    end: 6,
                },
                value_excerpt: "3".to_string(),
                debug_kind: "int",
                strong_count: None,
                call_depth: 0,
            },
        ];

        let rendered = render_trace_line(&vm, 0, &Value::Int(3), &records);

        assert!(
            has_highlighted_visible_line(&rendered, "1+2 = 3 (int)"),
            "expected highlighted expression snippet, got: {rendered:?}"
        );
        assert!(
            has_highlighted_visible_line(&rendered, "  - 1 = 1 (int)"),
            "expected highlighted child trace snippet, got: {rendered:?}"
        );
        assert!(
            !has_ansi_underline(&rendered),
            "trace snippets should not be underlined, got: {rendered:?}"
        );
        let visible = strip_ansi(&rendered);
        assert!(
            visible.contains("[<trace-test>:1:4]\n1+2 = 3 (int)"),
            "visible header changed, got: {rendered:?}"
        );
        assert!(
            visible.contains("\n- 1+2 = 3 (int)\n  - 1 = 1 (int)\n  - 2 = 2 (int)"),
            "visible trace tree changed, got: {rendered:?}"
        );
    }

    #[test]
    fn render_trace_line_shows_arc_backing_strong_count() {
        let source = "@d (1;2;3)";
        let mut vm = Vm::new(vec![Instruction::Return]);
        let file_id = vm.debug_info.new_file("<trace-test>", source);
        let chunk = vm.debug_info.new_chunk("<trace-test>", file_id, 1);
        vm.current_chunk = Some(chunk);
        vm.debug_info
            .expect_chunk_mut(chunk)
            .line_table
            .set_exact_span(
                0,
                Span {
                    file_id,
                    start: 0,
                    end: source.len(),
                },
            );
        let value = Value::IntList(std::sync::Arc::new(vec![1, 2, 3]));

        let rendered = strip_ansi(&render_trace_line(&vm, 0, &value, &[]));

        assert!(
            rendered.contains("(int-list, strong=1)"),
            "expected Arc strong count in debug output, got: {rendered:?}"
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
