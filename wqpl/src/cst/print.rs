//! Pretty printer for green and red CST trees.
//!
//! Mirrors the style of [`crate::astnode::AstNode`] pretty printing:
//! s-expression layout with heuristic line-breaking, ANSI colouring by
//! [`SyntaxKind`], and optional source-span annotations.

use colored::{Color, Colorize};

use super::green::{GreenChild, GreenNode, GreenToken};
use super::kind::SyntaxKind;
use super::red::{SyntaxElement, SyntaxNode, SyntaxToken, TextRange};

// -----------------------------------------------------------------------
// Colour scheme
// -----------------------------------------------------------------------

fn kind_color(kind: SyntaxKind) -> Color {
    use SyntaxKind::*;
    match kind {
        // Trivia — dim so it doesn't compete with real tokens.
        Whitespace | Newline | Comment => Color::BrightBlack,
        ScriptLine => Color::BrightYellow,

        // Literals
        IntLit | BigIntLit | FloatLit | ImagLit | CharLit | StringLit | TagLit | InfLit
        | TrueKw | FalseKw | FString => Color::Cyan,

        // Identifiers
        Ident | Apostrophe => Color::Blue,

        // Keywords / directives
        AtAssert | AtBreak | AtContinue | AtReturn | AtDebug | AtPause | AtDepth | AtSymbolic
        | AtTry | Dollar | DollarDot | DollarDollar => Color::Green,

        // Operators
        Plus | Minus | Star | Slash | SlashDot | Percent | Power | PowerDot | Matmul | FloorDiv
        | BoolAnd | BoolOr | BitAnd | BitOr | Shl | Shr | BitXor | PlusColon | MinusColon
        | StarColon | SlashColon | SlashDotColon | PercentColon | PowerColon | PowerDotColon
        | CommaColon | BoolAndColon | BoolOrColon | BitAndColon | BitOrColon | ShlColon
        | ShrColon | BitXorColon | FloorDivColon | EqEq | EqDot | NotEq | NotEqDot | Lt | Le
        | Gt | Ge => Color::Yellow,

        // Punctuation / brackets
        Colon | Hash | Pipe | PipeDot | PipePipe | PipePipeDot | RangeOp | RangeIncOp | LParen
        | RParen | LBrack | RBrack | LBrace | RBrace | Semicolon | Comma | Bang | Ellipsis
        | Backtick => Color::White,

        // Error token
        ErrorTok => Color::BrightMagenta,

        // Internal nodes — expressions are magenta, containers white, errors bright magenta
        Root => Color::White,
        Block => Color::White,
        Shebang | ScriptDirective => Color::BrightYellow,
        LiteralExpr | VarExpr | OuterVarExpr => Color::Cyan,
        BinaryExpr | UnaryExpr | ComparisonChainExpr | RangeExpr => Color::Yellow,
        AssignExpr
        | OuterAssignExpr
        | UnpackAssignExpr
        | IndexAssignExpr
        | MutatingIndexAssignExpr => Color::Red,
        MutatingIndexExpr => Color::BrightBlue,
        ListExpr | DictExpr | ParenExpr | BlockExpr => Color::White,
        PostfixExpr | NamedArgExpr | ArgList => Color::Magenta,
        FStringExpr => Color::Cyan,
        CondExpr | CondDotExpr | CondChainExpr | WLoopExpr | NLoopExpr | FunctionExpr
        | ParamList | Param | ReturnExpr | AssertExpr | DebugExpr | PauseExpr | TryExpr
        | SymbolicExpr | BreakExpr | ContinueExpr | EllipsisExpr | PipeExpr | PipeTapExpr
        | DictPair => Color::Green,
        ErrorNode => Color::BrightMagenta,

        __LastToken => Color::White,
    }
}

// -----------------------------------------------------------------------
// Layout helpers (same primitives as the AST printer)
// -----------------------------------------------------------------------

/// Visible width of a string that may contain ANSI escapes.
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

/// Budget shrinks with depth so deeply nested trees break earlier.
fn budget(depth: usize) -> usize {
    60usize.saturating_sub(depth * 2)
}

/// A document with a single-line (flat) form and a potentially multi-line form.
struct Pretty {
    flat: String,
    flat_len: usize,
    multi: String,
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

fn pretty_group(depth: usize, head: String, children: Vec<Pretty>, color: Color) -> Pretty {
    let colored_head = head.color(color).bold().to_string();
    let open = "(".color(color).bold().to_string();
    let close = ")".color(color).bold().to_string();

    let mut flat_parts = vec![colored_head.clone()];
    flat_parts.extend(children.iter().map(|c| c.flat.clone()));
    let flat_body = flat_parts.join(" ");
    let flat = format!("{open}{flat_body}{close}");
    let flat_len = visible_len(&flat);

    // Force multi-line for containers with more than one child.
    let force_multi = matches!(
        head.split_whitespace().next(),
        Some("ROOT" | "BLOCK" | "LIST_EXPR" | "DICT_EXPR" | "SET_EXPR" | "PARAM_LIST" | "ARG_LIST")
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

// -----------------------------------------------------------------------
// Escaping helpers
// -----------------------------------------------------------------------

fn escape_text(text: &str, max_len: usize) -> String {
    let mut s = text.replace('\n', "\\n").replace('\t', "\\t");
    if s.chars().count() > max_len {
        s = s.chars().take(max_len).collect::<String>() + "...";
    }
    s
}

// -----------------------------------------------------------------------
// Green tree printing
// -----------------------------------------------------------------------

fn green_token_pretty(token: &GreenToken, _depth: usize) -> Pretty {
    let kind = token.kind();
    let color = kind_color(kind);
    let text = token.text();
    let note = if kind.is_trivia() {
        format!(" \"{}\"", escape_text(text, 20))
    } else {
        format!(" \"{}\"", escape_text(text, 40))
    };
    pretty_leaf(&format!("{}{note}", kind.name()), "", color)
}

fn green_child_pretty(child: &GreenChild, depth: usize) -> Pretty {
    match child {
        GreenChild::Node(n) => green_node_pretty(n, depth),
        GreenChild::Token(t) => green_token_pretty(t, depth),
    }
}

fn green_node_pretty(node: &GreenNode, depth: usize) -> Pretty {
    let kind = node.kind();
    let color = kind_color(kind);
    let children: Vec<Pretty> = node
        .children()
        .iter()
        .map(|c| green_child_pretty(c, depth + 1))
        .collect();
    let head = kind.name().to_string();
    pretty_group(depth, head, children, color)
}

impl std::fmt::Display for GreenToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&green_token_pretty(self, 0).multi)
    }
}

impl std::fmt::Display for GreenNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&green_node_pretty(self, 0).multi)
    }
}

impl std::fmt::Display for GreenChild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GreenChild::Node(n) => n.fmt(f),
            GreenChild::Token(t) => t.fmt(f),
        }
    }
}

// -----------------------------------------------------------------------
// Red tree printing
// -----------------------------------------------------------------------

fn fmt_range_note(range: TextRange) -> String {
    format!(" [{}..{}]", range.start(), range.end())
}

fn red_token_pretty(token: &SyntaxToken, _depth: usize, show_span: bool) -> Pretty {
    let kind = token.kind();
    let color = kind_color(kind);
    let text = token.text();
    let note = if show_span {
        fmt_range_note(token.text_range())
    } else {
        String::new()
    };
    let text_escaped = escape_text(text, if kind.is_trivia() { 20 } else { 40 });
    let label = format!("{} \"{text_escaped}\"{note}", kind.name());
    pretty_leaf(&label, "", color)
}

fn red_element_pretty(elem: &SyntaxElement, depth: usize, show_span: bool) -> Pretty {
    match elem {
        SyntaxElement::Node(n) => red_node_pretty(n, depth, show_span),
        SyntaxElement::Token(t) => red_token_pretty(t, depth, show_span),
    }
}

fn red_node_pretty(node: &SyntaxNode, depth: usize, show_span: bool) -> Pretty {
    let kind = node.kind();
    let color = kind_color(kind);
    let note = if show_span {
        fmt_range_note(node.text_range())
    } else {
        String::new()
    };
    let children: Vec<Pretty> = node
        .children_with_tokens()
        .map(|c| red_element_pretty(&c, depth + 1, show_span))
        .collect();
    let head = format!("{}{note}", kind.name());
    pretty_group(depth, head, children, color)
}

impl SyntaxNode {
    /// Pretty-print this red node without source offsets.
    pub fn pretty_print(&self) -> String {
        red_node_pretty(self, 0, false).multi
    }

    /// Pretty-print this red node including byte-range annotations.
    pub fn pretty_print_with_spans(&self) -> String {
        red_node_pretty(self, 0, true).multi
    }
}

impl SyntaxToken {
    /// Pretty-print this red token without source offsets.
    pub fn pretty_print(&self) -> String {
        red_token_pretty(self, 0, false).multi
    }

    /// Pretty-print this red token including its byte-range annotation.
    pub fn pretty_print_with_span(&self) -> String {
        red_token_pretty(self, 0, true).multi
    }
}

impl std::fmt::Display for SyntaxNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.pretty_print())
    }
}

impl std::fmt::Display for SyntaxToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.pretty_print())
    }
}

impl std::fmt::Display for SyntaxElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyntaxElement::Node(n) => n.fmt(f),
            SyntaxElement::Token(t) => t.fmt(f),
        }
    }
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::green::{GreenChild, GreenNode, GreenToken};
    use super::super::kind::SyntaxKind;
    use super::super::red::SyntaxNode;

    fn tok(kind: SyntaxKind, text: &str) -> GreenChild {
        GreenChild::Token(GreenToken::new(kind, text))
    }

    fn node(kind: SyntaxKind, children: Vec<GreenChild>) -> GreenChild {
        GreenChild::Node(GreenNode::new(kind, children))
    }

    #[test]
    fn green_token_display_includes_text() {
        let t = GreenToken::new(SyntaxKind::IntLit, "42");
        let s = format!("{t}");
        assert!(s.contains("INT"), "{s}");
        assert!(s.contains("42"), "{s}");
    }

    #[test]
    fn green_node_display_is_tree() {
        let n = GreenNode::new(
            SyntaxKind::BinaryExpr,
            vec![
                tok(SyntaxKind::IntLit, "1"),
                tok(SyntaxKind::Plus, "+"),
                tok(SyntaxKind::IntLit, "2"),
            ],
        );
        let s = format!("{n}");
        assert!(s.contains("BINARY_EXPR"), "{s}");
        assert!(s.contains("INT \"1\""), "{s}");
        assert!(s.contains("PLUS \"+\""), "{s}");
    }

    #[test]
    fn red_node_display_matches_green() {
        let green = GreenNode::new(
            SyntaxKind::Root,
            vec![node(
                SyntaxKind::BinaryExpr,
                vec![
                    tok(SyntaxKind::IntLit, "1"),
                    tok(SyntaxKind::Whitespace, " "),
                    tok(SyntaxKind::Plus, "+"),
                    tok(SyntaxKind::Whitespace, " "),
                    tok(SyntaxKind::IntLit, "2"),
                ],
            )],
        );
        let red = SyntaxNode::new_root(green);
        let s = red.pretty_print();
        assert!(s.contains("ROOT"), "{s}");
        assert!(s.contains("BINARY_EXPR"), "{s}");
        assert!(s.contains("INT \"1\""), "{s}");
    }

    #[test]
    fn red_node_with_spans_shows_ranges() {
        let green = GreenNode::new(SyntaxKind::Root, vec![tok(SyntaxKind::IntLit, "99")]);
        let red = SyntaxNode::new_root(green);
        let s = red.pretty_print_with_spans();
        assert!(s.contains("[0..2]"), "{s}");
    }

    #[test]
    fn trivia_is_dimmed_but_present() {
        let t = GreenToken::new(SyntaxKind::Whitespace, "   ");
        let s = format!("{t}");
        assert!(s.contains("WHITESPACE"), "{s}");
        assert!(s.contains("\"   \""), "{s}");
    }

    #[test]
    fn long_token_text_is_truncated() {
        let long = "a".repeat(100);
        let t = GreenToken::new(SyntaxKind::StringLit, long.as_str());
        let s = format!("{t}");
        assert!(s.contains("..."), "{s}");
    }

    #[test]
    fn multiline_text_is_escaped() {
        let t = GreenToken::new(SyntaxKind::StringLit, "hello\nworld");
        let s = format!("{t}");
        assert!(s.contains("\\n"), "{s}");
        assert!(!s.contains('\n'), "{s}");
    }

    #[test]
    fn nested_node_breaks_lines_when_deep() {
        // Build a deeply nested left-associative chain: ((((1))))
        let mut inner = tok(SyntaxKind::IntLit, "1");
        for _ in 0..10 {
            inner = node(
                SyntaxKind::ParenExpr,
                vec![
                    tok(SyntaxKind::LParen, "("),
                    inner,
                    tok(SyntaxKind::RParen, ")"),
                ],
            );
        }
        let green = GreenNode::new(SyntaxKind::Root, vec![inner]);
        let s = format!("{green}");
        // Should have broken into multiple lines due to depth/budget.
        assert!(s.contains('\n'), "{s}");
    }
}
