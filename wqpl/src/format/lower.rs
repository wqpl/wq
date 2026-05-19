//! CST → [`Doc`] lowering.
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
//! * Space-call form `f x y` is rewritten to bracket-call `f[x[y]]`.
//! * Inside `[...]` / `(...)` / `(`...`)` / `S(...)` the contents are joined by
//!   `;` (no whitespace) when flat, by `;\n  ` when broken.
//! * Function bodies and control-form bodies break across newlines, with
//!   children indented by [`FormatConfig::indent_size`].
//! * Binary, unary, range, and assignment operators glue tightly (no
//!   surrounding whitespace).
//!
//! Width-aware breaking is applied at separator-joined constructs (argument
//! lists, list/dict literals, set literals): if the flat form exceeds the
//! configured width, the renderer breaks on the `Line` between separators.

use super::FormatConfig;
use super::doc::Doc;
use crate::cst::{SyntaxElement, SyntaxKind, SyntaxNode};

pub(super) fn lower(root: &SyntaxNode, config: &FormatConfig) -> Doc {
    LowerCtx { config }.node(root)
}

struct LowerCtx<'a> {
    config: &'a FormatConfig,
}

impl<'a> LowerCtx<'a> {
    fn indent(&self) -> i32 {
        self.config.indent_size as i32
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
    /// * Two or more `Newline` tokens between consecutive elements constitute a
    ///   *blank line*: the next element is preceded by an extra newline via
    ///   [`Doc::blank`].
    /// * `Whitespace` tokens are dropped (the formatter regenerates them).
    ///
    /// In `one_line_wizard` mode the whole sequence collapses to a single
    /// line; comments are dropped entirely in that mode since there is no
    /// place to put them without breaking the one-line invariant.
    fn lower_stmt_sequence<I>(&self, iter: I) -> Doc
    where
        I: IntoIterator<Item = SyntaxElement>,
    {
        if self.config.one_line_wizard {
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
        let mut out = Doc::nil();
        let mut newlines_since_payload = 0u32;
        let mut first = true;
        for elem in iter {
            match elem {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::Whitespace => {}
                    SyntaxKind::Newline => {
                        newlines_since_payload = newlines_since_payload.saturating_add(1);
                    }
                    SyntaxKind::Comment => {
                        let text = t.text().to_string();
                        if newlines_since_payload == 0 && !first {
                            // Trailing on the previous statement's line.
                            out = out + Doc::text(format!(" {text}"));
                        } else {
                            // Standalone comment line.
                            if !first {
                                out = out
                                    + if newlines_since_payload >= 2 {
                                        Doc::line_hard() + Doc::blank()
                                    } else {
                                        Doc::line_hard()
                                    };
                            }
                            out = out + Doc::text(text);
                            first = false;
                        }
                        newlines_since_payload = 0;
                    }
                    // Other tokens at statement-sequence level are
                    // unexpected (they would be inside the statement
                    // nodes), but we ignore them defensively rather than
                    // panicking.
                    _ => {}
                },
                SyntaxElement::Node(n) => {
                    let stmt = self.node(&n);
                    if !first {
                        out = out
                            + if newlines_since_payload >= 2 {
                                Doc::line_hard() + Doc::blank()
                            } else {
                                Doc::line_hard()
                            };
                    }
                    out = out + stmt;
                    first = false;
                    newlines_since_payload = 0;
                }
            }
        }
        out
    }

    fn node(&self, node: &SyntaxNode) -> Doc {
        match node.kind() {
            // Trivial — never reached at the top level, only inside the
            // tree where the parent handles them.
            SyntaxKind::Root => self.root(node),

            // Atoms.
            SyntaxKind::LiteralExpr
            | SyntaxKind::VarExpr
            | SyntaxKind::OuterVarExpr
            | SyntaxKind::EllipsisExpr
            | SyntaxKind::BreakExpr
            | SyntaxKind::ContinueExpr
            | SyntaxKind::FStringExpr => self.verbatim_concat(node),

            SyntaxKind::ParenExpr => self.paren(node),
            SyntaxKind::ListExpr => self.list_or_dict(node, /* dict = */ false),
            SyntaxKind::DictExpr => self.list_or_dict(node, /* dict = */ true),
            SyntaxKind::DictPair => self.dict_pair(node),
            SyntaxKind::SetExpr => self.set(node),

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
            SyntaxKind::WLoopExpr | SyntaxKind::NLoopExpr | SyntaxKind::BlockExpr => {
                self.control_form(node)
            }

            SyntaxKind::ReturnExpr => self.at_keyword_expr(node),
            SyntaxKind::AssertExpr | SyntaxKind::DebugExpr | SyntaxKind::TryExpr => {
                self.at_keyword_expr(node)
            }
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

    /// Same as [`Self::tight_concat`]. Kept distinct so binary lowering can
    /// later wrap in `Doc::group(...)` for width-aware breaking without
    /// affecting other tight concatenators.
    fn binary(&self, node: &SyntaxNode) -> Doc {
        self.tight_concat(node)
    }

    /// `(expr)`: emit literal parens around the inner expression. Comments
    /// inside are preserved verbatim (TODO Phase 4G).
    fn paren(&self, node: &SyntaxNode) -> Doc {
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

    fn list_or_dict(&self, node: &SyntaxNode, dict: bool) -> Doc {
        let mut items: Vec<Doc> = node.children().map(|n| self.node(&n)).collect();
        if items.is_empty() {
            // The empty-dict syntax `(`)` keeps its backtick from
            // `verbatim_concat`. Empty list `()` is also emitted verbatim.
            return Doc::text(if dict { "(`)" } else { "()" });
        }
        // A single-element `ListExpr` is the enlist form `,elem` — keep
        // that semantics (a single-element list is *not* the same value
        // as the element alone). Dicts always parenthesize.
        if !dict && items.len() == 1 {
            return Doc::text(",") + items.remove(0);
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

    fn set(&self, node: &SyntaxNode) -> Doc {
        let items: Vec<Doc> = node.children().map(|n| self.node(&n)).collect();
        if items.is_empty() {
            return Doc::text("S()");
        }
        let body = self.semicolon_joined(items);
        Doc::bracket(
            Doc::text("S("),
            body,
            Doc::text(")"),
            self.indent(),
            self.config.nlcd,
        )
    }

    fn arglist(&self, node: &SyntaxNode) -> Doc {
        // ArgList children: items (nodes) and `;` separators (tokens). We
        // re-emit our own `;` between item nodes, ignoring source tokens
        // except `[`, `!`, and `]` which are structural markers.
        let mut leading_bang = false;
        let mut items: Vec<Doc> = Vec::new();
        for elem in node.children_with_tokens() {
            match elem {
                SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::Bang {
                        leading_bang = true;
                    }
                    // All other tokens (brackets, separators, whitespace) are
                    // re-emitted from scratch — skip.
                }
                SyntaxElement::Node(n) => items.push(self.node(&n)),
            }
        }
        let open = if leading_bang {
            Doc::text("[!")
        } else {
            Doc::text("[")
        };
        if items.is_empty() {
            return open + Doc::text("]");
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
        enum Tail {
            Depth(Doc),
            ArgList(Doc),
            Arg(Doc),
        }

        // Children in source order:
        //   * the object node (always first)
        //   * optional `@N` depth tokens
        //   * either an `ArgList` node (bracket form) or one or more non-ArgList nodes
        //     (space-call form)
        //
        // We normalize both forms to `obj[args]`.
        let mut object: Option<Doc> = None;
        let mut tails: Vec<Tail> = Vec::new();
        let mut has_explicit_arglist = false;
        for elem in node.children_with_tokens() {
            match elem {
                SyntaxElement::Token(t) if t.kind().is_trivia() => {}
                SyntaxElement::Token(t) if t.kind() == SyntaxKind::AtDepth => {
                    tails.push(Tail::Depth(Doc::text(t.text().to_string())));
                }
                SyntaxElement::Token(_) => {}
                SyntaxElement::Node(n) if object.is_none() => {
                    object = Some(self.node(&n));
                }
                SyntaxElement::Node(n) if n.kind() == SyntaxKind::ArgList => {
                    has_explicit_arglist = true;
                    tails.push(Tail::ArgList(self.node(&n)));
                }
                SyntaxElement::Node(n) => tails.push(Tail::Arg(self.node(&n))),
            }
        }
        let Some(object) = object else {
            return Doc::nil();
        };
        if tails.is_empty() {
            // Should be unreachable — a PostfixExpr always has at least one
            // postfix argument or ArgList — but render the object alone if
            // we somehow get here.
            return object;
        }
        // If the remaining children include an ArgList, treat it as the
        // bracket form. Otherwise synthesize one from the space-call args.
        if has_explicit_arglist {
            // Concat object with all explicit ArgLists in order. This
            // handles `f[1][2][3]` correctly.
            let mut out = object;
            for tail in tails {
                out = out
                    + match tail {
                        Tail::Depth(doc) | Tail::ArgList(doc) | Tail::Arg(doc) => doc,
                    };
            }
            out
        } else {
            let mut head = object;
            let mut tail_docs: Vec<Doc> = Vec::new();
            for tail in tails {
                match tail {
                    Tail::Depth(doc) => head = head + doc,
                    Tail::ArgList(doc) | Tail::Arg(doc) => tail_docs.push(doc),
                }
            }
            // Space-call form: wrap the space-args in synthetic brackets.
            // Single-arg form stays tight — `f[multilinearg]` keeps `f[`
            // and `]` adjacent to the argument even when the argument
            // breaks across lines.
            if tail_docs.is_empty() {
                return head;
            }
            if tail_docs.len() == 1 {
                let arg = tail_docs.into_iter().next().expect("len == 1");
                head + Doc::text("[") + arg + Doc::text("]")
            } else {
                let body = self.semicolon_joined(tail_docs);
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

    fn mutating_index(&self, node: &SyntaxNode) -> Doc {
        // `obj[!args]`. CST shape: [obj_node, ArgList_with_bang_token]. The
        // ArgList lowering already prefixes `!` when it sees the bang
        // token, so we can just lower normally.
        let mut out = Doc::nil();
        for child in node.children() {
            out = out + self.node(&child);
        }
        out
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
        let mut saw_param_or_open = false;
        for elem in node.children_with_tokens() {
            match &elem {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::LBrace => {
                        saw_param_or_open = true;
                    }
                    SyntaxKind::RBrace => {}
                    _ if saw_param_or_open => body_elems.push(elem),
                    _ => {} // trivia outside the braces (shouldn't happen)
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
        // Count actual statements (node elements) and remember the single
        // statement if exactly one — for the "inline body if it fits"
        // optimization.
        let stmt_count = body_elems
            .iter()
            .filter(|e| matches!(e, SyntaxElement::Node(_)))
            .count();
        let body_has_trivia = body_elems.iter().any(
            |e| matches!(e, SyntaxElement::Token(t) if matches!(t.kind(), SyntaxKind::Comment)),
        );
        if self.config.one_line_wizard {
            let body = self.lower_stmt_sequence(body_elems);
            return Doc::text("{") + params + body + Doc::text("}");
        }
        if stmt_count == 0 && !body_has_trivia {
            return Doc::text("{") + params + Doc::text("}");
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
            let opening = Doc::text("{") + params.clone();
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
        let opening = Doc::text("{") + params;
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
                    // Trivia (whitespace, newlines, comments) *before* the
                    // opening bracket is source noise — it gets
                    // regenerated by the surrounding stmt sequence and
                    // must not become part of the sigil string.
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
        if self.config.one_line_wizard {
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

    // ===== @-keywords =====

    fn at_keyword_expr(&self, node: &SyntaxNode) -> Doc {
        // CST shape: `@x` token (a single AtSomething), then a body node.
        // Emit `@x` + " " + body (or `@x` + body in one_line_wizard mode).
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
        } else if self.config.one_line_wizard {
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
    use crate::session::Session;

    fn fmt(src: &str, width: usize) -> String {
        let s = Session::new();
        let (_, green) = s.parse_with_cst(src).expect("parse");
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
    }

    #[test]
    fn binary_glues_tightly() {
        assert_eq!(fmt("1+2", 80), "1+2");
        assert_eq!(fmt("1 + 2", 80), "1+2");
    }

    #[test]
    fn assign_is_tight() {
        assert_eq!(fmt("x:1", 80), "x:1");
        assert_eq!(fmt("x +: 1", 80), "x+:1");
    }

    #[test]
    fn paren_preserved() {
        assert_eq!(fmt("(1+2)*3", 80), "(1+2)*3");
    }

    #[test]
    fn list_emits_with_semicolons() {
        assert_eq!(fmt("(1; 2; 3)", 80), "(1;2;3)");
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
    fn space_call_becomes_bracket_call() {
        // The space-call `floor sqrt x` parses as nested Postfix; the
        // formatter normalizes to the bracket form.
        assert_eq!(fmt("floor sqrt x", 80), "floor[sqrt[x]]");
    }

    #[test]
    fn function_one_line() {
        let cfg = FormatConfig {
            one_line_wizard: true,
            ..FormatConfig::default()
        };
        let s = Session::new();
        let (_, g) = s.parse_with_cst("{[x;y]x+y}").unwrap();
        let root = SyntaxNode::new_root(g);
        let doc = lower(&root, &cfg);
        assert_eq!(render(&doc, 80), "{[x;y]x+y}");
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
            one_line_wizard: true,
            ..FormatConfig::default()
        };
        let s = Session::new();
        let (_, g) = s.parse_with_cst("$[c;t;f]").unwrap();
        let root = SyntaxNode::new_root(g);
        let doc = lower(&root, &cfg);
        assert_eq!(render(&doc, 80), "$[c;t;f]");
    }

    #[test]
    fn pipe_tight() {
        assert_eq!(fmt("xs|sum", 80), "xs|sum");
    }
}
