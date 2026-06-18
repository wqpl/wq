use colored::{Color, Colorize};

use crate::highlight::Highlighter as SyntaxHighlighter;
use crate::value::Value;
use crate::wqerror::WqError;

/// Source span: `None` means synthetic (not from source text).
pub(crate) type AstSpan = Option<(usize, usize)>;

#[derive(Debug, Clone, PartialEq)]
pub enum Parameter {
    Pos {
        name: String,
        span: AstSpan,
    },
    Named {
        name: String,
        span: AstSpan,
        default: Option<Box<AstNode>>,
    },
}

impl Parameter {
    pub(crate) fn name(&self) -> &str {
        match self {
            Parameter::Pos { name, .. } | Parameter::Named { name, .. } => name,
        }
    }

    pub(crate) fn span(&self) -> AstSpan {
        match self {
            Parameter::Pos { span, .. } | Parameter::Named { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstNode {
    Error(WqError, AstSpan),
    Literal(Value, AstSpan),
    Variable(String, AstSpan),
    OuterVariable(String, AstSpan),
    BinaryOp {
        left: Box<AstNode>,
        operator: BinaryOperator,
        right: Box<AstNode>,
    },
    /// Chain of comparison operators like `a < b <= c`
    ComparisonChain {
        first: Box<AstNode>,
        rest: Vec<(BinaryOperator, AstNode)>,
    },
    UnaryOp {
        operator: UnaryOperator,
        operand: Box<AstNode>,
        span: AstSpan,
    },
    Group {
        expr: Box<AstNode>,
        span: AstSpan,
    },
    Range {
        start: Box<AstNode>,
        end: Box<AstNode>,
        step: Option<Box<AstNode>>,
        inclusive: bool,
    },
    Assignment {
        name: String,
        op: Option<BinaryOperator>,
        value: Box<AstNode>,
        span: AstSpan,
        name_span: AstSpan,
    },
    OuterAssignment {
        name: String,
        op: Option<BinaryOperator>,
        value: Box<AstNode>,
        span: AstSpan,
        name_span: AstSpan,
    },
    /// Unpack assignment like (x;y):rhs
    UnpackAssignment {
        lhs: Vec<AstNode>,
        op: Option<BinaryOperator>,
        rhs: Box<AstNode>,
        span: AstSpan,
    },
    /// Ellipsis (for unpack patterns)
    Ellipsis,
    /// List construction
    List(Vec<AstNode>),
    /// N-ary concatenation (comma-separated items)
    Cat(Vec<AstNode>),
    /// Dictionary construction
    Dict(Vec<(String, AstNode)>),
    /// Generic postfix expression
    Postfix {
        object: Box<AstNode>,
        items: Vec<AstNode>,
        explicit_call: bool,
        depth: Option<i64>,
        span: AstSpan,
    },
    /// Placeholder used while compiling tap-pipe effects.
    PipeInput,
    /// Pipe expression (not yet lowered).
    Pipe {
        input: Box<AstNode>,
        effect: Box<AstNode>,
        kind: PipeKind,
        span: AstSpan,
    },
    /// Pipe that runs an effect but yields the original input.
    PipeTap {
        input: Box<AstNode>,
        effect: Box<AstNode>,
        span: AstSpan,
    },
    /// Function call
    CallName {
        name: String,
        args: Vec<AstNode>,
        span: AstSpan,
        name_span: AstSpan,
    },
    CallAnonymous {
        object: Box<AstNode>,
        args: Vec<AstNode>,
        span: AstSpan,
    },
    /// Index access
    Index {
        object: Box<AstNode>,
        index: Box<AstNode>,
        span: AstSpan,
    },
    /// Index assignment like `a[1]:3`
    IndexAssign {
        object: Box<AstNode>,
        index: Box<AstNode>,
        op: Option<BinaryOperator>,
        value: Box<AstNode>,
        span: AstSpan,
    },
    /// Mutating index access: `x[!]` (pop), `x[!i]` (remove)
    MutatingIndex {
        object: Box<AstNode>,
        index: Box<AstNode>,
        span: AstSpan,
    },
    /// Mutating index assignment: `x[!]:v` (insert between), `x[!i]:v` (insert
    /// at)
    MutatingIndexAssign {
        object: Box<AstNode>,
        index: Box<AstNode>,
        value: Box<AstNode>,
        span: AstSpan,
    },
    /// Named argument at a call site: `name: expr
    NamedArg {
        name: String,
        value: Box<AstNode>,
        span: AstSpan,
    },
    /// Function def
    Function {
        params: Option<Vec<Parameter>>, // None for implicit params (x, y, z)
        ref_capture: bool,
        body: Box<AstNode>,
    },
    /// Conditional expression
    Conditional {
        condition: Box<AstNode>,
        true_branch: Box<AstNode>,
        false_branch: Option<Box<AstNode>>,
        span: AstSpan,
    },
    /// Conditional dot from $.[...] (no false branch)
    ConditionalDot {
        condition: Box<AstNode>,
        true_branch: Box<AstNode>,
        span: AstSpan,
    },
    /// Conditional chain from $$[...]
    ConditionalChain {
        pairs: Vec<(AstNode, AstNode)>,
        default_branch: Box<AstNode>,
        span: AstSpan,
    },
    WLoop {
        condition: Box<AstNode>,
        body: Box<AstNode>,
        span: AstSpan,
    },
    NLoop {
        count: Box<AstNode>,
        body: Box<AstNode>,
        span: AstSpan,
    },
    Break,
    Continue,
    Return(Option<Box<AstNode>>),
    Assert {
        expr: Box<AstNode>,
        span: AstSpan,
    },
    Debug {
        expr: Box<AstNode>,
        span: AstSpan,
    },
    Pause {
        expr: Option<Box<AstNode>>,
        span: AstSpan,
    },
    Try(Box<AstNode>),
    /// Sequence of statements
    Block(Vec<AstNode>),
    /// Block expression from B[...]
    BlockExpr(Vec<AstNode>, AstSpan),
    /// F-string literal (@f"...{expr}...")
    FString {
        parts: Vec<FStringPart>,
        span: AstSpan,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum FStringPart {
    Text(String),
    Expr {
        expr: AstNode,
        spec: Option<String>,
        encoded_spec: Option<String>,
        spec_exprs: Vec<AstNode>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeKind {
    Pipe,
    PipeDot,
    PipePipe,
    PipePipeDot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Power,
    PowerDot,
    Divide,
    DivideDot,
    Modulo,
    Matmul,

    Equal,
    EqualDot,
    NotEqual,
    NotEqualDot,
    Lt,
    Lte,
    Gt,
    Gte,
    BoolAnd, // &|
    BoolOr,  // \|
    Cat,     // , (augmented-assignment marker only; compiled to Instruction::Cat)
    BitAnd,
    BitOr,
    Shl,
    Shr,
    BitXor,
    FloorDiv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOperator {
    Negate,
    Count, // #
    Not,
}

fn atom_ident(s: &str) -> String {
    // bare if simple symbol, otherwise quoted like Rust's Debug string
    if s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        s.to_string()
    } else {
        format!("{s:?}")
    }
}

pub(crate) fn binary_op_display(op: &BinaryOperator) -> &'static str {
    use BinaryOperator::*;
    match op {
        Add => "+",
        Subtract => "-",
        Multiply => "*",
        Power => "^",
        PowerDot => "^.",
        Divide => "/",
        DivideDot => "/.",
        Modulo => "%",
        Matmul => "**",

        Equal => "=",
        EqualDot => "=.",
        NotEqual => "~",
        NotEqualDot => "~.",

        BoolAnd => "&|",
        BoolOr => r"\|",
        BitAnd => "&",
        BitOr => r"\",
        Shl => "<<",
        Shr => ">>",
        BitXor => r"^\",
        FloorDiv => "/%",
        Lt => "<",
        Lte => "<=",
        Gt => ">",
        Gte => ">=",
        Cat => ",",
    }
}

pub(crate) fn unary_op_display(op: &UnaryOperator) -> &'static str {
    use UnaryOperator::*;
    match op {
        Negate => "-",
        Count => "#",
        Not => "~",
    }
}

impl std::fmt::Display for AstNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.sexpr_pretty())
    }
}

// -----------------------------------------------------------------------
// Pretty printer with heuristic line breaking and optional source spans
// -----------------------------------------------------------------------

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
}

fn fmt_span_note(src: &PrettySource<'_>, span: Option<(usize, usize)>) -> String {
    let (start, end) = match span {
        Some(s) => s,
        None => return String::new(),
    };
    let (sl, sc) = offset_to_line_col(src.text, start);
    let (el, ec) = offset_to_line_col(src.text, end);
    let snippet = extract_snippet(src.text, start, end, 20);
    let snippet = src.highlighter.highlight_ansi(&snippet);
    format!(" [{sl}:{sc}-{el}:{ec}] {snippet}")
}

/// Simple pretty-document: holds a flat (single-line) form and a multi-line
/// form.
struct Pretty {
    flat: String,
    flat_len: usize,
    multi: String,
}

/// Strip ANSI escape sequences to get the visible width of a string.
fn visible_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            len += 1;
        }
    }
    len
}

/// Color used for a given AST node type in the pretty printer.
fn node_color(node: &AstNode) -> Color {
    use AstNode::*;
    match node {
        Literal(..) | FString { .. } => Color::Cyan,
        Variable(..) | OuterVariable(..) => Color::Blue,
        Assignment { .. }
        | OuterAssignment { .. }
        | IndexAssign { .. }
        | MutatingIndexAssign { .. } => Color::Red,
        Function { .. }
        | CallName { .. }
        | CallAnonymous { .. }
        | Postfix { .. }
        | Pipe { .. }
        | PipeTap { .. } => Color::Magenta,
        BinaryOp { .. } | UnaryOp { .. } | ComparisonChain { .. } | Range { .. } | Group { .. } => {
            Color::Yellow
        }
        Conditional { .. }
        | ConditionalDot { .. }
        | ConditionalChain { .. }
        | WLoop { .. }
        | NLoop { .. }
        | Break
        | Continue
        | Return(..)
        | Try(..) => Color::Green,
        Cat(..) | List(..) | Dict(..) | Block(..) | BlockExpr(..) => Color::White,
        Index { .. } | MutatingIndex { .. } => Color::BrightBlue,
        Assert { .. } | Debug { .. } | Pause { .. } | PipeInput | Ellipsis => Color::BrightRed,
        UnpackAssignment { .. } => Color::Red,
        NamedArg { .. } => Color::BrightBlue,
        Error(..) => Color::BrightMagenta,
    }
}

/// Budget shrinks as we go deeper so that deeply nested expressions break
/// earlier.
fn budget(depth: usize) -> usize {
    60usize.saturating_sub(depth * 2)
}

fn pretty_leaf(text: &str, note: &str, color: Color) -> Pretty {
    let body = text.color(color).bold().to_string();
    let flat = if note.is_empty() {
        body
    } else {
        format!("{body}{note}")
    };
    let flat_len = visible_len(&flat);
    Pretty {
        flat: flat.clone(),
        flat_len,
        multi: flat,
    }
}

/// Color the first "word" of a head string (the label) while leaving any
/// trailing span untouched. If the label already contains ANSI escapes,
/// leave the whole head unchanged.
fn colorize_head(head: &str, color: Color) -> String {
    if head.starts_with('(') {
        return head.to_string();
    }

    let (label, rest) = match head.find(char::is_whitespace) {
        Some(pos) => (&head[..pos], &head[pos..]),
        None => (head, ""),
    };

    if label.contains('\x1b') {
        head.to_string()
    } else {
        format!("{}{}", label.color(color).bold(), rest)
    }
}

fn pretty_group(depth: usize, head: String, children: Vec<Pretty>, color: Color) -> Pretty {
    let colored_head = colorize_head(&head, color);
    let open = "(".color(color).bold().to_string();
    let close = ")".color(color).bold().to_string();

    let mut flat_parts = vec![colored_head.clone()];
    flat_parts.extend(children.iter().map(|c| c.flat.clone()));
    let flat_body = flat_parts.join(" ");
    let flat = format!("{open}{flat_body}{close}");
    let flat_len = visible_len(&flat);

    // Force multi-line for blocks / containers when they have more than one item.
    let force_multi = matches!(
        head.split_whitespace().next(),
        Some("BLOCK" | "B" | "LIST" | "DICT")
    ) && children.len() > 1;

    if flat_len <= budget(depth) && !force_multi {
        Pretty {
            flat: flat.clone(),
            flat_len,
            multi: flat,
        }
    } else {
        let mut lines = vec![format!("{open}{}", colored_head)];
        for child in children {
            let child_text = if child.flat_len <= 20 {
                child.flat.clone()
            } else {
                child.multi.clone()
            };
            for line in child_text.lines() {
                lines.push(format!("  {line}"));
            }
        }
        if let Some(last) = lines.last_mut() {
            last.push_str(&close);
        } else {
            lines.push(close);
        }
        let multi = lines.join("\n");
        Pretty {
            flat,
            flat_len,
            multi,
        }
    }
}

impl AstNode {
    pub(crate) fn span(&self) -> AstSpan {
        use AstNode::*;
        fn merge(a: AstSpan, b: AstSpan) -> AstSpan {
            match (a, b) {
                (Some((s1, e1)), Some((s2, e2))) => Some((s1.min(s2), e1.max(e2))),
                (a, None) => a,
                (None, b) => b,
            }
        }
        match self {
            Error(_, s) | Literal(_, s) | Variable(_, s) | OuterVariable(_, s) => *s,
            Assignment { span, .. }
            | OuterAssignment { span, .. }
            | Postfix { span, .. }
            | Pipe { span, .. }
            | PipeTap { span, .. }
            | CallName { span, .. }
            | CallAnonymous { span, .. }
            | IndexAssign { span, .. }
            | MutatingIndex { span, .. }
            | MutatingIndexAssign { span, .. }
            | Assert { span, .. }
            | Debug { span, .. }
            | Pause { span, .. }
            | NamedArg { span, .. }
            | FString { span, .. } => *span,
            BinaryOp { left, right, .. } => {
                let mut span = right.span();
                let mut current = left.as_ref();
                while let BinaryOp {
                    left: next_left,
                    right: next_right,
                    ..
                } = current
                {
                    span = merge(next_right.span(), span);
                    current = next_left;
                }
                merge(current.span(), span)
            }
            ComparisonChain { first, rest } => {
                let mut s = first.span();
                for (_, n) in rest {
                    s = merge(s, n.span());
                }
                s
            }
            UnaryOp { span, .. } | Group { span, .. } => *span,
            Range {
                start, end, step, ..
            } => {
                let mut s = merge(start.span(), end.span());
                if let Some(step) = step {
                    s = merge(s, step.span());
                }
                s
            }
            Cat(items) | List(items) | Block(items) => {
                if items.is_empty() {
                    None
                } else {
                    merge(
                        items.first().and_then(|n| n.span()),
                        items.last().and_then(|n| n.span()),
                    )
                }
            }
            BlockExpr(_, span) => *span,
            Dict(kvs) => {
                if kvs.is_empty() {
                    None
                } else {
                    merge(
                        kvs.first().and_then(|(_, v)| v.span()),
                        kvs.last().and_then(|(_, v)| v.span()),
                    )
                }
            }

            Index { span, .. } => *span,
            Function { params, body, .. } => {
                let mut s = body.span();
                if let Some(ps) = params {
                    for p in ps {
                        s = merge(s, p.span());
                    }
                }
                s
            }
            Conditional { span, .. } | ConditionalDot { span, .. } => *span,
            ConditionalChain { span, .. } => *span,
            WLoop { span, .. } => *span,
            NLoop { span, .. } => *span,
            Return(opt) => opt.as_ref().and_then(|v| v.span()),
            Try(expr) => expr.span(),
            UnpackAssignment { span, lhs, rhs, .. } => {
                // Prefer the parser-stored span (covers `(`...rhs.end). Fall
                // back to merging children for legacy callers that built
                // `UnpackAssignment` by hand without filling `span`.
                if span.is_some() {
                    *span
                } else {
                    let mut s = merge(
                        lhs.first().and_then(|n| n.span()),
                        lhs.last().and_then(|n| n.span()),
                    );
                    s = merge(s, rhs.span());
                    s
                }
            }
            Ellipsis | PipeInput | Break | Continue => None,
        }
    }

    pub(crate) fn sexpr_pretty(&self) -> String {
        self.pretty_with_depth(0, None).multi
    }

    pub(crate) fn sexpr_pretty_with_source(&self, src: &str) -> String {
        let src = PrettySource {
            text: src,
            highlighter: SyntaxHighlighter::new(),
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
            Variable(name, _) => pretty_leaf(&format!("VAR[{}]", atom_ident(name)), &note, color),
            OuterVariable(name, _) => {
                pretty_leaf(&format!("OUTER-VAR[{}]", atom_ident(name)), &note, color)
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
            } => {
                let head = format!("BOP[{}]{note}", binary_op_display(operator));
                let l = left.pretty_with_depth(depth + 1, src);
                let r = right.pretty_with_depth(depth + 1, src);
                pretty_group(depth, head, vec![l, r], color)
            }
            ComparisonChain { first, rest } => {
                let mut children = vec![first.pretty_with_depth(depth + 1, src)];
                for (op, node) in rest {
                    children.push(pretty_leaf(binary_op_display(op), "", Color::White));
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
                let name_p = pretty_leaf(&atom_ident(name), "", Color::White);
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
                let name_p = pretty_leaf(&atom_ident(name), "", Color::White);
                let val_p = value.pretty_with_depth(depth + 1, src);
                pretty_group(depth, head, vec![name_p, val_p], color)
            }
            Ellipsis => pretty_leaf("...", &note, color),
            List(items) => {
                let children: Vec<Pretty> = items
                    .iter()
                    .map(|i| i.pretty_with_depth(depth + 1, src))
                    .collect();
                let head = format!("LIST{note}");
                pretty_group(depth, head, children, color)
            }
            Cat(items) => {
                let children: Vec<Pretty> = items
                    .iter()
                    .map(|i| i.pretty_with_depth(depth + 1, src))
                    .collect();
                let head = format!("CAT{note}");
                pretty_group(depth, head, children, color)
            }
            Dict(kvs) => {
                let mut children = Vec::with_capacity(kvs.len());
                for (k, v) in kvs {
                    let v_p = v.pretty_with_depth(depth + 2, src);
                    children.push(pretty_group(
                        depth + 1,
                        atom_ident(k),
                        vec![v_p],
                        Color::White,
                    ));
                }
                let head = format!("DICT{note}");
                pretty_group(depth, head, children, color)
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
            } => {
                let params_p = match params {
                    Some(ps) if !ps.is_empty() => {
                        let first = pretty_leaf(&atom_ident(ps[0].name()), "", Color::White);
                        let rest: Vec<Pretty> = ps[1..]
                            .iter()
                            .map(|p| match p {
                                Parameter::Pos { name, .. } => {
                                    pretty_leaf(&atom_ident(name), "", Color::White)
                                }
                                Parameter::Named {
                                    name,
                                    default: Some(_),
                                    ..
                                } => pretty_leaf(
                                    &format!("`{}:...", atom_ident(name)),
                                    "",
                                    Color::White,
                                ),
                                Parameter::Named {
                                    name,
                                    default: None,
                                    ..
                                } => {
                                    pretty_leaf(&format!("`{}", atom_ident(name)), "", Color::White)
                                }
                            })
                            .collect();
                        pretty_group(depth + 1, first.flat, rest, Color::White)
                    }
                    Some(_) => pretty_group(depth + 1, String::new(), vec![], Color::White),
                    None => pretty_group(depth + 1, "implicit".to_string(), vec![], Color::White),
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
            Break => pretty_leaf("@b", &note, color),
            Continue => pretty_leaf("@c", &note, color),
            Return(opt) => {
                let mut children = Vec::new();
                if let Some(v) = opt {
                    children.push(v.pretty_with_depth(depth + 1, src));
                }
                let head = format!("@r{note}");
                pretty_group(depth, head, children, color)
            }
            Assert { expr, .. } => {
                let head = format!("@a{note}");
                let child = expr.pretty_with_depth(depth + 1, src);
                pretty_group(depth, head, vec![child], color)
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
            Try(expr) => {
                let head = format!("@t{note}");
                let child = expr.pretty_with_depth(depth + 1, src);
                pretty_group(depth, head, vec![child], color)
            }
            Block(stmts) => {
                let children: Vec<Pretty> = stmts
                    .iter()
                    .map(|s| s.pretty_with_depth(depth + 1, src))
                    .collect();
                let head = format!("BLOCK{note}");
                pretty_group(depth, head, children, color)
            }
            BlockExpr(stmts, _) => {
                let children: Vec<Pretty> = stmts
                    .iter()
                    .map(|s| s.pretty_with_depth(depth + 1, src))
                    .collect();
                let head = format!("B{note}");
                pretty_group(depth, head, children, color)
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
                                label.multi.push_str(&format!(" !{sp:?}"));
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
    use super::*;

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
}
