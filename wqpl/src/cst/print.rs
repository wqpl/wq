//! Pretty-printer for green and red CST trees.
//!
//! Uses s-expression layout, ANSI color by [`SyntaxKind`], and optional byte
//! spans.

use super::green::{GreenChild, GreenNode, GreenToken};
use super::kind::SyntaxKind;
use super::red::{SyntaxElement, SyntaxNode, SyntaxToken, TextRange};
use crate::style::AnsiColor;
use crate::tree_pretty::{self, HeadStyle, Pretty};

// Color scheme.

fn kind_color(kind: SyntaxKind) -> AnsiColor {
    use SyntaxKind::*;
    match kind {
        // Trivia.
        Whitespace | Newline | Comment => AnsiColor::BrightBlack,
        ScriptLine => AnsiColor::BrightYellow,

        // Literals.
        IntLit | BigIntLit | FloatLit | ImagLit | CharLit | StringLit | TagLit | InfLit
        | TrueKw | FalseKw | FString => AnsiColor::Cyan,

        // Identifiers.
        Ident | Apostrophe => AnsiColor::Blue,

        // Keywords and directives.
        AtAssert | AtBreak | AtContinue | AtReturn | AtDebug | AtPause | AtDepth | AtSymbolic
        | AtTry | Dollar | DollarDot | DollarDollar => AnsiColor::Green,

        // Operators.
        Plus | Minus | Star | Slash | SlashDot | Percent | Power | PowerDot | Matmul | FloorDiv
        | PlusColon | MinusColon | StarColon | SlashColon | SlashDotColon | PercentColon
        | PowerColon | PowerDotColon | CommaColon | FloorDivColon | EqEq | EqDot | NotEq
        | NotEqDot | Lt | Le | Gt | Ge => AnsiColor::Yellow,

        // Punctuation and brackets.
        Colon | Hash | Pipe | PipeDot | PipePipe | PipePipeDot | RangeOp | RangeIncOp | LParen
        | RParen | LBrack | RBrack | LBrace | RBrace | Semicolon | Comma | Bang | Ellipsis
        | Backtick => AnsiColor::White,

        // Error token.
        ErrorTok => AnsiColor::BrightMagenta,

        // Internal nodes.
        Root => AnsiColor::White,
        Block => AnsiColor::White,
        Shebang | ScriptDirective => AnsiColor::BrightYellow,
        LiteralExpr | VarExpr | OuterVarExpr => AnsiColor::Cyan,
        BinaryExpr | UnaryExpr | ComparisonChainExpr | RangeExpr => AnsiColor::Yellow,
        AssignExpr
        | OuterAssignExpr
        | UnpackAssignExpr
        | IndexAssignExpr
        | MutatingIndexAssignExpr => AnsiColor::Red,
        MutatingIndexExpr => AnsiColor::BrightBlue,
        ListExpr | DictExpr | ParenExpr | BlockExpr => AnsiColor::White,
        PostfixExpr | NamedArgExpr | ArgList => AnsiColor::Magenta,
        FStringExpr => AnsiColor::Cyan,
        CondExpr | CondDotExpr | CondChainExpr | WLoopExpr | NLoopExpr | FunctionExpr
        | ParamList | Param | ReturnExpr | AssertExpr | DebugExpr | PauseExpr | TryExpr
        | SymbolicExpr | BreakExpr | ContinueExpr | EllipsisExpr | PipeExpr | PipeTapExpr
        | DictPair => AnsiColor::Green,
        ErrorNode => AnsiColor::BrightMagenta,

        __LastToken => AnsiColor::White,
    }
}

// Layout helpers.

fn pretty_leaf(text: &str, note: &str, color: AnsiColor) -> Pretty {
    tree_pretty::leaf(text, note, color)
}

fn force_multiline_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Root
            | SyntaxKind::Block
            | SyntaxKind::ListExpr
            | SyntaxKind::DictExpr
            | SyntaxKind::ParamList
            | SyntaxKind::ArgList
    )
}

fn pretty_group(
    depth: usize,
    head: String,
    children: Vec<Pretty>,
    color: AnsiColor,
    force_multi: bool,
) -> Pretty {
    tree_pretty::group(depth, head, children, color, HeadStyle::Whole, force_multi)
}

// Escaping helpers.

fn escape_text(text: &str, max_len: usize) -> String {
    let mut s = text.replace('\n', "\\n").replace('\t', "\\t");
    if s.chars().count() > max_len {
        s = s.chars().take(max_len).collect::<String>() + "...";
    }
    s
}

// Green tree printing.

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
    let force_multi = force_multiline_kind(kind) && children.len() > 1;
    pretty_group(depth, head, children, color, force_multi)
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

// Red tree printing.

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
    let force_multi = force_multiline_kind(kind) && children.len() > 1;
    pretty_group(depth, head, children, color, force_multi)
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

// Tests.

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
    fn green_token_display_includes_text() {
        let t = GreenToken::new(SyntaxKind::IntLit, "42");
        let s = format!("{t}");
        assert!(s.contains("INT"), "{s}");
        assert!(s.contains("42"), "{s}");
    }

    #[test]
    fn green_token_display_uses_explicit_style_renderer() {
        let t = GreenToken::new(SyntaxKind::IntLit, "42");
        let s = format!("{t}");

        assert_eq!(s, "\x1b[1;36mINT \"42\"\x1b[0m");
        assert_eq!(strip_ansi(&s), "INT \"42\"");
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
        // Deep nesting should force a broken layout.
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
        assert!(s.contains('\n'), "{s}");
    }
}
