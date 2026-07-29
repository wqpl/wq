use super::{
    AstNode, FStringPart, Parameter, PipeKind, binary_op_display, bool_op_display, unary_op_display,
};
use crate::highlight::Highlighter as SyntaxHighlighter;
use crate::style::AnsiColor;
use crate::tree_pretty::{self, HeadStyle, Pretty};

fn atom_ident(s: &str) -> String {
    // bare if simple symbol, otherwise quoted like Rust's Debug string
    if crate::identifier::is_identifier(s) {
        s.to_string()
    } else {
        format!("{s:?}")
    }
}

impl std::fmt::Display for AstNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.sexpr_pretty())
    }
}

fn offset_to_line_col(src: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    for (i, c) in src.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line = line.saturating_add(1);
            col = 1;
        } else {
            col = col.saturating_add(1);
        }
    }
    (line, col)
}

fn extract_snippet(src: &str, start: usize, end: usize, max_len: usize) -> String {
    let start = start.min(src.len());
    let end = end.min(src.len());
    let mut s = match src.get(start..end) {
        Some(s) => s.to_string(),
        None => {
            // Byte indices don't fall on char boundaries (e.g. multi-byte UTF-8).
            // Find the nearest valid boundaries.
            let mut safe_start = start;
            while safe_start < src.len() && !src.is_char_boundary(safe_start) {
                safe_start += 1;
            }
            let mut safe_end = end.min(src.len());
            while safe_end > 0 && !src.is_char_boundary(safe_end) {
                safe_end -= 1;
            }
            src.get(safe_start..safe_end).unwrap_or("").to_string()
        }
    };
    if let Some(first) = s.lines().next() {
        s = first.to_string();
    }
    if s.chars().count() > max_len {
        s = s.chars().take(max_len).collect::<String>() + "...";
    }
    s
}

struct PrettySource<'a> {
    text: &'a str,
    highlighter: SyntaxHighlighter,
    semantic_ansi: bool,
}

fn fmt_span_note(src: &PrettySource<'_>, span: Option<(usize, usize)>) -> String {
    let (start, end) = match span {
        Some(s) => s,
        None => return String::new(),
    };
    let (sl, sc) = offset_to_line_col(src.text, start);
    let (el, ec) = offset_to_line_col(src.text, end);
    let snippet = extract_snippet(src.text, start, end, 20);
    let snippet = if src.semantic_ansi {
        src.highlighter.highlight_ansi_semantic(&snippet)
    } else {
        src.highlighter.highlight_ansi(&snippet)
    };
    format!(" [{sl}:{sc}-{el}:{ec}] {snippet}")
}

/// Color used for a given AST node type in the pretty printer.
fn node_color(node: &AstNode) -> AnsiColor {
    use AstNode::*;
    match node {
        Literal(..) | UnpackValue { .. } | FString { .. } => AnsiColor::Cyan,
        Variable(..) | OuterVariable(..) => AnsiColor::Blue,
        Assignment { .. }
        | OuterAssignment { .. }
        | IndexAssign { .. }
        | MutatingIndexAssign { .. } => AnsiColor::Red,
        Function { .. }
        | CallName { .. }
        | CallAnonymous { .. }
        | Postfix { .. }
        | Pipe { .. }
        | PipeTap { .. } => AnsiColor::Magenta,
        BinaryOp { .. }
        | LazyBool { .. }
        | UnaryOp { .. }
        | ComparisonChain { .. }
        | Range { .. }
        | Group { .. } => AnsiColor::Yellow,
        Conditional { .. }
        | ConditionalDot { .. }
        | ConditionalChain { .. }
        | WLoop { .. }
        | NLoop { .. }
        | Break(..)
        | Continue(..)
        | Return(..)
        | Try(..)
        | Import { .. } => AnsiColor::Green,
        Cat(..) | List(..) | Dict(..) | Block(..) | BlockExpr(..) => AnsiColor::White,
        Index { .. } | MutatingIndex { .. } => AnsiColor::BrightBlue,
        Debug { .. } | Pause { .. } | PipeInput | Ellipsis(..) | DictUnpackPattern(..) => {
            AnsiColor::BrightRed
        }
        UnpackAssignment { .. } => AnsiColor::Red,
        NamedArg { .. } => AnsiColor::BrightBlue,
        Error(..) => AnsiColor::BrightMagenta,
    }
}

fn pretty_leaf(text: &str, note: &str, color: AnsiColor) -> Pretty {
    tree_pretty::leaf(text, note, color)
}

fn pretty_group(depth: usize, head: String, children: Vec<Pretty>, color: AnsiColor) -> Pretty {
    tree_pretty::group(depth, head, children, color, HeadStyle::FirstWord, false)
}

fn pretty_container_group(
    depth: usize,
    head: String,
    children: Vec<Pretty>,
    color: AnsiColor,
) -> Pretty {
    let force_multi = children.len() > 1;
    tree_pretty::group(
        depth,
        head,
        children,
        color,
        HeadStyle::FirstWord,
        force_multi,
    )
}

impl AstNode {
    pub(crate) fn sexpr_pretty(&self) -> String {
        self.pretty_with_depth(0, None).multi
    }

    pub(crate) fn sexpr_pretty_with_source(&self, src: &str) -> String {
        let src = PrettySource {
            text: src,
            highlighter: SyntaxHighlighter::new(),
            semantic_ansi: false,
        };
        self.pretty_with_depth(0, Some(&src)).multi
    }

    pub(crate) fn sexpr_pretty_with_source_semantic_ansi(&self, src: &str) -> String {
        let src = PrettySource {
            text: src,
            highlighter: SyntaxHighlighter::new(),
            semantic_ansi: true,
        };
        self.pretty_with_depth(0, Some(&src)).multi
    }

    fn pretty_with_depth(&self, depth: usize, src: Option<&PrettySource<'_>>) -> Pretty {
        let note = src
            .map(|s| fmt_span_note(s, self.span()))
            .unwrap_or_default();
        let color = node_color(self);
        use AstNode::*;
        match self {
            Literal(v, _) => {
                let text = format!("LIT[{v:?}]").chars().take(1000).collect::<String>();
                pretty_leaf(&text, &note, color)
            }
            UnpackValue { slot, .. } => pretty_leaf(&format!("UNPACK-VALUE[{slot}]"), &note, color),
            Variable(name, _) => pretty_leaf(&format!("VAR[{}]", atom_ident(name)), &note, color),
            OuterVariable(name, _) => {
                pretty_leaf(&format!("OUTER-VAR[{}]", atom_ident(name)), &note, color)
            }
            Import { specifier, .. } => {
                pretty_leaf(&format!("IMPORT[{specifier:?}]"), &note, color)
            }
            UnaryOp {
                operator, operand, ..
            } => {
                let head = format!("UOP[{}]{note}", unary_op_display(operator));
                let child = operand.pretty_with_depth(depth + 1, src);
                pretty_group(depth, head, vec![child], color)
            }
            Group { expr, .. } => {
                let head = format!("GROUP{note}");
                let child = expr.pretty_with_depth(depth + 1, src);
                pretty_group(depth, head, vec![child], color)
            }
            BinaryOp {
                left,
                operator,
                right,
                ..
            } => {
                let head = format!("BOP[{}]{note}", binary_op_display(operator));
                let l = left.pretty_with_depth(depth + 1, src);
                let r = right.pretty_with_depth(depth + 1, src);
                pretty_group(depth, head, vec![l, r], color)
            }
            LazyBool {
                operator, operands, ..
            } => {
                let head = format!("LAZY-BOOL[{}]{note}", bool_op_display(operator));
                let children = operands
                    .iter()
                    .map(|operand| operand.pretty_with_depth(depth + 1, src))
                    .collect();
                pretty_group(depth, head, children, color)
            }
            ComparisonChain { first, rest, .. } => {
                let mut children = vec![first.pretty_with_depth(depth + 1, src)];
                for (op, node) in rest {
                    children.push(pretty_leaf(binary_op_display(op), "", AnsiColor::White));
                    children.push(node.pretty_with_depth(depth + 1, src));
                }
                let head = format!("CMP-CHAIN{note}");
                pretty_group(depth, head, children, color)
            }
            Range {
                start,
                end,
                step,
                inclusive,
                ..
            } => {
                let mut children = vec![
                    start.pretty_with_depth(depth + 1, src),
                    end.pretty_with_depth(depth + 1, src),
                ];
                if let Some(step) = step {
                    children.push(step.pretty_with_depth(depth + 1, src));
                }
                let head = format!("{}{note}", if *inclusive { "RANGE=" } else { "RANGE" });
                pretty_group(depth, head, children, color)
            }
            Assignment {
                name, op, value, ..
            } => {
                let head = if let Some(op) = op {
                    format!("ASSIGN[{}]{note}", binary_op_display(op))
                } else {
                    format!("ASSIGN{note}")
                };
                let name_p = pretty_leaf(&atom_ident(name), "", AnsiColor::White);
                let val_p = value.pretty_with_depth(depth + 1, src);
                pretty_group(depth, head, vec![name_p, val_p], color)
            }
            OuterAssignment {
                name, op, value, ..
            } => {
                let head = if let Some(op) = op {
                    format!("OUTER-ASSIGN[{}]{note}", binary_op_display(op))
                } else {
                    format!("OUTER-ASSIGN{note}")
                };
                let name_p = pretty_leaf(&atom_ident(name), "", AnsiColor::White);
                let val_p = value.pretty_with_depth(depth + 1, src);
                pretty_group(depth, head, vec![name_p, val_p], color)
            }
            Ellipsis(_) => pretty_leaf("...", &note, color),
            List(items, _) => {
                let children: Vec<Pretty> = items
                    .iter()
                    .map(|i| i.pretty_with_depth(depth + 1, src))
                    .collect();
                let head = format!("LIST{note}");
                pretty_container_group(depth, head, children, color)
            }
            Cat(items, _) => {
                let children: Vec<Pretty> = items
                    .iter()
                    .map(|i| i.pretty_with_depth(depth + 1, src))
                    .collect();
                let head = format!("CAT{note}");
                pretty_group(depth, head, children, color)
            }
            Dict(kvs, _) => {
                let mut children = Vec::with_capacity(kvs.len());
                for (k, v) in kvs {
                    let v_p = v.pretty_with_depth(depth + 2, src);
                    children.push(pretty_group(
                        depth + 1,
                        atom_ident(k),
                        vec![v_p],
                        AnsiColor::White,
                    ));
                }
                let head = format!("DICT{note}");
                pretty_container_group(depth, head, children, color)
            }
            Postfix {
                object,
                items,
                explicit_call,
                depth: depth_modifier,
                ..
            } => {
                let head = format!(
                    "{}{}{note}",
                    if *explicit_call {
                        "POSTFIX*"
                    } else {
                        "POSTFIX"
                    },
                    match depth_modifier {
                        Some(depth) => format!("@{depth}"),
                        None => String::new(),
                    }
                );
                let mut children = vec![object.pretty_with_depth(depth + 1, src)];
                children.extend(items.iter().map(|i| i.pretty_with_depth(depth + 1, src)));
                pretty_group(depth, head, children, color)
            }
            PipeInput => pretty_leaf("PIPE-IN", &note, color),
            Pipe {
                input,
                effect,
                kind,
                ..
            } => {
                let kind_str = match kind {
                    PipeKind::Pipe => "L-PIPE",
                    PipeKind::PipeDot => "L-PIPE-TAP",
                    PipeKind::PipePipe => "R-PIPE",
                    PipeKind::PipePipeDot => "R-PIPE-TAP",
                };
                let head = format!("{kind_str}{note}");
                let children = vec![
                    input.pretty_with_depth(depth + 1, src),
                    effect.pretty_with_depth(depth + 1, src),
                ];
                pretty_group(depth, head, children, color)
            }
            PipeTap { input, effect, .. } => {
                let head = format!("PIPE-TAP{note}");
                let children = vec![
                    input.pretty_with_depth(depth + 1, src),
                    effect.pretty_with_depth(depth + 1, src),
                ];
                pretty_group(depth, head, children, color)
            }
            CallName { name, args, .. } => {
                let head = format!("CALL-NAME[{}]{note}", atom_ident(name));
                let children: Vec<Pretty> = args
                    .iter()
                    .map(|a| a.pretty_with_depth(depth + 1, src))
                    .collect();
                pretty_group(depth, head, children, color)
            }
            CallAnonymous { object, args, .. } => {
                let obj = object.pretty_with_depth(depth + 1, src);
                let children: Vec<Pretty> = args
                    .iter()
                    .map(|a| a.pretty_with_depth(depth + 1, src))
                    .collect();
                pretty_group(depth, obj.flat, children, color)
            }
            NamedArg { name, value, .. } => {
                let head = format!("NAMED-ARG[`{name}]");
                let val_p = value.pretty_with_depth(depth + 1, src);
                pretty_group(depth, head, vec![val_p], color)
            }
            Index { object, index, .. } => {
                let head = format!("IDX{note}");
                let children = vec![
                    object.pretty_with_depth(depth + 1, src),
                    index.pretty_with_depth(depth + 1, src),
                ];
                pretty_group(depth, head, children, color)
            }
            IndexAssign {
                object,
                index,
                op,
                value,
                ..
            } => {
                let head = if let Some(op) = op {
                    format!("IDX-ASSIGN[{}]{note}", binary_op_display(op))
                } else {
                    format!("IDX-ASSIGN{note}")
                };
                let children = vec![
                    object.pretty_with_depth(depth + 1, src),
                    index.pretty_with_depth(depth + 1, src),
                    value.pretty_with_depth(depth + 1, src),
                ];
                pretty_group(depth, head, children, color)
            }
            MutatingIndex { object, index, .. } => {
                let head = format!("MUT-IDX{note}");
                let children = vec![
                    object.pretty_with_depth(depth + 1, src),
                    index.pretty_with_depth(depth + 1, src),
                ];
                pretty_group(depth, head, children, color)
            }
            MutatingIndexAssign {
                object,
                index,
                value,
                ..
            } => {
                let head = format!("MUT-IDX-ASSIGN{note}");
                let children = vec![
                    object.pretty_with_depth(depth + 1, src),
                    index.pretty_with_depth(depth + 1, src),
                    value.pretty_with_depth(depth + 1, src),
                ];
                pretty_group(depth, head, children, color)
            }
            Function {
                params,
                ref_capture,
                body,
                ..
            } => {
                let params_p = match params {
                    Some(ps) if !ps.is_empty() => {
                        let first = pretty_leaf(&atom_ident(ps[0].name()), "", AnsiColor::White);
                        let rest: Vec<Pretty> = ps[1..]
                            .iter()
                            .map(|p| match p {
                                Parameter::Pos { name, .. } => {
                                    pretty_leaf(&atom_ident(name), "", AnsiColor::White)
                                }
                                Parameter::Named {
                                    name,
                                    default: Some(_),
                                    ..
                                } => pretty_leaf(
                                    &format!("`{}:...", atom_ident(name)),
                                    "",
                                    AnsiColor::White,
                                ),
                                Parameter::Named {
                                    name,
                                    default: None,
                                    ..
                                } => pretty_leaf(
                                    &format!("`{}", atom_ident(name)),
                                    "",
                                    AnsiColor::White,
                                ),
                            })
                            .collect();
                        pretty_group(depth + 1, first.flat, rest, AnsiColor::White)
                    }
                    Some(_) => pretty_group(depth + 1, String::new(), vec![], AnsiColor::White),
                    None => {
                        pretty_group(depth + 1, "implicit".to_string(), vec![], AnsiColor::White)
                    }
                };
                let head = if *ref_capture {
                    format!("FN'{note}")
                } else {
                    format!("FN{note}")
                };
                let children = vec![params_p, body.pretty_with_depth(depth + 1, src)];
                pretty_group(depth, head, children, color)
            }
            Conditional {
                condition,
                true_branch,
                false_branch,
                ..
            } => {
                let mut children = vec![
                    condition.pretty_with_depth(depth + 1, src),
                    true_branch.pretty_with_depth(depth + 1, src),
                ];
                if let Some(fb) = false_branch {
                    children.push(fb.pretty_with_depth(depth + 1, src));
                }
                let head = format!("IF{note}");
                pretty_group(depth, head, children, color)
            }
            ConditionalDot {
                condition,
                true_branch,
                ..
            } => {
                let children = vec![
                    condition.pretty_with_depth(depth + 1, src),
                    true_branch.pretty_with_depth(depth + 1, src),
                ];
                let head = format!("IFDOT{note}");
                pretty_group(depth, head, children, color)
            }
            ConditionalChain {
                pairs,
                default_branch,
                ..
            } => {
                let mut children: Vec<Pretty> = pairs
                    .iter()
                    .flat_map(|(cond, branch)| {
                        vec![
                            cond.pretty_with_depth(depth + 1, src),
                            branch.pretty_with_depth(depth + 1, src),
                        ]
                    })
                    .collect();
                children.push(default_branch.pretty_with_depth(depth + 1, src));
                let head = format!("COND-CHAIN{note}");
                pretty_group(depth, head, children, color)
            }
            WLoop {
                condition, body, ..
            } => {
                let head = format!("W-LOOP{note}");
                let children = vec![
                    condition.pretty_with_depth(depth + 1, src),
                    body.pretty_with_depth(depth + 1, src),
                ];
                pretty_group(depth, head, children, color)
            }
            NLoop { count, body, .. } => {
                let head = format!("N-LOOP{note}");
                let children = vec![
                    count.pretty_with_depth(depth + 1, src),
                    body.pretty_with_depth(depth + 1, src),
                ];
                pretty_group(depth, head, children, color)
            }
            Break(_) => pretty_leaf("@b", &note, color),
            Continue(_) => pretty_leaf("@c", &note, color),
            Return(opt, _) => {
                let mut children = Vec::new();
                if let Some(v) = opt {
                    children.push(v.pretty_with_depth(depth + 1, src));
                }
                let head = format!("@r{note}");
                pretty_group(depth, head, children, color)
            }
            Debug { expr, .. } => {
                let head = format!("@d{note}");
                let child = expr.pretty_with_depth(depth + 1, src);
                pretty_group(depth, head, vec![child], color)
            }
            Pause { expr, .. } => {
                let mut children = Vec::new();
                if let Some(expr) = expr {
                    children.push(expr.pretty_with_depth(depth + 1, src));
                }
                let head = format!("@p{note}");
                pretty_group(depth, head, children, color)
            }
            Try(expr, _) => {
                let head = format!("@t{note}");
                let child = expr.pretty_with_depth(depth + 1, src);
                pretty_group(depth, head, vec![child], color)
            }
            Block(stmts, _) => {
                let children: Vec<Pretty> = stmts
                    .iter()
                    .map(|s| s.pretty_with_depth(depth + 1, src))
                    .collect();
                let head = format!("BLOCK{note}");
                pretty_container_group(depth, head, children, color)
            }
            BlockExpr(stmts, _) => {
                let children: Vec<Pretty> = stmts
                    .iter()
                    .map(|s| s.pretty_with_depth(depth + 1, src))
                    .collect();
                let head = format!("B{note}");
                pretty_container_group(depth, head, children, color)
            }
            UnpackAssignment { lhs, op, rhs, .. } => {
                let head = if let Some(op) = op {
                    format!("UNPACK-ASSIGN[{}]{note}", binary_op_display(op))
                } else {
                    format!("UNPACK-ASSIGN{note}")
                };
                let mut children: Vec<Pretty> = lhs
                    .iter()
                    .map(|n| n.pretty_with_depth(depth + 1, src))
                    .collect();
                children.push(rhs.pretty_with_depth(depth + 1, src));
                pretty_group(depth, head, children, color)
            }
            DictUnpackPattern(entries, _) => {
                let children = entries
                    .iter()
                    .map(|entry| {
                        let target = entry.target.pretty_with_depth(depth + 2, src);
                        pretty_group(
                            depth + 1,
                            format!("KEY(`{})", entry.key),
                            vec![target],
                            color,
                        )
                    })
                    .collect();
                pretty_container_group(depth, format!("DICT-PATTERN{note}"), children, color)
            }
            FString { parts, .. } => {
                let head = format!("FSTRING{note}");
                let children: Vec<Pretty> = parts
                    .iter()
                    .map(|p| match p {
                        FStringPart::Text(t) => pretty_leaf(&format!("TEXT({t:?})"), "", color),
                        FStringPart::Expr {
                            expr,
                            spec,
                            encoded_spec,
                            spec_exprs,
                        } => {
                            let mut label = expr.pretty_with_depth(depth + 1, src);
                            if let Some(sp) = spec {
                                label.multi.push_str(&format!(" spec={sp:?}"));
                            }
                            if let Some(enc) = encoded_spec {
                                label.multi.push_str(&format!(" enc={enc:?}"));
                            }
                            for se in spec_exprs {
                                label.multi.push(' ');
                                label
                                    .multi
                                    .push_str(&se.pretty_with_depth(depth + 1, src).multi);
                            }
                            label
                        }
                    })
                    .collect();
                pretty_group(depth, head, children, color)
            }

            Error(..) => pretty_leaf("ERROR", &note, color),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::{AstNode, BinaryOperator};
    use crate::value::Value;

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

    #[test]
    fn source_snippets_in_ast_notes_are_syntax_highlighted() {
        let ast = AstNode::BinaryOp {
            left: Box::new(AstNode::Literal(Value::Int(1), Some((0, 1)))),
            operator: BinaryOperator::Add,
            right: Box::new(AstNode::Literal(Value::Int(2), Some((2, 3)))),
            span: Some((0, 3)),
        };

        let pretty = ast.sexpr_pretty_with_source("1+2");

        assert!(
            pretty.contains(" [1:1-1:4] \x1b[38;5;220m1"),
            "expected root source note snippet to be ANSI highlighted, got: {pretty:?}"
        );
        assert!(
            strip_ansi(&pretty).contains(" [1:1-1:4] 1+2"),
            "visible source note text changed, got: {pretty:?}"
        );
    }

    #[test]
    fn ast_pretty_printer_uses_explicit_style_renderer() {
        let ast = AstNode::Literal(Value::Int(1), None);

        let pretty = ast.sexpr_pretty();

        assert_eq!(pretty, "\x1b[1;36mLIT[Int(1)]\x1b[0m");
        assert_eq!(strip_ansi(&pretty), "LIT[Int(1)]");
    }

    #[test]
    fn semantic_ansi_ast_uses_named_terminal_colors() {
        let ast = AstNode::Literal(Value::Int(1), Some((0, 1)));

        let pretty = ast.sexpr_pretty_with_source_semantic_ansi("1");

        assert_eq!(
            pretty,
            "\x1b[1;36mLIT[Int(1)]\x1b[0m [1:1-1:2] \x1b[33m1\x1b[0m"
        );
        assert!(!pretty.contains("\x1b[38;5;"));
    }
}
