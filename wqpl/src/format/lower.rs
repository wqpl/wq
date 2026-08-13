//! CST -> [`Doc`] lowering.
//!
//! Walks a [`SyntaxNode`] tree and emits a [`Doc`] that, when rendered,
//! produces the formatted source. Every CST node kind has a corresponding
//! lowering branch in [`LowerCtx::node`]; trivia is filtered out by the
//! lowering pass itself (whitespace is regenerated via [`Doc::Line`] /
//! [`Doc::LineSoft`], comments are reattached at appropriate seams in a
//! follow-up phase).
//!
//! ## Style choices
//!
//! The lowering aims to match the long-standing AST-based formatter on
//! comment-free inputs so the existing `hotchoco/suite` snapshots remain
//! green. The notable normalizations are:
//!
//! * Single-argument postfix forms prefer `f x` when that reparses the same way
//!   and avoids noisier `f[x]` brackets.
//! * Inside `[...]` / `(...)` / `(`...`)` the contents are joined by `;` (no
//!   whitespace) when flat, by `;\n  ` when broken.
//! * Function bodies and control-form bodies break across newlines, with
//!   children indented by [`FormatConfig::indent_size`].
//! * Binary, unary, range, and assignment operators glue tightly (no
//!   surrounding whitespace).
//!
//! Width-aware breaking is applied at separator-joined constructs (argument
//! lists, list/dict literals, set literals) and binary / comparison chains: if
//! the flat form exceeds the configured width, the renderer breaks at the soft
//! line opportunities.

use super::FormatConfig;
use super::doc::Doc;
use super::render::render;
use crate::cst::{SyntaxElement, SyntaxKind, SyntaxNode};
use crate::lex::Lexer;
use crate::parse::Parser;
use crate::token::{FmtPart, TokenType};

pub(super) fn lower(root: &SyntaxNode, config: &FormatConfig) -> Doc {
    LowerCtx {
        config,
        mode: LowerMode::Normal,
    }
    .node(root)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LowerMode {
    Normal,
    Inline,
}

struct LowerCtx<'a> {
    config: &'a FormatConfig,
    mode: LowerMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingStmtSep {
    Semicolon,
    SemicolonLine,
    SemicolonBlank,
    Line,
    Blank,
}

#[derive(Debug, Default)]
struct DelimitedLayout {
    rows: Vec<DelimitedRow>,
    saw_newline: bool,
    leading_newline: bool,
    close_on_own_line: bool,
}

#[derive(Debug, Default)]
struct DelimitedRow {
    items: Vec<Doc>,
    trailing_semicolon: bool,
}

impl<'a> LowerCtx<'a> {
    fn indent(&self) -> i32 {
        self.config.indent_size as i32
    }

    fn is_inline_like(&self) -> bool {
        self.config.oneline || self.mode == LowerMode::Inline
    }

    /// Lower a sequence of statement-level children, attaching trivia
    /// (comments and blank lines) to the right seams.
    ///
    /// `iter` should iterate over the **children-with-tokens** of a node
    /// whose role is "a sequence of statements", i.e. [`SyntaxKind::Root`],
    /// [`SyntaxKind::Block`], a function body, or a control-form body.
    ///
    /// ## Attachment rules
    ///
    /// * A `Comment` token that appears with **no `Newline` between it and the
    ///   preceding non-trivia node** is treated as a *trailing* comment:
    ///   emitted glued to the same line as the previous statement, separated by
    ///   a single space.
    /// * Otherwise it is a *standalone* comment: emitted on its own line, with
    ///   a hard newline separator before and after.
    /// * A source `Semicolon` with no intervening `Newline` keeps adjacent
    ///   short statements on the same line.
    /// * Two or more `Newline` tokens between consecutive elements constitute a
    ///   *blank line*: the next element is preceded by an extra newline via
    ///   [`Doc::blank`].
    /// * `Whitespace` tokens are dropped (the formatter regenerates them).
    ///
    /// In inline-like mode the whole sequence collapses to a single line;
    /// comments are dropped entirely in that mode since there is no place to
    /// put them without breaking the one-line invariant. Callers that need
    /// comment-safe inline formatting must reject commented subtrees before
    /// lowering.
    fn lower_stmt_sequence<I>(&self, iter: I) -> Doc
    where
        I: IntoIterator<Item = SyntaxElement>,
    {
        if self.is_inline_like() {
            // In one-line mode trivia is dropped; just `;`-join statements.
            let stmts: Vec<Doc> = iter
                .into_iter()
                .filter_map(|e| match e {
                    SyntaxElement::Node(n) => Some(self.node(&n)),
                    SyntaxElement::Token(_) => None,
                })
                .collect();
            return Doc::join(Doc::text(";"), stmts);
        }
        let mut rows: Vec<(PendingStmtSep, Doc)> = Vec::new();
        let mut row_items: Vec<Doc> = Vec::new();
        let mut sep_before_row = PendingStmtSep::Line;
        let mut newlines_since_payload = 0u32;
        let mut semicolon_since_payload = false;
        for elem in iter {
            match elem {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::Whitespace => {}
                    SyntaxKind::Semicolon => {
                        semicolon_since_payload = true;
                    }
                    SyntaxKind::Newline => {
                        newlines_since_payload = newlines_since_payload.saturating_add(1);
                    }
                    SyntaxKind::Comment => {
                        let text = t.text().to_string();
                        if newlines_since_payload == 0 && !row_items.is_empty() {
                            // Trailing on the previous statement's line.
                            let last = row_items.pop().expect("row is not empty");
                            row_items.push(last + Doc::text(format!(" {text}")));
                        } else {
                            // Standalone comment line.
                            let sep = Self::pending_stmt_sep(
                                newlines_since_payload,
                                semicolon_since_payload,
                            );
                            if !row_items.is_empty() {
                                self.finish_stmt_row(&mut rows, &mut row_items, sep_before_row);
                                sep_before_row = sep;
                            } else if !rows.is_empty() {
                                sep_before_row = sep;
                            }
                            row_items.push(Doc::text(text));
                        }
                        newlines_since_payload = 0;
                        semicolon_since_payload = false;
                    }
                    // Other tokens at statement-sequence level are
                    // unexpected (they would be inside the statement
                    // nodes), but we ignore them defensively rather than
                    // panicking.
                    _ => {}
                },
                SyntaxElement::Node(n) => {
                    let stmt = self.node(&n);
                    let sep =
                        Self::pending_stmt_sep(newlines_since_payload, semicolon_since_payload);
                    let drop_semicolon = sep == PendingStmtSep::Semicolon
                        && row_items.iter().any(Doc::has_forced_break);
                    if !row_items.is_empty() && (sep != PendingStmtSep::Semicolon || drop_semicolon)
                    {
                        self.finish_stmt_row(&mut rows, &mut row_items, sep_before_row);
                        sep_before_row = if drop_semicolon {
                            PendingStmtSep::Line
                        } else {
                            sep
                        };
                    } else if row_items.is_empty() && !rows.is_empty() {
                        sep_before_row = sep;
                    }
                    row_items.push(stmt);
                    newlines_since_payload = 0;
                    semicolon_since_payload = false;
                }
            }
        }
        self.finish_stmt_row(&mut rows, &mut row_items, sep_before_row);

        let mut out = Doc::nil();
        let mut first = true;
        for (sep, row) in rows {
            if !first {
                out = out + Self::pending_stmt_sep_kind_doc(sep);
            }
            out = out + row;
            first = false;
        }
        out
    }

    fn finish_stmt_row(
        &self,
        rows: &mut Vec<(PendingStmtSep, Doc)>,
        row_items: &mut Vec<Doc>,
        sep_before_row: PendingStmtSep,
    ) {
        if row_items.is_empty() {
            return;
        }
        let items = std::mem::take(row_items);
        rows.push((sep_before_row, self.stmt_row_doc(items)));
    }

    fn stmt_row_doc(&self, items: Vec<Doc>) -> Doc {
        if items.len() == 1 {
            return items.into_iter().next().expect("len == 1");
        }
        Doc::group(Doc::join(Doc::text(";") + Doc::line_soft(), items))
    }

    fn pending_stmt_sep_kind_doc(sep: PendingStmtSep) -> Doc {
        match sep {
            PendingStmtSep::Semicolon => Doc::text(";"),
            PendingStmtSep::SemicolonLine => Doc::text(";") + Doc::line_hard(),
            PendingStmtSep::SemicolonBlank => Doc::text(";") + Doc::line_hard() + Doc::blank(),
            PendingStmtSep::Line => Doc::line_hard(),
            PendingStmtSep::Blank => Doc::line_hard() + Doc::blank(),
        }
    }

    fn pending_stmt_sep(newlines: u32, semicolon: bool) -> PendingStmtSep {
        if semicolon && newlines >= 2 {
            PendingStmtSep::SemicolonBlank
        } else if semicolon && newlines == 1 {
            PendingStmtSep::SemicolonLine
        } else if newlines >= 2 {
            PendingStmtSep::Blank
        } else if newlines == 1 {
            PendingStmtSep::Line
        } else if semicolon {
            PendingStmtSep::Semicolon
        } else {
            // Adjacent payload nodes without an explicit source separator
            // should not normally occur. Keep the previous defensive behavior
            // by separating them onto distinct statement lines.
            PendingStmtSep::Line
        }
    }

    fn node(&self, node: &SyntaxNode) -> Doc {
        match node.kind() {
            SyntaxKind::Root => self.root(node),

            SyntaxKind::LiteralExpr
            | SyntaxKind::VarExpr
            | SyntaxKind::OuterVarExpr
            | SyntaxKind::EllipsisExpr
            | SyntaxKind::BreakExpr
            | SyntaxKind::ContinueExpr
            | SyntaxKind::ImportExpr => self.verbatim_concat(node),

            SyntaxKind::FStringExpr => self.fstring(node),

            SyntaxKind::ParenExpr => self.paren(node),
            SyntaxKind::ListExpr => self.list_or_dict(node, /* dict = */ false),
            SyntaxKind::DictExpr => self.list_or_dict(node, /* dict = */ true),
            SyntaxKind::DictPair => self.dict_pair(node),

            SyntaxKind::PostfixExpr => self.postfix(node),
            SyntaxKind::MutatingIndexExpr => self.mutating_index(node),
            SyntaxKind::ArgList => self.arglist(node),

            SyntaxKind::BinaryExpr => self.binary(node),
            SyntaxKind::ComparisonChainExpr => self.binary(node),
            SyntaxKind::UnaryExpr => self.tight_concat(node),
            SyntaxKind::RangeExpr => self.tight_concat(node),

            SyntaxKind::AssignExpr
            | SyntaxKind::OuterAssignExpr
            | SyntaxKind::UnpackAssignExpr
            | SyntaxKind::IndexAssignExpr
            | SyntaxKind::MutatingIndexAssignExpr => self.assign(node),

            SyntaxKind::PipeExpr => self.tight_concat(node),
            SyntaxKind::PipeTapExpr => self.tight_concat(node),

            SyntaxKind::FunctionExpr => self.function(node),
            SyntaxKind::ParamList => self.param_list(node),
            SyntaxKind::Param => self.verbatim_concat(node),

            SyntaxKind::CondExpr | SyntaxKind::CondDotExpr | SyntaxKind::CondChainExpr => {
                self.control_form(node)
            }
            SyntaxKind::LazyBoolExpr => self.tight_concat(node),
            SyntaxKind::WLoopExpr | SyntaxKind::NLoopExpr | SyntaxKind::BlockExpr => {
                self.control_form(node)
            }

            SyntaxKind::ReturnExpr => self.at_keyword_expr(node),
            SyntaxKind::DebugExpr | SyntaxKind::TryExpr => self.at_keyword_expr(node),
            SyntaxKind::PauseExpr => self.verbatim_concat(node),
            SyntaxKind::SymbolicExpr => self.verbatim_concat(node),

            SyntaxKind::Block => self.block(node),

            SyntaxKind::ErrorNode => Doc::text("/* error */"),

            // Anything else (mostly leaf token kinds that shouldn't appear
            // at node level) falls back to verbatim concat, which is the
            // safe choice for round-trip.
            _ => self.verbatim_concat(node),
        }
    }

    // ===== top-level / blocks =====

    fn root(&self, node: &SyntaxNode) -> Doc {
        self.lower_stmt_sequence(node.children_with_tokens())
    }

    fn block(&self, node: &SyntaxNode) -> Doc {
        // Block as a node: a sequence of statements. Used in nested
        // contexts where the caller has already produced the surrounding
        // brackets and indentation.
        self.lower_stmt_sequence(node.children_with_tokens())
    }

    // ===== atomic / verbatim =====

    /// Concatenate every non-trivia descendant token's text verbatim.
    /// Used for atomic forms (literals, identifiers, f-strings, etc.)
    /// where the source text is the formatted form.
    fn verbatim_concat(&self, node: &SyntaxNode) -> Doc {
        let mut out = Doc::nil();
        for elem in node.descendants_with_tokens() {
            if let SyntaxElement::Token(t) = elem
                && !t.kind().is_trivia()
            {
                out = out + Doc::text(t.text().to_string());
            }
        }
        out
    }

    fn fstring(&self, node: &SyntaxNode) -> Doc {
        let Some(text) = node.descendants_with_tokens().find_map(|elem| {
            if let SyntaxElement::Token(t) = elem
                && t.kind() == SyntaxKind::FString
            {
                Some(t.text().to_string())
            } else {
                None
            }
        }) else {
            return self.verbatim_concat(node);
        };

        Doc::text(
            self.format_fstring_token(&text)
                .unwrap_or_else(|| text.to_string()),
        )
    }

    fn format_fstring_token(&self, text: &str) -> Option<String> {
        let mut lexer = Lexer::new(text);
        let tokens = lexer.tokenize().ok()?;
        let token = tokens
            .into_iter()
            .find(|token| !matches!(token.token_type, TokenType::Eof))?;
        let TokenType::FormatString(parts, ..) = token.token_type else {
            return None;
        };

        let mut out = String::new();
        let mut cursor = 0;
        for part in parts {
            match part {
                FmtPart::Text { end, .. } => {
                    Self::push_checked_slice(&mut out, text, cursor, end)?;
                    cursor = end;
                }
                FmtPart::Expr { source, start, end } => {
                    Self::push_checked_slice(&mut out, text, cursor, start)?;
                    out.push_str(
                        &self
                            .format_fstring_expr(&source)
                            .unwrap_or_else(|| source.to_string()),
                    );
                    cursor = end;
                }
            }
        }
        Self::push_checked_slice(&mut out, text, cursor, text.len())?;
        Some(out)
    }

    fn push_checked_slice(out: &mut String, text: &str, start: usize, end: usize) -> Option<()> {
        if start > end
            || end > text.len()
            || !text.is_char_boundary(start)
            || !text.is_char_boundary(end)
        {
            return None;
        }
        out.push_str(&text[start..end]);
        Some(())
    }

    fn format_fstring_expr(&self, source: &str) -> Option<String> {
        let inner = source.strip_prefix('{')?.strip_suffix('}')?;
        let split = Parser::split_expr_and_format_spec(inner).ok()?;
        let formatted_expr = self.format_inline_expr(split.expr)?;
        let Some(spec) = split.spec else {
            return Some(format!("{{{formatted_expr}}}"));
        };
        Some(format!(
            "{{[{}]{formatted_expr}}}",
            self.format_fstring_spec(spec)
        ))
    }

    fn format_fstring_spec(&self, spec: &str) -> String {
        let mut out = String::new();
        let mut cursor = 0;
        while cursor < spec.len() {
            let c = spec[cursor..]
                .chars()
                .next()
                .expect("cursor is inside spec");
            if c != '{' {
                out.push(c);
                cursor += c.len_utf8();
                continue;
            }

            let Some(close) = Parser::matching_fstring_brace(spec, cursor) else {
                out.push_str(&spec[cursor..]);
                break;
            };
            let inner_start = cursor + c.len_utf8();
            let inner = &spec[inner_start..close];
            if let Some(formatted) = self.format_inline_expr(inner) {
                out.push('{');
                out.push_str(&formatted);
                out.push('}');
            } else {
                out.push_str(&spec[cursor..close + '}'.len_utf8()]);
            }
            cursor = close + '}'.len_utf8();
        }
        out
    }

    fn format_inline_expr(&self, source: &str) -> Option<String> {
        let source = source.trim();
        if source.is_empty() {
            return None;
        }

        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize().ok()?;
        let mut parser = Parser::new(tokens, source.to_string());
        parser.enable_cst();
        let _ast = parser.parse().ok()?;
        let green = parser.take_cst()?;
        let root = SyntaxNode::new_root(green);
        if Self::contains_comment_or_error(&root) {
            return None;
        }
        let doc = LowerCtx {
            config: self.config,
            mode: LowerMode::Inline,
        }
        .node(&root);
        Some(render(&doc, usize::MAX))
    }

    fn contains_comment_or_error(node: &SyntaxNode) -> bool {
        node.descendants_with_tokens().any(|elem| match elem {
            SyntaxElement::Token(t) => t.kind() == SyntaxKind::Comment,
            SyntaxElement::Node(n) => n.kind() == SyntaxKind::ErrorNode,
        })
    }

    /// Concatenate non-trivia children tightly: token texts and lowered
    /// nodes, no whitespace introduced. Used by binary/unary/range/pipe
    /// expressions where operators glue without space.
    fn tight_concat(&self, node: &SyntaxNode) -> Doc {
        let mut out = Doc::nil();
        for elem in node.children_with_tokens() {
            match elem {
                SyntaxElement::Token(t) if !t.kind().is_trivia() => {
                    out = out + Doc::text(t.text().to_string());
                }
                SyntaxElement::Token(_) => {}
                SyntaxElement::Node(n) => {
                    out = out + self.node(&n);
                }
            }
        }
        out
    }

    fn contains_comment(node: &SyntaxNode) -> bool {
        node.descendants_with_tokens().any(|elem| {
            matches!(
                elem,
                SyntaxElement::Token(t) if t.kind() == SyntaxKind::Comment
            )
        })
    }

    fn push_binary_parts(&self, node: &SyntaxNode, out: &mut Vec<Doc>) {
        for elem in node.children_with_tokens() {
            match elem {
                SyntaxElement::Token(t) if t.kind().is_trivia() => {}
                SyntaxElement::Token(t) => {
                    out.push(Doc::text(t.text().to_string()) + Doc::line_soft());
                }
                SyntaxElement::Node(n)
                    if matches!(
                        n.kind(),
                        SyntaxKind::BinaryExpr | SyntaxKind::ComparisonChainExpr
                    ) =>
                {
                    self.push_binary_parts(&n, out);
                }
                SyntaxElement::Node(n) => out.push(self.node(&n)),
            }
        }
    }

    /// Binary and comparison-chain expressions keep their tight flat spelling
    /// but break after operators when they exceed the configured width.
    fn binary(&self, node: &SyntaxNode) -> Doc {
        if Self::contains_comment(node) {
            return Doc::text(node.text());
        }
        let mut parts = Vec::new();
        self.push_binary_parts(node, &mut parts);
        Doc::group(Doc::nest(self.indent(), Doc::join(Doc::nil(), parts)))
    }

    /// `(expr)`: emit literal parens around the inner expression. If the
    /// parenthesized subtree contains a comment, keep that source verbatim so
    /// expression-level trivia is not lost.
    fn paren(&self, node: &SyntaxNode) -> Doc {
        if Self::contains_comment(node) {
            return Doc::text(node.text());
        }
        let inner = node
            .children()
            .next()
            .map(|n| self.node(&n))
            .unwrap_or_else(Doc::nil);
        Doc::text("(") + inner + Doc::text(")")
    }

    // ===== bracketed sequences =====

    /// Join `items` with `;` and an optional break.
    ///
    /// `flat`: rendered as `a;b;c` (no spaces, like the existing formatter).
    /// `broken`: rendered as
    /// ```text
    /// a;
    /// b;
    /// c
    /// ```
    /// when wrapped in a `Group` and the budget is exceeded.
    fn semicolon_joined(&self, items: Vec<Doc>) -> Doc {
        if items.is_empty() {
            return Doc::nil();
        }
        Doc::join(Doc::text(";") + Doc::line_soft(), items)
    }

    fn tight_semicolon_joined(items: Vec<Doc>) -> Doc {
        Doc::join(Doc::text(";"), items)
    }

    fn delimited_layout(&self, node: &SyntaxNode) -> DelimitedLayout {
        let mut layout = DelimitedLayout::default();
        let mut row = DelimitedRow::default();
        let mut saw_item = false;

        for elem in node.children_with_tokens() {
            match elem {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::Whitespace
                    | SyntaxKind::LParen
                    | SyntaxKind::RParen
                    | SyntaxKind::LBrack
                    | SyntaxKind::RBrack
                    | SyntaxKind::LBrace
                    | SyntaxKind::RBrace
                    | SyntaxKind::Bang => {}
                    SyntaxKind::Semicolon => {
                        if !row.items.is_empty() {
                            row.trailing_semicolon = true;
                        }
                    }
                    SyntaxKind::Newline => {
                        layout.saw_newline = true;
                        if !saw_item {
                            layout.leading_newline = true;
                        }
                        if !row.items.is_empty() {
                            layout.rows.push(row);
                            row = DelimitedRow::default();
                        }
                        layout.close_on_own_line = true;
                    }
                    // Callers avoid comment-bearing delimited forms before
                    // reaching this helper. Keep defensive behavior here.
                    SyntaxKind::Comment => {}
                    _ => {}
                },
                SyntaxElement::Node(n) => {
                    saw_item = true;
                    layout.close_on_own_line = false;
                    if row.trailing_semicolon {
                        // The previous semicolon separated this item from
                        // the preceding item on the same source row.
                        row.trailing_semicolon = false;
                    }
                    row.items.push(self.node(&n));
                }
            }
        }

        if !row.items.is_empty() {
            layout.rows.push(row);
        }
        layout
    }

    fn delimited_source_doc(&self, open: Doc, layout: DelimitedLayout, close: Doc) -> Doc {
        let row_docs = layout.rows.into_iter().map(Self::delimited_row_doc);
        let body = Doc::join(Doc::line_hard(), row_docs);
        let close_doc = if self.config.nlcd || layout.close_on_own_line {
            Doc::line_hard() + close
        } else {
            close
        };
        let inner = if layout.leading_newline {
            Doc::line_hard() + body
        } else {
            body
        };
        Doc::group(open + Doc::nest(self.indent(), inner) + close_doc)
    }

    fn delimited_row_doc(row: DelimitedRow) -> Doc {
        let body = Self::tight_semicolon_joined(row.items);
        if row.trailing_semicolon {
            body + Doc::text(";")
        } else {
            body
        }
    }

    fn list_or_dict(&self, node: &SyntaxNode, dict: bool) -> Doc {
        if Self::contains_comment(node) {
            return Doc::text(node.text());
        }
        let layout = self.delimited_layout(node);
        let mut items: Vec<Doc> = node.children().map(|n| self.node(&n)).collect();
        if items.is_empty() {
            // The empty-dict syntax `(`)` keeps its backtick from
            // `verbatim_concat`. Empty list `()` is also emitted verbatim.
            return Doc::text(if dict { "(`)" } else { "()" });
        }
        // A single-element `ListExpr` is the enlist form `,elem`.
        // Dicts always parenthesize.
        if !dict && items.len() == 1 {
            return Doc::text(",") + items.remove(0);
        }
        if layout.saw_newline {
            return self.delimited_source_doc(Doc::text("("), layout, Doc::text(")"));
        }
        let body = self.semicolon_joined(items);
        Doc::bracket(
            Doc::text("("),
            body,
            Doc::text(")"),
            self.indent(),
            self.config.nlcd,
        )
    }

    fn dict_pair(&self, node: &SyntaxNode) -> Doc {
        // Children: a `Tag` token (key), then `:`, then a value node.
        let mut out = Doc::nil();
        let mut saw_colon = false;
        for elem in node.children_with_tokens() {
            match elem {
                SyntaxElement::Token(t) if t.kind().is_trivia() => {}
                SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::Colon {
                        saw_colon = true;
                    }
                    if t.kind() == SyntaxKind::TagLit {
                        out = out + Doc::text(format!("`{}", t.text().trim_start_matches('`')));
                    } else {
                        out = out + Doc::text(t.text().to_string());
                    }
                }
                SyntaxElement::Node(n) => {
                    out = out + self.node(&n);
                }
            }
        }
        let _ = saw_colon; // sanity; ignored at this stage
        out
    }

    fn arglist_items(&self, node: &SyntaxNode) -> (bool, Vec<SyntaxNode>) {
        let mut leading_bang = false;
        let mut items = Vec::new();
        for elem in node.children_with_tokens() {
            match elem {
                SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::Bang {
                        leading_bang = true;
                    }
                }
                SyntaxElement::Node(n) => items.push(n),
            }
        }
        (leading_bang, items)
    }

    fn arglist_has_separator_or_trivia(&self, node: &SyntaxNode) -> bool {
        node.children_with_tokens().any(|elem| {
            matches!(
                elem,
                SyntaxElement::Token(t)
                    if matches!(
                        t.kind(),
                        SyntaxKind::Semicolon | SyntaxKind::Newline | SyntaxKind::Comment
                    )
            )
        })
    }

    fn arglist(&self, node: &SyntaxNode) -> Doc {
        if Self::contains_comment(node) {
            return Doc::text(node.text());
        }
        // ArgList children: items (nodes) and `;` separators (tokens). We
        // re-emit our own `;` between item nodes, ignoring source tokens
        // except `[`, `!`, and `]` which are structural markers.
        let (leading_bang, item_nodes) = self.arglist_items(node);
        let mut items: Vec<Doc> = item_nodes.iter().map(|n| self.node(n)).collect();
        let open = if leading_bang {
            Doc::text("[!")
        } else {
            Doc::text("[")
        };
        if items.is_empty() {
            return open + Doc::text("]");
        }
        let layout = self.delimited_layout(node);
        if layout.saw_newline {
            return self.delimited_source_doc(open, layout, Doc::text("]"));
        }
        // Single argument: don't introduce a break between the brackets
        // and the argument. `f[somearg]` stays adjacent even when `somearg`
        // is itself a multi-line construct (e.g. a function literal). This
        // matches the long-standing convention from the AST formatter.
        if items.len() == 1 {
            return open + items.remove(0) + Doc::text("]");
        }
        let body = self.semicolon_joined(items);
        Doc::bracket(open, body, Doc::text("]"), self.indent(), self.config.nlcd)
    }

    // ===== postfix / mutating index =====

    fn postfix(&self, node: &SyntaxNode) -> Doc {
        self.postfix_with_space_style(node, true)
    }

    fn postfix_object(&self, node: &SyntaxNode) -> Doc {
        if node.kind() == SyntaxKind::PostfixExpr {
            self.postfix_with_space_style(node, false)
        } else {
            self.node(node)
        }
    }

    fn postfix_with_space_style(&self, node: &SyntaxNode, allow_space_style: bool) -> Doc {
        enum Tail {
            Depth(Doc),
            ArgList(SyntaxNode),
            Arg { node: SyntaxNode, glued: bool },
        }

        // Children in source order:
        //   * the object node (always first)
        //   * optional `@N` depth tokens
        //   * either an `ArgList` node (bracket form) or one or more non-ArgList nodes
        //     (space-call form)
        let mut object: Option<SyntaxNode> = None;
        let mut tails: Vec<Tail> = Vec::new();
        let mut has_explicit_arglist = false;
        let mut saw_trivia_before_next_tail = false;
        for elem in node.children_with_tokens() {
            match elem {
                SyntaxElement::Token(t) if t.kind().is_trivia() => {
                    saw_trivia_before_next_tail = true;
                }
                SyntaxElement::Token(t) if t.kind() == SyntaxKind::AtDepth => {
                    tails.push(Tail::Depth(Doc::text(t.text().to_string())));
                    saw_trivia_before_next_tail = false;
                }
                SyntaxElement::Token(_) => {}
                SyntaxElement::Node(n) if object.is_none() => {
                    object = Some(n);
                    saw_trivia_before_next_tail = false;
                }
                SyntaxElement::Node(n) if n.kind() == SyntaxKind::ArgList => {
                    has_explicit_arglist = true;
                    tails.push(Tail::ArgList(n));
                    saw_trivia_before_next_tail = false;
                }
                SyntaxElement::Node(n) => {
                    tails.push(Tail::Arg {
                        node: n,
                        glued: !saw_trivia_before_next_tail,
                    });
                    saw_trivia_before_next_tail = false;
                }
            }
        }
        let Some(object_node) = object else {
            return Doc::nil();
        };
        let object = self.postfix_object(&object_node);
        if tails.is_empty() {
            // Should be unreachable because a PostfixExpr always has at least one
            // postfix argument or ArgList.
            return object;
        }
        // If the remaining children include an ArgList, treat it as the
        // bracket form. Otherwise synthesize one from the space-call args.
        if has_explicit_arglist {
            // Concat object with all explicit ArgLists in order. This
            // handles `f[1][2][3]`.
            let mut out = object;
            for tail in tails {
                out = match tail {
                    Tail::Depth(doc) => out + doc,
                    Tail::ArgList(arglist) => {
                        if allow_space_style && let Some(arg) = self.single_space_arg(&arglist) {
                            out + Doc::text(" ") + self.node(&arg)
                        } else {
                            out + self.node(&arglist)
                        }
                    }
                    Tail::Arg { node, glued } => {
                        out + self.postfix_arg_separator(&node, glued) + self.node(&node)
                    }
                };
            }
            out
        } else {
            let mut head = object;
            let mut tail_docs: Vec<(SyntaxNode, bool)> = Vec::new();
            for tail in tails {
                match tail {
                    Tail::Depth(doc) => head = head + doc,
                    Tail::ArgList(arglist) => {
                        let (_, items) = self.arglist_items(&arglist);
                        tail_docs.extend(items.into_iter().map(|node| (node, false)));
                    }
                    Tail::Arg { node, glued } => tail_docs.push((node, glued)),
                }
            }
            if tail_docs.is_empty() {
                return head;
            }
            if allow_space_style && tail_docs.len() == 1 {
                let (arg, glued) = tail_docs.into_iter().next().expect("len == 1");
                head + self.postfix_arg_separator(&arg, glued) + self.node(&arg)
            } else if tail_docs.len() == 1 {
                let (arg, _) = tail_docs.into_iter().next().expect("len == 1");
                head + Doc::text("[") + self.node(&arg) + Doc::text("]")
            } else {
                let body =
                    self.semicolon_joined(tail_docs.iter().map(|(n, _)| self.node(n)).collect());
                head + Doc::bracket(
                    Doc::text("["),
                    body,
                    Doc::text("]"),
                    self.indent(),
                    self.config.nlcd,
                )
            }
        }
    }

    fn postfix_arg_separator(&self, arg: &SyntaxNode, glued: bool) -> Doc {
        if glued && Self::can_glue_postfix_arg(arg) {
            Doc::nil()
        } else {
            Doc::text(" ")
        }
    }

    fn can_glue_postfix_arg(node: &SyntaxNode) -> bool {
        matches!(
            node.kind(),
            SyntaxKind::FunctionExpr | SyntaxKind::FStringExpr
        )
    }

    fn single_space_arg(&self, arglist: &SyntaxNode) -> Option<SyntaxNode> {
        let (leading_bang, items) = self.arglist_items(arglist);
        if leading_bang || items.len() != 1 || self.arglist_has_separator_or_trivia(arglist) {
            return None;
        }
        let arg = items.into_iter().next().expect("len == 1");
        self.can_emit_bracket_arg_as_space(&arg).then_some(arg)
    }

    fn can_emit_bracket_arg_as_space(&self, node: &SyntaxNode) -> bool {
        let Some(first) = Self::first_non_trivia_token_kind(node) else {
            return false;
        };
        if !Self::can_start_space_arg_without_forcing_call(first) {
            return false;
        }
        match node.kind() {
            SyntaxKind::LiteralExpr
            | SyntaxKind::VarExpr
            | SyntaxKind::OuterVarExpr
            | SyntaxKind::ParenExpr
            | SyntaxKind::PostfixExpr
            | SyntaxKind::RangeExpr
            | SyntaxKind::CondExpr
            | SyntaxKind::CondDotExpr
            | SyntaxKind::CondChainExpr
            | SyntaxKind::LazyBoolExpr
            | SyntaxKind::WLoopExpr
            | SyntaxKind::NLoopExpr
            | SyntaxKind::BlockExpr
            | SyntaxKind::FunctionExpr
            | SyntaxKind::DictExpr
            | SyntaxKind::FStringExpr
            | SyntaxKind::DebugExpr
            | SyntaxKind::PauseExpr
            | SyntaxKind::SymbolicExpr
            | SyntaxKind::ImportExpr => true,
            SyntaxKind::ListExpr => first == SyntaxKind::LParen,
            SyntaxKind::UnaryExpr => first == SyntaxKind::Hash,
            SyntaxKind::BinaryExpr => Self::is_power_expr(node),
            _ => false,
        }
    }

    fn first_non_trivia_token_kind(node: &SyntaxNode) -> Option<SyntaxKind> {
        node.descendants_with_tokens().find_map(|elem| {
            if let SyntaxElement::Token(t) = elem
                && !t.kind().is_trivia()
            {
                Some(t.kind())
            } else {
                None
            }
        })
    }

    fn can_start_space_arg_without_forcing_call(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::IntLit
                | SyntaxKind::BigIntLit
                | SyntaxKind::FloatLit
                | SyntaxKind::ImagLit
                | SyntaxKind::CharLit
                | SyntaxKind::StringLit
                | SyntaxKind::TagLit
                | SyntaxKind::InfLit
                | SyntaxKind::TrueKw
                | SyntaxKind::FalseKw
                | SyntaxKind::Ident
                | SyntaxKind::Apostrophe
                | SyntaxKind::Hash
                | SyntaxKind::Dollar
                | SyntaxKind::DollarDot
                | SyntaxKind::DollarDollar
                | SyntaxKind::AtDebug
                | SyntaxKind::AtPause
                | SyntaxKind::FString
                | SyntaxKind::AtSymbolic
                | SyntaxKind::AtImport
                | SyntaxKind::LParen
                | SyntaxKind::LBrace
        )
    }

    fn is_power_expr(node: &SyntaxNode) -> bool {
        let mut saw_power = false;
        for elem in node.children_with_tokens() {
            if let SyntaxElement::Token(t) = elem {
                if t.kind().is_trivia() {
                    continue;
                }
                match t.kind() {
                    SyntaxKind::Power | SyntaxKind::PowerDot => saw_power = true,
                    _ => return false,
                }
            }
        }
        saw_power
    }

    fn mutating_index(&self, node: &SyntaxNode) -> Doc {
        // `obj[!args]` or `obj!`. Empty bang index has a compact postfix form.
        let mut out = Doc::nil();
        let mut first = true;
        for child in node.children() {
            if first {
                out = out + self.postfix_object(&child);
                first = false;
            } else if child.kind() == SyntaxKind::ArgList
                && let Some(doc) = self.compact_empty_bang_arglist(&child)
            {
                out = out + doc;
            } else {
                out = out + self.node(&child);
            }
        }
        out
    }

    fn compact_empty_bang_arglist(&self, node: &SyntaxNode) -> Option<Doc> {
        if Self::contains_comment(node) {
            return None;
        }
        let (leading_bang, items) = self.arglist_items(node);
        (leading_bang && items.is_empty()).then(|| Doc::text("!"))
    }

    // ===== assignment =====

    fn assign(&self, node: &SyntaxNode) -> Doc {
        // The CST `*AssignExpr` carries lhs tokens, the `:` (or `+:` etc.),
        // and the rhs node. We re-emit verbatim non-trivia tokens for the
        // lhs/operator and lower the rhs node.
        self.tight_concat(node)
    }

    // ===== function / params =====

    fn function(&self, node: &SyntaxNode) -> Doc {
        // CST shape: `{`, optional ParamList, then a body element stream
        // (statement nodes interleaved with trivia tokens), then `}`. We
        // split into the param doc + the body element stream, and pass
        // the body stream through the trivia-aware sequence walker so
        // comments and blank lines inside the body are preserved.
        let mut param_doc: Option<Doc> = None;
        let mut body_elems: Vec<SyntaxElement> = Vec::new();
        let mut prefix = String::new();
        let mut saw_param_or_open = false;
        for elem in node.children_with_tokens() {
            match &elem {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::LBrace => {
                        saw_param_or_open = true;
                    }
                    SyntaxKind::RBrace => {}
                    _ if saw_param_or_open => body_elems.push(elem),
                    _ => {
                        if !t.kind().is_trivia() {
                            prefix.push_str(t.text());
                        }
                    }
                },
                SyntaxElement::Node(n) => {
                    if n.kind() == SyntaxKind::ParamList && param_doc.is_none() {
                        param_doc = Some(self.node(n));
                        saw_param_or_open = true;
                    } else {
                        body_elems.push(elem);
                    }
                }
            }
        }
        let params = param_doc.unwrap_or_else(Doc::nil);
        // Count statements and retain the only statement for inline formatting.
        let stmt_count = body_elems
            .iter()
            .filter(|e| matches!(e, SyntaxElement::Node(_)))
            .count();
        let body_has_trivia = body_elems.iter().any(
            |e| matches!(e, SyntaxElement::Token(t) if matches!(t.kind(), SyntaxKind::Comment)),
        );
        let open = Doc::text(format!("{prefix}{{"));
        if self.is_inline_like() {
            let body = self.lower_stmt_sequence(body_elems);
            return open + params + body + Doc::text("}");
        }
        if stmt_count == 0 && !body_has_trivia {
            return open + params + Doc::text("}");
        }
        // Single-statement, no comment trivia: keep inline `{stmt}` if it
        // fits the configured width.
        if stmt_count == 1 && !body_has_trivia {
            let stmt = body_elems
                .into_iter()
                .find_map(|e| match e {
                    SyntaxElement::Node(n) => Some(self.node(&n)),
                    _ => None,
                })
                .expect("stmt_count == 1");
            let opening = open.clone() + params.clone();
            return Doc::group(
                opening
                    + Doc::nest(self.indent(), Doc::line_soft() + stmt)
                    + if self.config.nlcd {
                        Doc::line_soft() + Doc::text("}")
                    } else {
                        Doc::text("}")
                    },
            );
        }
        // Multi-statement function body (or body containing comments).
        // Opening `{`, optional params, newline, indented body, newline,
        // closing `}`. Hard newlines ensure the body never flattens.
        let body = self.lower_stmt_sequence(body_elems);
        let opening = open + params;
        Doc::group(
            opening
                + Doc::nest(self.indent(), Doc::line_hard() + body)
                + if self.config.nlcd {
                    Doc::line_hard() + Doc::text("}")
                } else {
                    Doc::text("}")
                },
        )
    }

    fn param_list(&self, node: &SyntaxNode) -> Doc {
        let items: Vec<Doc> = node.children().map(|n| self.node(&n)).collect();
        if items.is_empty() {
            return Doc::text("[]");
        }
        let body = self.semicolon_joined(items);
        Doc::bracket(
            Doc::text("["),
            body,
            Doc::text("]"),
            self.indent(),
            self.config.nlcd,
        )
    }

    // ===== control forms =====

    fn control_form(&self, node: &SyntaxNode) -> Doc {
        // CST shape: sigil tokens (e.g. `$`, `$.`, `$$`, `W`, `N`, `B`),
        // then `[`, then a body element stream (statement nodes
        // interleaved with `;` separators, trivia tokens, and comments),
        // then `]`. We collect the sigil from leading non-trivia tokens,
        // the body elements between `[` and `]`, and feed the body
        // through the trivia-aware sequence walker for comment +
        // blank-line preservation.
        let mut sigil = String::new();
        let mut body_elems: Vec<SyntaxElement> = Vec::new();
        let mut seen_lbrack = false;
        for elem in node.children_with_tokens() {
            match &elem {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::LBrack => {
                        seen_lbrack = true;
                    }
                    SyntaxKind::RBrack => {}

                    // Ignore trivia before the opening bracket. The surrounding statement
                    // sequence regenerates it, and it is not part of the sigil string.
                    _ if !seen_lbrack => {
                        if !t.kind().is_trivia() {
                            sigil.push_str(t.text());
                        }
                    }
                    _ => body_elems.push(elem),
                },
                SyntaxElement::Node(_) if seen_lbrack => body_elems.push(elem),
                SyntaxElement::Node(_) => {}
            }
        }
        if node.kind() == SyntaxKind::BlockExpr {
            sigil.clear();
            if Self::block_needs_legacy_head_in_function_body(node) {
                sigil.push('B');
            }
        }
        let open = Doc::text(format!("{sigil}["));
        let stmt_count = body_elems
            .iter()
            .filter(|e| matches!(e, SyntaxElement::Node(_)))
            .count();
        let has_comment = body_elems.iter().any(
            |e| matches!(e, SyntaxElement::Token(t) if matches!(t.kind(), SyntaxKind::Comment)),
        );
        if stmt_count == 0 && !has_comment {
            return open + Doc::text("]");
        }
        let body = self.lower_stmt_sequence(body_elems);
        if self.is_inline_like() {
            return open + body + Doc::text("]");
        }
        // The head (first statement) sits directly after `[`; subsequent
        // statements live on indented continuation lines via Nest. The
        // `Group` lets the renderer collapse single-statement forms to one
        // line when they fit.
        let close = if self.config.nlcd {
            Doc::line_hard() + Doc::text("]")
        } else {
            Doc::text("]")
        };
        // For multi-statement bodies the first stmt should still go on the
        // header line, so we don't indent it. lower_stmt_sequence already
        // emits LineHards between subsequent stmts; wrapping the whole
        // body in Nest only indents those continuation lines (Nest applies
        // to embedded newlines, not the first character).
        Doc::group(open + Doc::nest(self.indent(), body) + close)
    }

    fn block_needs_legacy_head_in_function_body(node: &SyntaxNode) -> bool {
        let Some(parent) = node.parent() else {
            return false;
        };
        if parent.kind() != SyntaxKind::FunctionExpr {
            return false;
        }
        let mut saw_param_list = false;
        let mut first_child_node: Option<SyntaxNode> = None;
        for child in parent.children() {
            if child.kind() == SyntaxKind::ParamList {
                saw_param_list = true;
            }
            if first_child_node.is_none() {
                first_child_node = Some(child);
            }
        }
        !saw_param_list
            && first_child_node
                .is_some_and(|first| first.index_in_parent() == node.index_in_parent())
    }

    // ===== @-keywords =====

    fn at_keyword_expr(&self, node: &SyntaxNode) -> Doc {
        // CST shape: `@x` token (a single AtSomething), then a body node.
        // Emit `@x` + " " + body (or `@x` + body in inline-like mode).
        let mut keyword = Doc::nil();
        let mut body = Doc::nil();
        for elem in node.children_with_tokens() {
            match elem {
                SyntaxElement::Token(t) if t.kind().is_trivia() => {}
                SyntaxElement::Token(t) => {
                    keyword = keyword + Doc::text(t.text().to_string());
                }
                SyntaxElement::Node(n) => {
                    body = body + self.node(&n);
                }
            }
        }
        if body.is_nil() {
            keyword
        } else if self.is_inline_like() {
            keyword + body
        } else {
            keyword + Doc::text(" ") + body
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::render::render;
    use super::*;
    use crate::cst::SyntaxNode;
    use crate::frontend::Frontend;

    fn fmt(src: &str, width: usize) -> String {
        let frontend = Frontend::default();
        let (_, green) = frontend.parse_with_cst(src).expect("parse");
        let root = SyntaxNode::new_root(green);
        let cfg = FormatConfig {
            max_width: width,
            ..FormatConfig::default()
        };
        let doc = lower(&root, &cfg);
        render(&doc, width)
    }

    #[test]
    fn simple_literal_round_trip() {
        assert_eq!(fmt("1", 80), "1");
        assert_eq!(fmt("foo", 80), "foo");
        assert_eq!(fmt("\"hi\"", 80), "\"hi\"");
        assert_eq!(fmt("\"a\"", 80), "\"a\"");
        assert_eq!(fmt("\"\\u{1f980}\"", 80), "\"\\u{1f980}\"");
    }

    #[test]
    fn import_keeps_a_tight_literal_specifier() {
        assert_eq!(fmt("@i \"module.wq\"", 80), "@i\"module.wq\"");
        assert_eq!(fmt("@i @l\"raw\\path.wq\"", 80), "@i@l\"raw\\path.wq\"");
    }

    #[test]
    fn fstring_formats_interpolation_expr() {
        assert_eq!(fmt(r#"@f"{ 1 + 2 }""#, 80), r#"@f"{1+2}""#);
    }

    #[test]
    fn fstring_formats_dynamic_spec_expr() {
        assert_eq!(
            fmt(r#"@f"{[>{ width + 1 }] value }""#, 80),
            r#"@f"{[>{width+1}]value}""#
        );
    }

    #[test]
    fn fstring_formats_interpolation_control_form_inline() {
        assert_eq!(fmt(r#"@f"{ $[x; 1; 0] }""#, 80), r#"@f"{$[x;1;0]}""#);
    }

    #[test]
    fn fstring_preserves_commented_interpolation_verbatim() {
        assert_eq!(
            fmt(r#"@f"{1 /*inner*/ + 2}""#, 80),
            r#"@f"{1 /*inner*/ + 2}""#
        );
    }

    #[test]
    fn fstring_formats_dynamic_spec_expr_with_quoted_brace() {
        assert_eq!(
            fmt(r##"@f"{[>{"}"}] value}""##, 80),
            r##"@f"{[>{"}"}]value}""##
        );
    }

    #[test]
    fn binary_glues_tightly() {
        assert_eq!(fmt("1+2", 80), "1+2");
        assert_eq!(fmt("1 + 2", 80), "1+2");
    }

    #[test]
    fn binary_wraps_when_width_exceeded() {
        assert_eq!(fmt("1111+2222+3333", 10), "1111+\n  2222+\n  3333");
    }

    #[test]
    fn comparison_chain_wraps_when_width_exceeded() {
        assert_eq!(fmt("1111<2222<3333", 10), "1111<\n  2222<\n  3333");
    }

    #[test]
    fn assign_is_tight() {
        assert_eq!(fmt("x:1", 80), "x:1");
        assert_eq!(fmt("x +: 1", 80), "x+:1");
    }

    #[test]
    fn dict_unpack_patterns() {
        assert_eq!(
            fmt("(`a; `b:renamed): @i \"module.wq\"", 80),
            "(`a;`b:renamed):@i\"module.wq\""
        );
        assert_eq!(
            fmt("(`api: (`start; `stop); `pair: (x; y)): module", 80),
            "(`api:(`start;`stop);`pair:(x;y)):module"
        );
    }

    #[test]
    fn paren_preserved() {
        assert_eq!(fmt("(1+2)*3", 80), "(1+2)*3");
    }

    #[test]
    fn paren_preserves_inner_comment() {
        assert_eq!(fmt("(1 /*inner*/ + 2)", 80), "(1 /*inner*/ + 2)");
    }

    #[test]
    fn list_emits_with_semicolons() {
        assert_eq!(fmt("(1; 2; 3)", 80), "(1;2;3)");
    }

    #[test]
    fn list_preserves_source_rows() {
        assert_eq!(fmt("x:(1;2;3;\n   4;5;6)", 80), "x:(1;2;3;\n  4;5;6)");
        assert_eq!(fmt("x:((1;2)\n   (3;4))", 80), "x:((1;2)\n  (3;4))");
    }

    #[test]
    fn empty_list_and_dict() {
        assert_eq!(fmt("()", 80), "()");
        assert_eq!(fmt("(`)", 80), "(`)");
    }

    #[test]
    fn arglist_normalizes_separators() {
        assert_eq!(fmt("f[1; 2; 3]", 80), "f[1;2;3]");
    }

    #[test]
    fn arglist_preserves_source_rows() {
        assert_eq!(
            fmt("f[a;b;\n  `x:1;\n  `y:2]", 80),
            "f[a;b;\n  `x:1;\n  `y:2]"
        );
        assert_eq!(fmt("f[\n  a;\n  b\n]", 80), "f[\n  a;\n  b\n]");
    }

    #[test]
    fn lazy_bool_forms_preserve_their_sigil() {
        assert_eq!(fmt("A[T; F]", 80), "A[T;F]");
        assert_eq!(fmt("O[F; raise \"boom\"]", 80), "O[F;raise \"boom\"]");
        let nested = fmt("$[A[#token>1;token[-1]=\"/\"];1;0]", 80);
        assert_eq!(nested, "$[A[#token>1;token[-1]=\"/\"];1;0]");
        assert_eq!(fmt(&nested, 80), nested);
    }

    #[test]
    fn bare_block_is_canonical() {
        assert_eq!(fmt("[1]", 80), "[1]");
        assert_eq!(fmt("B[1]", 80), "[1]");
        assert_eq!(fmt("[1; 2; 3]", 80), "[1;2;3]");
        assert_eq!(fmt("B[1; 2; 3]", 80), "[1;2;3]");
        assert_eq!(fmt("[1\n2; 3]", 80), "[1\n  2;3]");
    }

    #[test]
    fn implicit_function_body_block_keeps_legacy_head() {
        assert_eq!(fmt("{B[x]}", 80), "{B[x]}");
        assert_eq!(fmt("{B[x; y]}", 80), "{B[x;y]}");
        assert_eq!(fmt("{[a]B[x]}", 80), "{[a][x]}");
        assert_eq!(fmt("{x;B[y]}", 80), "{\n  x;[y]}");
    }

    #[test]
    fn statement_sequences_preserve_same_line_semicolon_runs() {
        assert_eq!(fmt("{a:1;b:2\nc:3}", 80), "{\n  a:1;b:2\n  c:3}");
        assert_eq!(fmt("N[10;i:0;j:1]", 80), "N[10;i:0;j:1]");
        assert_eq!(fmt("N[10\ni:0;j:1]", 80), "N[10\n  i:0;j:1]");
    }

    #[test]
    fn statement_sequence_drops_semicolon_after_multiline_statement() {
        assert_eq!(fmt("$[T;1\nF;2\n3];1", 80), "$[T;1\n  F;2\n  3]\n1");
    }

    #[test]
    fn wrapped_semicolon_runs_are_idempotent() {
        let once = fmt("{alpha:1111;beta:2222;gamma:3333}", 18);
        assert_eq!(fmt(&once, 18), once);
    }

    #[test]
    fn space_call_stays_postfix() {
        // The space-call `floor sqrt x` parses as nested Postfix; keep the
        // clean postfix surface syntax instead of adding brackets.
        assert_eq!(fmt("floor sqrt x", 80), "floor sqrt x");
    }

    #[test]
    fn glued_function_and_fstring_postfix_stays_glued() {
        assert_eq!(fmt("xs|map{x+1}", 80), "xs|map{x+1}");
        assert_eq!(fmt("xs|map {x+1}", 80), "xs|map{x+1}");
        assert_eq!(fmt(r#"echo@f"{x}""#, 80), r#"echo@f"{x}""#);
    }

    #[test]
    fn single_arg_bracket_call_uses_postfix_when_safe() {
        assert_eq!(fmt("f[x]", 80), "f x");
        assert_eq!(fmt("f[1]", 80), "f 1");
        assert_eq!(fmt("f[\"x\"]", 80), "f \"x\"");
        assert_eq!(fmt("f[{x}]", 80), "f {x}");
        assert_eq!(fmt("f[x^2]", 80), "f x^2");
        assert_eq!(fmt("f[x..3]", 80), "f x..3");
        assert_eq!(fmt("f[(1;2)]", 80), "f (1;2)");
    }

    #[test]
    fn bracket_call_stays_bracketed_when_space_would_reparse_differently() {
        assert_eq!(fmt("f[x+1]", 80), "f[x+1]");
        assert_eq!(fmt("f[x*2]", 80), "f[x*2]");
        assert_eq!(fmt("f[x=1]", 80), "f[x=1]");
        assert_eq!(fmt("f[-x]", 80), "f[-x]");
        assert_eq!(fmt("f[~x]", 80), "f[~x]");
        assert_eq!(fmt("f[1;2]", 80), "f[1;2]");
    }

    #[test]
    fn nested_postfix_keeps_target_grouping() {
        assert_eq!(fmt("f[g[3]]", 80), "f g 3");
        assert_eq!(fmt("f[1][2]", 80), "f[1] 2");
        assert_eq!(fmt("f[1][2][3]", 80), "f[1][2] 3");
    }

    #[test]
    fn empty_mutating_index_prefers_bare_bang() {
        assert_eq!(fmt("xs[!]", 80), "xs!");
        assert_eq!(fmt("xs[!]:15", 80), "xs!:15");
        assert_eq!(fmt("xs!", 80), "xs!");
        assert_eq!(fmt("xs!:15", 80), "xs!:15");
        assert_eq!(fmt("xs[!1]", 80), "xs[!1]");
        assert_eq!(fmt("xs[!/*pop*/]", 80), "xs[!/*pop*/]");
    }

    #[test]
    fn function_one_line() {
        let cfg = FormatConfig {
            oneline: true,
            ..FormatConfig::default()
        };
        let frontend = Frontend::default();
        let (_, g) = frontend.parse_with_cst("{[x;y]x+y}").unwrap();
        let root = SyntaxNode::new_root(g);
        let doc = lower(&root, &cfg);
        assert_eq!(render(&doc, 80), "{[x;y]x+y}");
    }

    #[test]
    fn ref_default_function_preserves_prefix() {
        assert_eq!(fmt("'{[]a}", 80), "'{[]a}");
    }

    #[test]
    fn function_multi_line() {
        let out = fmt("{[x;y]\n  a:1\n  a+x+y}", 80);
        // Expected layout: open brace + params, newline, indented body,
        // closing brace immediately after last stmt.
        assert!(out.starts_with("{[x;y]"), "got: {out:?}");
        assert!(out.contains("\n  a:1"), "got: {out:?}");
        assert!(out.ends_with('}'), "got: {out:?}");
    }

    #[test]
    fn conditional_one_line() {
        let cfg = FormatConfig {
            oneline: true,
            ..FormatConfig::default()
        };
        let frontend = Frontend::default();
        let (_, g) = frontend.parse_with_cst("$[c;t;f]").unwrap();
        let root = SyntaxNode::new_root(g);
        let doc = lower(&root, &cfg);
        assert_eq!(render(&doc, 80), "$[c;t;f]");
    }

    #[test]
    fn pipe_tight() {
        assert_eq!(fmt("xs|sum", 80), "xs|sum");
    }
}
