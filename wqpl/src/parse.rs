pub(crate) mod fold;
pub(crate) mod resolve;

use std::collections::HashMap;
use std::sync::Arc;

use crate::astnode::{AstNode, AstSpan, BinaryOperator, FStringPart, Parameter, UnaryOperator};
use crate::cas::{cas_binary_expr, cas_symbolic_call_expr, cas_unary_expr};
use crate::cst::{
    Checkpoint, GreenNode, GreenNodeBuilder, SyntaxKind, SyntaxNode, syntax_kind_of_token,
};
use crate::lex::Lexer;
use crate::token::{Token, TokenType};
use crate::value::cas::{CasConst, CasOp};
use crate::value::{IntoWqValue, Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

/// A bookmark for a parse construct that wants both a CST wrap and an AST
/// span. Produced by [`Parser::cst_open`]; consumed by
/// [`Parser::cst_close_with_span`] or [`Parser::cst_close`].
///
/// `Copy` so it can be reused across multiple iterations of a left-recursive
/// loop (e.g. `parse_pipe`'s repeated `||` wraps) without re-allocating.
#[derive(Clone, Copy)]
struct PendingNode {
    cp: Option<Checkpoint>,
    byte_start: usize,
}

#[derive(Clone, Copy)]
struct DirtyByteRange {
    old_start: usize,
    old_end: usize,
    new_start: usize,
    new_end: usize,
}

struct ReusePlan {
    dirty: DirtyByteRange,
    previous_statements: HashMap<usize, GreenNode>,
}

struct ReusedSequenceItem {
    ast: AstNode,
    span: (usize, usize),
    stmt_spans: Vec<(usize, usize)>,
    fn_spans: Vec<Vec<(usize, usize)>>,
}

impl ReusePlan {
    fn old_offset_for_new(&self, new_offset: usize) -> Option<usize> {
        if new_offset < self.dirty.new_start {
            return Some(new_offset);
        }
        if new_offset < self.dirty.new_end {
            return None;
        }

        let old_len = self.dirty.old_end.saturating_sub(self.dirty.old_start);
        let new_len = self.dirty.new_end.saturating_sub(self.dirty.new_start);
        if new_len >= old_len {
            new_offset.checked_sub(new_len - old_len)
        } else {
            new_offset.checked_add(old_len - new_len)
        }
    }

    fn candidate_for_new_offset(&self, new_offset: usize) -> Option<&GreenNode> {
        let old_offset = self.old_offset_for_new(new_offset)?;
        self.previous_statements.get(&old_offset)
    }
}

pub(crate) struct Parser {
    tokens: Vec<Token>,
    current: usize,
    source: String,
    builtins: crate::builtins::Builtins,
    // Optional global source context for accurate error locations when parsing a snippet within a
    // larger file When present, errors will display using this source with line/column
    // adjusted by `line_base`/`col_base`
    global_source: Option<String>,
    // Number of lines before the start of this snippet in the global source
    line_base: usize,
    // Column offset at the start of this snippet (usually 0 when starting at a line boundary).
    // Applied only for tokens on the first line of the snippet.
    col_base: usize,
    // Base byte offset of the snippet within the global source
    base_offset: usize,
    // Optional source file path / label for error rendering
    source_path: Option<String>,
    // Byte spans for statements parsed at the current (top-level) scope
    stmt_spans: Vec<(usize, usize)>,
    // Function body top-level statement spans, in function encounter order
    fn_spans: Vec<Vec<(usize, usize)>>,
    // Stack of span collectors for the current function context.
    // Nested statement spans are tracked separately from the function-body top-level spans.
    fn_span_stack: Vec<Vec<(usize, usize)>>,
    // Bracket nesting depth for error-recovery sync (N[...], W[...], $[...], (a;b), etc.)
    bracket_depth: usize,
    // Set when parsing hits an EOF error at the top level.
    // Callers that need to distinguish "incomplete input" from "success" can inspect this after
    // `parse()` returns `Ok`.
    eof_error: Option<WqError>,

    // ===== CST building (Phase 2B) =====
    // The full lexer output, including `Comment` tokens that `tokens` filters
    // out. Used to flush trivia into the green tree at the same byte
    // positions the lexer produced.
    raw_tokens: Vec<Token>,
    // Index of the next raw token waiting to be flushed to the CST.
    raw_idx: usize,
    // Byte cursor into `source` used to synthesize `Whitespace` tokens for
    // any gap between consecutive lexer-produced tokens. Always equal to
    // `raw_tokens[raw_idx-1].byte_end` (or 0 before the first flush).
    raw_cursor_byte: usize,
    // Optional green-tree builder. `None` when the caller did not request a
    // CST; in that case all `cst_*` helpers are no-ops, and the parser runs
    // exactly as before. The Root node is opened in `parse()` and closed in
    // `take_cst()`; callers must call `take_cst()` exactly once after
    // `parse()` returns Ok.
    cst: Option<GreenNodeBuilder>,
    // Statement-level subtree reuse plan for incremental CST parses.
    reuse: Option<ReusePlan>,
}

impl Parser {
    pub(crate) fn new(tokens: Vec<Token>, source: String) -> Self {
        Self::new_with_builtins(tokens, source, crate::builtins::Builtins::new())
    }

    pub(crate) fn new_with_builtins(
        tokens: Vec<Token>,
        source: String,
        builtins: crate::builtins::Builtins,
    ) -> Self {
        // Keep the original (comment-including) token list for CST flushing.
        // The parser proper continues to operate on the comment-filtered view.
        let raw_tokens = tokens.clone();
        let tokens: Vec<Token> = tokens
            .into_iter()
            .filter(|t| !matches!(t.token_type, TokenType::Comment(_)))
            .collect();
        Parser {
            tokens,
            current: 0,
            source,
            builtins,
            stmt_spans: Vec::new(),
            fn_spans: Vec::new(),
            fn_span_stack: Vec::new(),
            global_source: None,
            line_base: 0,
            col_base: 0,
            base_offset: 0,
            source_path: None,
            bracket_depth: 0,
            eof_error: None,
            raw_tokens,
            raw_idx: 0,
            raw_cursor_byte: 0,
            cst: None,
            reuse: None,
        }
    }

    pub(crate) fn set_source_path(&mut self, path: String) {
        self.source_path = Some(path);
    }

    pub(crate) fn new_with_ctx(
        tokens: Vec<Token>,
        source: String,
        global_source: Option<String>,
        base_offset: usize,
        builtins: crate::builtins::Builtins,
    ) -> Self {
        let mut p = Parser::new_with_builtins(tokens, source, builtins);
        p.apply_ctx(global_source, base_offset);
        p
    }

    fn apply_ctx(&mut self, global_source: Option<String>, base_offset: usize) {
        if let Some(gs) = &global_source {
            let base = base_offset.min(gs.len());
            // Count lines before the offset in the global source
            let line_base = gs[..base].bytes().filter(|b| *b == b'\n').count();
            // Column offset at the snippet start (chars since last newline)
            let col_base = if base == 0 {
                0
            } else {
                match gs[..base].rfind('\n') {
                    Some(i) => gs[i + 1..base].chars().count(),
                    None => gs[..base].chars().count(),
                }
            };
            self.global_source = global_source;
            self.line_base = line_base;
            self.col_base = col_base;
            self.base_offset = base_offset;
        }
    }

    // helpers ====================================================================================

    fn current_token(&self) -> Option<&Token> {
        self.tokens.get(self.current)
    }

    fn last_consumed_byte_end(&self) -> usize {
        self.current
            .checked_sub(1)
            .and_then(|idx| self.tokens.get(idx))
            .map(|tok| tok.byte_end)
            .unwrap_or(0)
    }

    fn peek_token(&self) -> Option<&Token> {
        self.tokens.get(self.current + 1)
    }

    fn peek_next_real_token(&self) -> Option<&Token> {
        let mut offset = 1;
        while let Some(tok) = self.tokens.get(self.current + offset) {
            match tok.token_type {
                TokenType::Newline | TokenType::Comment(_) => offset += 1,
                _ => return Some(tok),
            }
        }
        None
    }

    /// Like [`Self::peek_next_real_token`] but starts at `self.current`
    /// itself, so it answers "what is the next non-trivia token from here,
    /// without advancing?".
    ///
    /// Used by left-recursive parse loops that want to test the next real
    /// token before deciding to consume trivia. The advance-then-rewind
    /// pattern (mutate `self.current`, then reset on failure) is wrong with
    /// CST building enabled, because each `advance()` permanently emits
    /// the trivia into the green tree.
    fn peek_real_token_from_here(&self) -> Option<&Token> {
        let mut offset = 0;
        while let Some(tok) = self.tokens.get(self.current + offset) {
            match tok.token_type {
                TokenType::Newline | TokenType::Comment(_) => offset += 1,
                _ => return Some(tok),
            }
        }
        None
    }

    fn is_terminator(tt: &TokenType) -> bool {
        matches!(
            tt,
            TokenType::Semicolon
                | TokenType::LeftBracket
                | TokenType::RightBracket
                | TokenType::RightParen
                | TokenType::RightBrace
                | TokenType::Pipe
                | TokenType::PipeDot
                | TokenType::PipePipe
                | TokenType::PipePipeDot
                | TokenType::Comma
                | TokenType::Eof
        )
    }

    fn ends_optional_probe_operand(tt: &TokenType) -> bool {
        matches!(
            tt,
            TokenType::Semicolon
                | TokenType::RightBracket
                | TokenType::RightParen
                | TokenType::RightBrace
                | TokenType::Newline
                | TokenType::Pipe
                | TokenType::PipeDot
                | TokenType::PipePipe
                | TokenType::PipePipeDot
                | TokenType::Comma
                | TokenType::Eof
        )
    }

    fn current_token_ends_optional_probe_operand(&self) -> bool {
        let mut offset = 0;
        while let Some(tok) = self.tokens.get(self.current + offset) {
            match tok.token_type {
                TokenType::Comment(_) => offset += 1,
                _ => return Self::ends_optional_probe_operand(&tok.token_type),
            }
        }
        true
    }

    fn token_after_current_ends_optional_probe_operand(&self) -> bool {
        let mut offset = 1;
        while let Some(tok) = self.tokens.get(self.current + offset) {
            match tok.token_type {
                TokenType::Comment(_) => offset += 1,
                _ => return Self::ends_optional_probe_operand(&tok.token_type),
            }
        }
        true
    }

    fn advance(&mut self) -> Option<&Token> {
        if self.current < self.tokens.len() {
            // Flush trivia + this token into the CST before advancing the
            // AST cursor. When CST building is disabled this is a no-op.
            let target_start = self.tokens[self.current].byte_start;
            let target_end = self.tokens[self.current].byte_end;
            self.cst_flush_through(target_start, target_end);
            let tok = &self.tokens[self.current];
            self.current += 1;
            Some(tok)
        } else {
            None
        }
    }

    // ===== CST helpers =====
    //
    // Each helper is a no-op when `cst` is `None`, so the parser can
    // unconditionally sprinkle calls without any branch in the hot path of
    // CST-disabled callers. The Root frame is the responsibility of
    // [`Self::parse`] / [`Self::take_cst`].

    /// Turn on green-tree building. Idempotent: a second call clears any
    /// previously-buffered builder state, which is what callers that re-use
    /// a parser would expect.
    pub(crate) fn enable_cst(&mut self) {
        self.cst = Some(GreenNodeBuilder::new());
        self.raw_idx = 0;
        self.raw_cursor_byte = 0;
        self.reuse = None;
    }

    pub(crate) fn enable_cst_with_cache(
        &mut self,
        previous: &GreenNode,
        old_start: usize,
        old_end: usize,
        new_start: usize,
        new_end: usize,
    ) {
        self.cst = Some(GreenNodeBuilder::with_cache(64));
        self.raw_idx = 0;
        self.raw_cursor_byte = 0;

        let mut previous_statements = HashMap::new();
        let root = SyntaxNode::new_root(previous.clone());
        Self::collect_reusable_statement_nodes(&root, &mut previous_statements);
        self.reuse = Some(ReusePlan {
            dirty: DirtyByteRange {
                old_start,
                old_end,
                new_start,
                new_end,
            },
            previous_statements,
        });
    }

    /// Finalize and take the green tree. Returns `None` if CST building was
    /// not enabled. Must be called exactly once, after [`Self::parse`] has
    /// returned.
    pub(crate) fn take_cst(&mut self) -> Option<GreenNode> {
        let mut builder = self.cst.take()?;
        // Flush any trivia that the parser never reached (e.g. trailing
        // comments after the last statement, or everything after an EOF
        // recovery break). This guarantees the green tree covers every byte
        // of `self.source`.
        Self::cst_flush_remaining(
            &mut builder,
            &self.source,
            &self.raw_tokens,
            &mut self.raw_idx,
            &mut self.raw_cursor_byte,
        );
        // Close the Root frame opened by `parse()`.
        builder.finish_node();
        Some(builder.finish())
    }

    /// Push every raw token (with synthesized whitespace) up to and including
    /// the one whose byte range matches `target_start..target_end`. The next
    /// flush continues from the byte directly after the target.
    fn cst_flush_through(&mut self, target_start: usize, target_end: usize) {
        let Some(builder) = self.cst.as_mut() else {
            return;
        };
        while self.raw_idx < self.raw_tokens.len() {
            let r = &self.raw_tokens[self.raw_idx];
            if matches!(r.token_type, TokenType::Eof) {
                break;
            }
            if r.byte_start > target_start {
                // The raw stream is past the target. This should not happen
                // in practice (token streams from the same lexer are
                // monotonic and `tokens` is a subset of `raw_tokens`), but we
                // bail safely instead of asserting.
                break;
            }
            if r.byte_start > self.raw_cursor_byte {
                let gap = &self.source[self.raw_cursor_byte..r.byte_start];
                builder.token(SyntaxKind::Whitespace, gap);
            }
            let text = &self.source[r.byte_start..r.byte_end];
            builder.token(syntax_kind_of_token(&r.token_type), text);
            self.raw_cursor_byte = r.byte_end;
            self.raw_idx += 1;
            if r.byte_start == target_start && r.byte_end == target_end {
                break;
            }
        }
    }

    /// Flush any remaining raw tokens (and trailing whitespace) into the
    /// builder. Free function (taking explicit refs) so that
    /// [`Self::take_cst`] can call it after `take()`-ing the builder out of
    /// `self`.
    fn cst_flush_remaining(
        builder: &mut GreenNodeBuilder,
        source: &str,
        raw_tokens: &[Token],
        raw_idx: &mut usize,
        raw_cursor_byte: &mut usize,
    ) {
        while *raw_idx < raw_tokens.len() {
            let r = &raw_tokens[*raw_idx];
            if matches!(r.token_type, TokenType::Eof) {
                break;
            }
            if r.byte_start > *raw_cursor_byte {
                let gap = &source[*raw_cursor_byte..r.byte_start];
                builder.token(SyntaxKind::Whitespace, gap);
            }
            let text = &source[r.byte_start..r.byte_end];
            builder.token(syntax_kind_of_token(&r.token_type), text);
            *raw_cursor_byte = r.byte_end;
            *raw_idx += 1;
        }
        if *raw_cursor_byte < source.len() {
            let trailing = &source[*raw_cursor_byte..];
            builder.token(SyntaxKind::Whitespace, trailing);
            *raw_cursor_byte = source.len();
        }
    }

    /// Open a CST node. No-op when CST building is disabled.
    fn cst_start_node(&mut self, kind: SyntaxKind) {
        if let Some(b) = self.cst.as_mut() {
            b.start_node(kind);
        }
    }

    /// Close the most recently opened CST node. No-op when CST building is
    /// disabled. Calling this without a matching `cst_start_node` is a logic
    /// bug that will panic in [`Self::take_cst`] via the underlying builder.
    fn cst_finish_node(&mut self) {
        if let Some(b) = self.cst.as_mut() {
            b.finish_node();
        }
    }

    /// Take a checkpoint at the current position. Returns `None` when CST
    /// building is disabled, so callers can pair it with
    /// [`Self::cst_start_node_at`] without branching on whether CST is on.
    fn cst_checkpoint(&mut self) -> Option<Checkpoint> {
        self.cst.as_mut().map(|b| b.checkpoint())
    }

    /// Wrap children since the checkpoint into a node of `kind`. No-op when
    /// either CST building is disabled or the checkpoint is `None`.
    fn cst_start_node_at(&mut self, cp: Option<Checkpoint>, kind: SyntaxKind) {
        if let (Some(b), Some(cp)) = (self.cst.as_mut(), cp) {
            b.start_node_at(cp, kind);
        }
    }

    /// Byte position of the next token to be consumed. When the cursor is
    /// past the end of the token stream this falls back to the end of the
    /// source, which is the byte position EOF sits at by lexer convention.
    fn current_byte_start(&self) -> usize {
        self.current_token()
            .map(|t| t.byte_start)
            .unwrap_or_else(|| self.source.len())
    }

    /// Open a structural bookmark for a parse construct.
    ///
    /// Pairs a CST [`Checkpoint`] (so the green-tree wrap can be applied
    /// retroactively once we know the kind) with the byte position the
    /// construct started at. Pair with [`Self::cst_close_with_span`] to
    /// finalize both the CST wrap and the AST span in one call.
    ///
    /// This is the workhorse helper that replaces the old
    /// `start_idx` / `span_from_start` / `header_start_idx = current-2` /
    /// `last_consumed_byte_end` patterns that used to be sprinkled through
    /// every node-producing parse function. The pair always agrees, so the
    /// AST span exactly matches the byte range of the CST subtree.
    fn cst_open(&mut self) -> PendingNode {
        PendingNode {
            cp: self.cst_checkpoint(),
            byte_start: self.current_byte_start(),
        }
    }

    /// Close a bookmark from [`Self::cst_open`]: wrap the consumed tokens in
    /// the CST as `kind`, and return the corresponding AST span. The end
    /// byte is taken from the most recently consumed token.
    fn cst_close_with_span(&mut self, pending: PendingNode, kind: SyntaxKind) -> AstSpan {
        self.cst_start_node_at(pending.cp, kind);
        self.cst_finish_node();
        Some((pending.byte_start, self.last_consumed_byte_end()))
    }

    /// Like [`Self::cst_close_with_span`] but discards the span (used by
    /// nodes whose AST variant does not carry a span field, e.g.
    /// [`AstNode::BinaryOp`]). Provided for parity so callers do not have to
    /// reach for [`Self::cst_start_node_at`] directly.
    fn cst_close(&mut self, pending: PendingNode, kind: SyntaxKind) {
        let _ = self.cst_close_with_span(pending, kind);
    }

    fn span_from_byte_start(&self, byte_start: usize) -> AstSpan {
        Some((byte_start, self.last_consumed_byte_end()))
    }

    fn merge_spans(a: AstSpan, b: AstSpan) -> AstSpan {
        match (a, b) {
            (Some((a_start, a_end)), Some((b_start, b_end))) => {
                Some((a_start.min(b_start), a_end.max(b_end)))
            }
            (Some(span), None) | (None, Some(span)) => Some(span),
            (None, None) => None,
        }
    }

    fn span_for_items(items: &[AstNode]) -> AstSpan {
        let start = items.first().and_then(AstNode::span);
        let end = items.last().and_then(AstNode::span);
        Self::merge_spans(start, end)
    }

    fn is_reuse_sequence_context(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::Root
                | SyntaxKind::Block
                | SyntaxKind::FunctionExpr
                | SyntaxKind::CondExpr
                | SyntaxKind::CondDotExpr
                | SyntaxKind::CondChainExpr
                | SyntaxKind::WLoopExpr
                | SyntaxKind::NLoopExpr
                | SyntaxKind::BlockExpr
        )
    }

    fn collect_reusable_statement_nodes(node: &SyntaxNode, out: &mut HashMap<usize, GreenNode>) {
        if Self::is_reuse_sequence_context(node.kind()) {
            for child in node.children() {
                if node.kind() == SyntaxKind::FunctionExpr && child.kind() == SyntaxKind::ParamList
                {
                    continue;
                }
                out.entry(child.abs_offset() as usize)
                    .or_insert_with(|| child.green().clone());
            }
        }

        for child in node.children() {
            Self::collect_reusable_statement_nodes(&child, out);
        }
    }

    fn parse_reused_sequence_item(&self, text: &str, offset: usize) -> Option<ReusedSequenceItem> {
        let tokens = Lexer::new(text).tokenize().ok()?;
        let mut parser = Parser::new_with_builtins(tokens, text.to_string(), self.builtins.clone());
        if let Some(path) = &self.source_path {
            parser.set_source_path(path.clone());
        }
        let mut ast = parser.parse().ok()?;
        if parser.eof_error().is_some() {
            return None;
        }
        Self::offset_spans(&mut ast, offset);

        let mut stmt_spans = parser.stmt_spans.clone();
        for (start, end) in &mut stmt_spans {
            *start += offset;
            *end += offset;
        }
        let mut fn_spans = parser.fn_spans;
        for spans in &mut fn_spans {
            for (start, end) in spans {
                *start += offset;
                *end += offset;
            }
        }

        Some(ReusedSequenceItem {
            ast,
            span: (offset, offset + text.len()),
            stmt_spans,
            fn_spans,
        })
    }

    fn skip_reused_bytes(&mut self, byte_end: usize) {
        while let Some(tok) = self.tokens.get(self.current) {
            if tok.byte_end > byte_end {
                break;
            }
            self.current += 1;
        }

        while let Some(tok) = self.raw_tokens.get(self.raw_idx) {
            if matches!(tok.token_type, TokenType::Eof) || tok.byte_end > byte_end {
                break;
            }
            self.raw_idx += 1;
        }

        self.raw_cursor_byte = byte_end;
    }

    fn is_sequence_boundary_token(tt: &TokenType) -> bool {
        matches!(
            tt,
            TokenType::Semicolon
                | TokenType::Newline
                | TokenType::Comment(_)
                | TokenType::RightBrace
                | TokenType::RightBracket
                | TokenType::Eof
        )
    }

    fn can_reuse_sequence_item_until(&self, byte_end: usize) -> bool {
        let mut idx = self.raw_idx;
        while let Some(tok) = self.raw_tokens.get(idx) {
            if matches!(tok.token_type, TokenType::Eof) {
                return self.source[byte_end..].chars().all(char::is_whitespace);
            }
            if tok.byte_end <= byte_end {
                idx += 1;
                continue;
            }
            if tok.byte_start < byte_end {
                return false;
            }
            if !self.source[byte_end..tok.byte_start]
                .chars()
                .all(char::is_whitespace)
            {
                return false;
            }
            return Self::is_sequence_boundary_token(&tok.token_type);
        }
        self.source[byte_end..].chars().all(char::is_whitespace)
    }

    fn merge_reused_stmt_spans(&mut self, stmt_spans: Vec<(usize, usize)>) {
        if let Some(cur) = self.fn_span_stack.last_mut() {
            cur.extend(stmt_spans);
        } else {
            self.stmt_spans.extend(stmt_spans);
        }
    }

    fn merge_reused_fn_spans(&mut self, fn_spans: Vec<Vec<(usize, usize)>>) {
        self.fn_spans.extend(fn_spans);
    }

    fn try_reuse_sequence_item(&mut self) -> Option<ReusedSequenceItem> {
        let start = self.current_byte_start();
        let candidate = self
            .reuse
            .as_ref()?
            .candidate_for_new_offset(start)?
            .clone();
        let end = start.checked_add(candidate.text_len() as usize)?;
        let candidate_text = candidate.text();
        if self.source.get(start..end)? != candidate_text {
            return None;
        }
        if !self.can_reuse_sequence_item_until(end) {
            return None;
        }

        let reused = self.parse_reused_sequence_item(&candidate_text, start)?;
        if let Some(builder) = self.cst.as_mut() {
            builder.append_node(candidate);
        }
        self.skip_reused_bytes(end);
        Some(reused)
    }

    fn syntax_err(&self, token: &Token, msg: impl Into<String>) -> WqError {
        let msg = msg.into();
        let (text, abs_start, abs_end) = if let Some(gs) = &self.global_source {
            let start = token.byte_start + self.base_offset;
            let end = token.byte_end + self.base_offset;
            (gs.clone(), start, end)
        } else {
            (self.source.clone(), token.byte_start, token.byte_end)
        };
        let path = self.source_path.as_deref().unwrap_or("?");
        WqError::new(WqErrorType::Syntax)
            .src("parser")
            .msg(msg)
            .span(Some((abs_start, abs_end)))
            .source_ctx(text, path)
    }

    fn eof_error_here(&self, msg: impl Into<String>) -> WqError {
        let msg = msg.into();
        if let Some(tok) = self.current_token() {
            let (text, abs_start, abs_end) = if let Some(gs) = &self.global_source {
                let start = tok.byte_start + self.base_offset;
                let end = tok.byte_end + self.base_offset;
                (gs.clone(), start, end)
            } else {
                (self.source.clone(), tok.byte_start, tok.byte_end)
            };
            let path = self.source_path.as_deref().unwrap_or("?");
            WqError::new(WqErrorType::Eof)
                .src("parser")
                .msg(msg)
                .span(Some((abs_start, abs_end)))
                .source_ctx(text, path)
        } else {
            WqError::new(WqErrorType::Eof).src("parser").msg(msg)
        }
    }

    fn consume(&mut self, expected: TokenType) -> WqResult<()> {
        if let Some(tok) = self.current_token() {
            if std::mem::discriminant(&tok.token_type) == std::mem::discriminant(&expected) {
                self.advance();
                Ok(())
            } else {
                Err(self.syntax_err(
                    tok,
                    format!("expected {:?}, found {:?}", expected, tok.token_type),
                ))
            }
        } else {
            Err(self.eof_error_here("unexpected end of input"))
        }
    }

    #[inline]
    fn is_token(&self, tt: &TokenType) -> bool {
        self.current_token()
            .map(|t| std::mem::discriminant(&t.token_type) == std::mem::discriminant(tt))
            .unwrap_or(false)
    }

    /// Skip trivia tokens.
    /// * allow_nl: if true, skip newline tokens as trivia
    /// * allow_com: if true, skip comments as trivia
    #[inline]
    fn eat_trivia(&mut self, allow_nl: bool, allow_com: bool) -> usize {
        let mut n = 0;
        while let Some(tok) = self.current_token() {
            match tok.token_type {
                TokenType::Newline if allow_nl => {
                    self.advance();
                    n += 1;
                }
                TokenType::Comment(_) if allow_com => {
                    self.advance();
                    n += 1;
                }
                _ => break,
            }
        }
        n
    }

    #[inline]
    fn eat_stmt_separators(&mut self) -> usize {
        let mut n = 0;
        while let Some(TokenType::Semicolon)
        | Some(TokenType::Newline)
        | Some(TokenType::Comment(_)) = self.current_token().map(|t| &t.token_type)
        {
            self.advance();
            n += 1;
        }
        n
    }

    // /// Require a literal semicolon. Comments/newlines may appear around it.
    // /// But only `;` satisfies the requirement.
    // #[inline]
    // fn require_semicolon(&mut self, ctx: &str) -> WqResult<()> {
    //     self.eat_trivia(true, true);
    //     match self.current_token().map(|t| &t.token_type) {
    //         Some(TokenType::Semicolon) => {
    //             self.advance();
    //             Ok(())
    //         }
    //         Some(TokenType::Eof) => {
    //             Err(self.eof_error_here(format!("Unexpected end of input in
    // {ctx}")))         }
    //         Some(tt) => Err(self.syntax_err(
    //             self.current_token().unwrap(),
    //             format!("expected ';' in {ctx}, found {tt:?}"),
    //         )),
    //         None => Err(self.eof_error_here(format!("Unexpected end of input in
    // {ctx}"))),     }
    // }

    #[inline]
    fn require_control_separator(&mut self, ctx: &str) -> WqResult<()> {
        // Prefer a literal semicolon first
        if matches!(
            self.current_token().map(|t| &t.token_type),
            Some(TokenType::Semicolon)
        ) {
            self.advance();
            // eat trailing trivia
            self.eat_trivia(true, true);
            return Ok(());
        }
        // Skip comments
        self.eat_trivia(false, true);
        // Now require at least one newline
        match self.current_token().map(|t| &t.token_type) {
            Some(TokenType::Newline) => {
                // consume >=1 newline
                self.advance();
                // then any additional newlines/comments
                while let Some(TokenType::Newline) | Some(TokenType::Comment(_)) =
                    self.current_token().map(|t| &t.token_type)
                {
                    self.advance();
                }
                Ok(())
            }
            Some(TokenType::Eof) => {
                Err(self.eof_error_here(format!("unexpected end of input in {ctx}")))
            }
            Some(tt) => Err(self.syntax_err(
                self.current_token().unwrap(),
                format!("expected ';' or newline in {ctx}, found {tt:?}"),
            )),
            None => Err(self.eof_error_here(format!("unexpected end of input in {ctx}"))),
        }
    }

    #[inline]
    fn missing_rhs(&self, op_tok: &Token, ctx: &str) -> WqError {
        self.syntax_err(op_tok, format!("expected expression after {ctx}"))
    }

    #[inline]
    fn ensure_rhs(&self, op_tok: &Token, ctx: &str) -> WqResult<()> {
        match self.current_token().map(|t| &t.token_type) {
            Some(TokenType::Eof) | None => Err(self.missing_rhs(op_tok, ctx)),
            _ => Ok(()),
        }
    }

    fn eat_rhs_trivia(&mut self, op_tok: &Token, ctx: &str) -> WqResult<()> {
        loop {
            match self.current_token().map(|t| &t.token_type) {
                Some(TokenType::Newline) => {
                    let Some(next) = self.next_rhs_continuation_token() else {
                        return Err(self.missing_rhs(op_tok, ctx));
                    };
                    if matches!(next.token_type, TokenType::Eof) || next.column <= 1 {
                        return Err(self.missing_rhs(op_tok, ctx));
                    }
                    self.advance();
                }
                Some(TokenType::Comment(_)) => {
                    self.advance();
                }
                _ => break,
            }
        }
        self.ensure_rhs(op_tok, ctx)
    }

    fn next_rhs_continuation_token(&self) -> Option<&Token> {
        let mut offset = 1;
        while let Some(tok) = self.tokens.get(self.current + offset) {
            if matches!(tok.token_type, TokenType::Comment(_) | TokenType::Newline) {
                offset += 1;
                continue;
            }
            return Some(tok);
        }
        None
    }

    fn assignment_op_from_token_type(tt: &TokenType) -> Option<Option<BinaryOperator>> {
        match tt {
            TokenType::Colon => Some(None),
            TokenType::PlusColon => Some(Some(BinaryOperator::Add)),
            TokenType::MinusColon => Some(Some(BinaryOperator::Subtract)),
            TokenType::MultiplyColon => Some(Some(BinaryOperator::Multiply)),
            TokenType::DivideColon => Some(Some(BinaryOperator::Divide)),
            TokenType::DivideDotColon => Some(Some(BinaryOperator::DivideDot)),
            TokenType::ModuloColon => Some(Some(BinaryOperator::Modulo)),
            TokenType::PowerColon => Some(Some(BinaryOperator::Power)),
            TokenType::PowerDotColon => Some(Some(BinaryOperator::PowerDot)),
            TokenType::CommaColon => Some(Some(BinaryOperator::Cat)),
            TokenType::ShlColon => Some(Some(BinaryOperator::Shl)),
            TokenType::ShrColon => Some(Some(BinaryOperator::Shr)),
            TokenType::FloorDivColon => Some(Some(BinaryOperator::FloorDiv)),
            _ => None,
        }
    }

    fn is_pipe_stage_boundary(tt: &TokenType) -> bool {
        matches!(
            tt,
            TokenType::Semicolon
                | TokenType::Newline
                | TokenType::RightBracket
                | TokenType::RightParen
                | TokenType::RightBrace
                | TokenType::Pipe
                | TokenType::PipeDot
                | TokenType::PipePipe
                | TokenType::PipePipeDot
                | TokenType::Comma
                | TokenType::Eof
        )
    }

    fn token_after_current_ends_pipe_stage(&self) -> bool {
        let mut offset = 1;
        while let Some(tok) = self.tokens.get(self.current + offset) {
            if matches!(tok.token_type, TokenType::Comment(_)) {
                offset += 1;
                continue;
            }
            return Self::is_pipe_stage_boundary(&tok.token_type);
        }
        true
    }

    fn is_checkpoint_assignment_target(expr: &AstNode) -> bool {
        matches!(
            expr,
            AstNode::Variable(_, _)
                | AstNode::OuterVariable(_, _)
                | AstNode::Index { .. }
                | AstNode::MutatingIndex { .. }
                | AstNode::Postfix {
                    explicit_call: false,
                    ..
                }
        )
    }

    // program ====================================================================================

    fn synchronize_to_stmt_boundary(&mut self) {
        while let Some(tok) = self.current_token() {
            match tok.token_type {
                TokenType::Semicolon | TokenType::Newline => {
                    // Inside a bracket expression (N[...], W[...], (a;b), etc.)
                    // separators are expression delimiters, not statement
                    // terminators; stop here so the caller can see them.
                    if self.bracket_depth == 0 {
                        self.advance();
                    }
                    return;
                }
                TokenType::RightBrace | TokenType::Eof => return,
                TokenType::RightBracket | TokenType::RightParen => {
                    // Structural boundary: consume it so the outer loop
                    // doesn't get stuck, then stop.
                    self.advance();
                    return;
                }
                _ => {
                    self.advance();
                }
            }
        }
    }

    pub(crate) fn parse(&mut self) -> WqResult<AstNode> {
        // Open the CST Root once at the top of every parse. `take_cst`
        // matches this with `finish_node`. When CST is disabled this is a
        // no-op and the parser behaves identically.
        self.cst_start_node(SyntaxKind::Root);
        let mut statements = Vec::new();
        loop {
            self.eat_stmt_separators();
            match self.current_token().map(|t| &t.token_type) {
                Some(TokenType::Eof) | None => break,
                Some(TokenType::RightBrace) => break,
                Some(TokenType::RightBracket) | Some(TokenType::RightParen) => {
                    // stray closing bracket/paren from a broken nested construct;
                    // consume and continue so we don't spin forever
                    self.advance();
                    continue;
                }
                _ => {
                    let start_idx = self.current;
                    let start_byte = self.current_token().map(|t| t.byte_start).unwrap_or(0);
                    if let Some(reused) = self.try_reuse_sequence_item() {
                        self.merge_reused_stmt_spans(reused.stmt_spans);
                        self.merge_reused_fn_spans(reused.fn_spans);
                        statements.push(reused.ast);
                    } else {
                        // Checkpoint *before* the statement attempt so a parse
                        // failure can retroactively wrap the consumed tokens in
                        // an `ErrorNode`.
                        let stmt_cp = self.cst_checkpoint();
                        match self.parse_statement() {
                            Ok(stmt) => {
                                let end_idx = self.current.saturating_sub(1);
                                if start_idx < self.tokens.len() && end_idx < self.tokens.len() {
                                    self.push_stmt_span_bounds(
                                        self.tokens[start_idx].byte_start,
                                        self.tokens[end_idx].byte_end,
                                    );
                                }
                                statements.push(stmt);
                            }
                            Err(e) => {
                                if e.err_type == crate::wqerror::WqErrorType::Eof {
                                    self.eof_error = Some(e);
                                    break;
                                }
                                self.synchronize_to_stmt_boundary();
                                // Wrap whatever tokens we consumed for this
                                // failed statement (including those skipped by
                                // synchronize_to_stmt_boundary above) in
                                // ErrorNode.
                                self.cst_start_node_at(stmt_cp, SyntaxKind::ErrorNode);
                                self.cst_finish_node();
                                let span = e.span.unwrap_or_else(|| {
                                    let end_byte = self
                                        .current_token()
                                        .map(|t| t.byte_end)
                                        .unwrap_or(start_byte);
                                    (start_byte, end_byte)
                                });
                                statements.push(AstNode::Error(e, Some(span)));
                            }
                        }
                    }
                }
            }
            self.eat_stmt_separators();
        }
        Self::normalize_stmt_spans(&mut self.stmt_spans);
        if statements.len() == 1 {
            Ok(statements.remove(0))
        } else {
            let span = Self::span_for_items(&statements);
            Ok(AstNode::Block(statements, span))
        }
    }

    pub(crate) fn stmt_spans_top(&self) -> &[(usize, usize)] {
        &self.stmt_spans
    }

    /// Returns the EOF error encountered at the top level, if any.
    pub(crate) fn eof_error(&self) -> Option<&WqError> {
        self.eof_error.as_ref()
    }

    pub(crate) fn fn_body_spans_all(&self) -> &Vec<Vec<(usize, usize)>> {
        &self.fn_spans
    }

    #[inline]
    fn push_stmt_span_bounds(&mut self, start: usize, end: usize) {
        if let Some(cur) = self.fn_span_stack.last_mut() {
            cur.push((start, end));
        } else {
            self.stmt_spans.push((start, end));
        }
    }

    fn normalize_stmt_spans(spans: &mut Vec<(usize, usize)>) {
        spans.sort_by(|(a_start, a_end), (b_start, b_end)| {
            a_start.cmp(b_start).then_with(|| b_end.cmp(a_end))
        });
        spans.dedup();
    }

    #[inline]
    fn record_stmt_span_idx(&mut self, start_idx: usize, end_idx: usize) {
        if start_idx < self.tokens.len() && end_idx < self.tokens.len() {
            self.push_stmt_span_bounds(
                self.tokens[start_idx].byte_start,
                self.tokens[end_idx].byte_end,
            );
        }
    }

    // Parse a block and also record per-statement spans (start,end) for debug
    // mapping.
    fn parse_block_with_spans(&mut self) -> WqResult<(AstNode, Vec<(usize, usize)>)> {
        let mut statements = Vec::new();
        let mut spans: Vec<(usize, usize)> = Vec::new();
        loop {
            self.eat_stmt_separators();
            match self.current_token().map(|t| &t.token_type) {
                Some(TokenType::RightBrace) => break,
                Some(TokenType::Eof) | None => {
                    return Err(self.eof_error_here("unexpected end of input in block"));
                }
                Some(TokenType::RightBracket) | Some(TokenType::RightParen) => {
                    self.advance();
                    continue;
                }
                _ => {
                    let start_idx = self.current;
                    let start_byte = self.current_token().map(|t| t.byte_start).unwrap_or(0);
                    if let Some(reused) = self.try_reuse_sequence_item() {
                        spans.push(reused.span);
                        self.merge_reused_stmt_spans(reused.stmt_spans);
                        self.merge_reused_fn_spans(reused.fn_spans);
                        statements.push(reused.ast);
                    } else {
                        match self.parse_statement() {
                            Ok(stmt) => {
                                let end_idx = self.current.saturating_sub(1);
                                if start_idx < self.tokens.len() && end_idx < self.tokens.len() {
                                    let start = self.tokens[start_idx].byte_start;
                                    let end = self.tokens[end_idx].byte_end;
                                    spans.push((start, end));
                                }
                                // Also record into the current function collector if any
                                self.record_stmt_span_idx(start_idx, end_idx);
                                statements.push(stmt);
                            }
                            Err(e) => {
                                if e.err_type == crate::wqerror::WqErrorType::Eof {
                                    return Err(e);
                                }
                                self.synchronize_to_stmt_boundary();
                                let span = e.span.unwrap_or_else(|| {
                                    let end_byte = self
                                        .current_token()
                                        .map(|t| t.byte_end)
                                        .unwrap_or(start_byte);
                                    (start_byte, end_byte)
                                });
                                statements.push(AstNode::Error(e, Some(span)));
                            }
                        }
                    }
                }
            }
            self.eat_stmt_separators();
        }
        // Do not push into fn_spans here; function-level span collection is finalized
        // in parse_function to ensure nested branch statements are included.
        let block = if statements.len() == 1 {
            statements.remove(0)
        } else {
            let span = Self::span_for_items(&statements);
            AstNode::Block(statements, span)
        };
        Ok((block, spans))
    }

    // expr ====================================================================================

    fn parse_statement(&mut self) -> WqResult<AstNode> {
        self.parse_expression()
    }

    fn parse_expression(&mut self) -> WqResult<AstNode> {
        self.parse_assignment()
    }

    // Lower an unpack assignment like (x;y):rhs into a Block of simpler AST nodes.
    fn lower_unpack_assign(
        &mut self,
        items: Vec<AstNode>,
        op: Option<BinaryOperator>,
        value: AstNode,
        colon_tok: &Token,
        span: AstSpan,
    ) -> WqResult<AstNode> {
        if items.is_empty() {
            return Err(self.syntax_err(colon_tok, "invalid unpack assignment target: empty list"));
        }

        self.validate_unpack_targets(&items, colon_tok)?;

        Ok(AstNode::UnpackAssignment {
            lhs: items,
            op,
            rhs: Box::new(value),
            span,
        })
    }

    fn validate_unpack_targets(&mut self, items: &[AstNode], colon_tok: &Token) -> WqResult<()> {
        let mut saw_ellipsis = false;
        for it in items {
            match it {
                AstNode::Variable(nm, _) => {
                    if self.builtins.has_function(nm) {
                        return Err(self
                            .syntax_err(colon_tok, format!("cannot assign to builtin '{}'", nm)));
                    }
                }
                AstNode::Index { .. } => {}
                AstNode::Postfix {
                    explicit_call: false,
                    ..
                } => {}
                AstNode::Ellipsis(_) => {
                    if saw_ellipsis {
                        return Err(
                            self.syntax_err(colon_tok, "invalid unpack assignment: multiple '...'")
                        );
                    }
                    saw_ellipsis = true;
                }
                AstNode::List(inner, _) => {
                    self.validate_unpack_targets(inner, colon_tok)?;
                }
                other => {
                    return Err(self
                        .syntax_err(
                            colon_tok,
                            "invalid unpack assignment target: expected identifier, index assignment target, '_' or '...'",
                        )
                        .attach_note(format!("got {other}")));
                }
            }
        }
        Ok(())
    }

    fn parse_assignment(&mut self) -> WqResult<AstNode> {
        // One bookmark covers both the CST wrap and the AST span. If no
        // assignment operator follows we never close it, the checkpoint
        // simply dies, and we pass `expr` through unchanged.
        let pending = self.cst_open();
        let mut expr = self.parse_pipe()?;
        while let Some(token) = self.current_token() {
            let Some(assign_op) = Self::assignment_op_from_token_type(&token.token_type) else {
                break;
            };

            match expr {
                // Unpack assignment: (x;y):rhs
                AstNode::List(items, _) => {
                    let colon_tok = token.clone();
                    self.advance();
                    self.ensure_rhs(&colon_tok, "assignment operator")?;
                    let value = self.parse_assignment()?;
                    let span = self.cst_close_with_span(pending, SyntaxKind::UnpackAssignExpr);
                    expr = self.lower_unpack_assign(items, assign_op, value, &colon_tok, span)?;
                }
                AstNode::Variable(name, var_span) => {
                    if self.builtins.has_function(&name) {
                        return Err(
                            self.syntax_err(token, format!("cannot assign to builtin '{name}'"))
                        );
                    }
                    let colon_tok = token.clone();
                    self.advance();
                    self.ensure_rhs(&colon_tok, "assignment operator")?;
                    let value = self.parse_assignment()?;
                    let span = self.cst_close_with_span(pending, SyntaxKind::AssignExpr);
                    expr = AstNode::Assignment {
                        name,
                        op: assign_op,
                        value: Box::new(value),
                        span,
                        name_span: var_span,
                    };
                }
                AstNode::OuterVariable(name, var_span) => {
                    if self.builtins.has_function(&name) {
                        return Err(
                            self.syntax_err(token, format!("cannot assign to builtin '{name}'"))
                        );
                    }
                    let colon_tok = token.clone();
                    self.advance();
                    self.ensure_rhs(&colon_tok, "assignment operator")?;
                    let value = self.parse_assignment()?;
                    let span = self.cst_close_with_span(pending, SyntaxKind::OuterAssignExpr);
                    expr = AstNode::OuterAssignment {
                        name,
                        op: assign_op,
                        value: Box::new(value),
                        span,
                        name_span: var_span,
                    };
                }
                AstNode::Index { object, index, .. } => {
                    let colon_tok = token.clone();
                    self.advance();
                    self.ensure_rhs(&colon_tok, "assignment operator")?;
                    let value = self.parse_assignment()?;
                    let span = self.cst_close_with_span(pending, SyntaxKind::IndexAssignExpr);
                    expr = AstNode::IndexAssign {
                        object,
                        index,
                        op: assign_op,
                        value: Box::new(value),
                        span,
                    };
                }
                AstNode::MutatingIndex { object, index, .. } => {
                    let colon_tok = token.clone();
                    self.advance();
                    self.ensure_rhs(&colon_tok, "assignment operator")?;
                    let value = self.parse_assignment()?;
                    let span =
                        self.cst_close_with_span(pending, SyntaxKind::MutatingIndexAssignExpr);
                    expr = AstNode::MutatingIndexAssign {
                        object,
                        index,
                        value: Box::new(value),
                        span,
                    };
                }
                AstNode::Postfix {
                    object,
                    items,
                    explicit_call: false,
                    ..
                } => {
                    let colon_tok = token.clone();
                    self.advance();
                    self.ensure_rhs(&colon_tok, "assignment operator")?;
                    let value = self.parse_assignment()?;
                    let span = self.cst_close_with_span(pending, SyntaxKind::IndexAssignExpr);
                    let index = if items.len() == 1 {
                        Box::new(items.into_iter().next().expect("len == 1"))
                    } else {
                        Box::new(AstNode::List(items, None))
                    };
                    expr = AstNode::IndexAssign {
                        object,
                        index,
                        op: assign_op,
                        value: Box::new(value),
                        span,
                    };
                }
                AstNode::Literal(Value::Tag(tag_name), _tag_span) => {
                    if assign_op.is_some() {
                        return Err(self.syntax_err(
                            token,
                            "augmented assignment is not supported for named arguments",
                        ));
                    }
                    let name = tag_name.to_string();
                    let colon_tok = token.clone();
                    self.advance(); // consume ':'
                    self.ensure_rhs(&colon_tok, "named argument")?;
                    let value = self.parse_assignment()?;
                    let span = self.cst_close_with_span(pending, SyntaxKind::NamedArgExpr);
                    expr = AstNode::NamedArg {
                        name,
                        value: Box::new(value),
                        span,
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_pipe(&mut self) -> WqResult<AstNode> {
        use crate::astnode::PipeKind;
        // The Pipe AST span now covers the whole expression `LHS | RHS` --
        // matching the CST `PipeExpr`. Previously it covered only the
        // operator and RHS; the change makes spans usable for highlighting
        // and diagnostics without a second computation step, and makes the
        // CST/AST consistent.
        //
        // Lookahead trick: with CST building on, every `advance()` is a
        // permanent flush -- we can't rewind. So we peek through trivia
        // *without* advancing to decide whether a pipe follows; only when
        // the answer is yes do we actually consume the trivia and the
        // operator.
        let pending = self.cst_open();
        let mut left = self.parse_comma()?;
        while let Some(token) = self.peek_real_token_from_here().cloned() {
            let kind = match token.token_type {
                TokenType::Pipe => PipeKind::Pipe,
                TokenType::PipeDot => PipeKind::PipeDot,
                TokenType::PipePipe => PipeKind::PipePipe,
                TokenType::PipePipeDot => PipeKind::PipePipeDot,
                _ => break,
            };
            // We've committed: consume any trivia preceding the operator
            // and then the operator itself.
            self.eat_trivia(true, true);
            self.advance();
            let op_name = match kind {
                PipeKind::Pipe => "'|'",
                PipeKind::PipeDot => "'|.'",
                PipeKind::PipePipe => "'||'",
                PipeKind::PipePipeDot => "'||.'",
            };
            self.eat_rhs_trivia(&token, &format!("{op_name} operator"))?;
            let right = self.parse_pipe_rhs_expr()?;
            // Iterating on `||` etc. nests the PipeExpr left-associatively,
            // mirroring the AST. `pending` is `Copy`, safe to reuse.
            let span = self.cst_close_with_span(pending, SyntaxKind::PipeExpr);
            left = AstNode::Pipe {
                input: Box::new(left),
                effect: Box::new(right),
                kind,
                span,
            };
        }
        Ok(left)
    }

    fn parse_comma(&mut self) -> WqResult<AstNode> {
        let pending = self.cst_open();
        let mut items = Vec::new();
        if let Some(token) = self.current_token()
            && token.token_type == TokenType::Comma
        {
            if let Some(next) = self.peek_next_real_token() {
                if Self::is_terminator(&next.token_type) {
                    return self.parse_comparison();
                }
            } else {
                return self.parse_comparison();
            }

            // Leading comma list: ,a,b,c -- produces a `List` AST. The CST
            // wraps it in a `ListExpr` covering everything since `cp`
            // (which captures the leading `,`).
            while let Some(t) = self.current_token() {
                if t.token_type != TokenType::Comma {
                    break;
                }
                if let Some(next) = self.peek_next_real_token() {
                    if Self::is_terminator(&next.token_type) {
                        break;
                    }
                } else {
                    break;
                }
                self.advance();
                self.eat_trivia(true, true);
                let expr = self.parse_comparison()?;
                items.push(expr);
            }
            let span = self.span_from_byte_start(pending.byte_start);
            self.cst_start_node_at(pending.cp, SyntaxKind::ListExpr);
            self.cst_finish_node();
            return Ok(AstNode::List(items, span));
        }
        let first = self.parse_comparison()?;
        let mut items = vec![first];
        while let Some(t) = self.current_token() {
            if t.token_type != TokenType::Comma {
                break;
            }
            if let Some(next) = self.peek_next_real_token() {
                if Self::is_terminator(&next.token_type) {
                    break;
                }
            } else {
                break;
            }
            self.advance(); // eat ','
            self.eat_trivia(true, true);
            let right = self.parse_comparison()?;
            items.push(right);
        }
        if items.len() == 1 {
            return Ok(items.pop().expect("len == 1"));
        }
        // Merge List nodes: [1;2],[3;4] → [1;2;3;4]
        if items.iter().all(|item| matches!(item, AstNode::List(..))) {
            let mut merged = vec![];
            for item in items {
                if let AstNode::List(mut inner, _) = item {
                    merged.append(&mut inner);
                }
            }
            let span = self.span_from_byte_start(pending.byte_start);
            self.cst_start_node_at(pending.cp, SyntaxKind::ListExpr);
            self.cst_finish_node();
            return Ok(AstNode::List(merged, span));
        }
        let span = self.span_from_byte_start(pending.byte_start);
        self.cst_start_node_at(pending.cp, SyntaxKind::BinaryExpr);
        self.cst_finish_node();
        Ok(AstNode::Cat(items, span))
    }

    fn parse_range(&mut self) -> WqResult<AstNode> {
        let pending = self.cst_open();
        let start = self.parse_unary()?;
        if let Some(token) = self.current_token().cloned() {
            let inclusive = match token.token_type {
                TokenType::Range => false,
                TokenType::RangeInclusive => true,
                _ => return Ok(start),
            };
            self.advance();
            self.eat_rhs_trivia(&token, "range operator")?;
            let end = self.parse_unary()?;
            let mut step_node = None;
            if let Some(next_tok) = self.current_token().cloned() {
                match next_tok.token_type {
                    TokenType::Range => {
                        self.advance();
                        self.eat_rhs_trivia(&next_tok, "range step operator")?;
                        let step_expr = self.parse_unary()?;
                        step_node = Some(Box::new(step_expr));
                    }
                    TokenType::RangeInclusive => {
                        return Err(self.syntax_err(&next_tok, "unexpected '..=' for range step"));
                    }
                    _ => {}
                }
            }
            let span = self.span_from_byte_start(pending.byte_start);
            self.cst_start_node_at(pending.cp, SyntaxKind::RangeExpr);
            self.cst_finish_node();
            return Ok(AstNode::Range {
                start: Box::new(start),
                end: Box::new(end),
                step: step_node,
                inclusive,
                span,
            });
        }
        Ok(start)
    }

    fn parse_comparison(&mut self) -> WqResult<AstNode> {
        let pending = self.cst_open();
        let first = self.parse_shift()?;
        let mut rest: Vec<(BinaryOperator, AstNode)> = Vec::new();
        while let Some(token) = self.current_token().cloned() {
            let (op, op_tok) = match token.token_type {
                TokenType::Equal => (BinaryOperator::Equal, token),
                TokenType::EqualDot => (BinaryOperator::EqualDot, token),
                TokenType::NotEqual => (BinaryOperator::NotEqual, token),
                TokenType::NotEqualDot => (BinaryOperator::NotEqualDot, token),
                TokenType::LessThan => (BinaryOperator::Lt, token),
                TokenType::LessThanOrEqual => (BinaryOperator::Lte, token),
                TokenType::GreaterThan => (BinaryOperator::Gt, token),
                TokenType::GreaterThanOrEqual => (BinaryOperator::Gte, token),
                _ => break,
            };
            self.advance();
            self.eat_rhs_trivia(&op_tok, "comparison operator")?;
            let right = self.parse_shift()?;
            rest.push((op, right));
        }
        match rest.len() {
            0 => Ok(first),
            1 => {
                let (op, rhs) = rest.into_iter().next().unwrap();
                let span = self.span_from_byte_start(pending.byte_start);
                self.cst_start_node_at(pending.cp, SyntaxKind::BinaryExpr);
                self.cst_finish_node();
                Ok(AstNode::BinaryOp {
                    left: Box::new(first),
                    operator: op,
                    right: Box::new(rhs),
                    span,
                })
            }
            _ => {
                let span = self.span_from_byte_start(pending.byte_start);
                self.cst_start_node_at(pending.cp, SyntaxKind::ComparisonChainExpr);
                self.cst_finish_node();
                Ok(AstNode::ComparisonChain {
                    first: Box::new(first),
                    rest,
                    span,
                })
            }
        }
    }

    fn parse_shift(&mut self) -> WqResult<AstNode> {
        let cp = self.cst_checkpoint();
        let mut left = self.parse_additive()?;
        while let Some(token) = self.current_token().cloned() {
            let (op, op_tok) = match token.token_type {
                TokenType::Shl => (BinaryOperator::Shl, token),
                TokenType::Shr => (BinaryOperator::Shr, token),
                _ => break,
            };
            self.advance();
            self.eat_rhs_trivia(&op_tok, "binary operator")?;
            let right = self.parse_additive()?;
            let span = Self::merge_spans(left.span(), right.span());
            left = AstNode::BinaryOp {
                left: Box::new(left),
                operator: op,
                right: Box::new(right),
                span,
            };
            self.cst_start_node_at(cp, SyntaxKind::BinaryExpr);
            self.cst_finish_node();
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> WqResult<AstNode> {
        let cp = self.cst_checkpoint();
        let mut left = self.parse_multiplicative()?;
        while let Some(token) = self.current_token().cloned() {
            let (op, op_tok) = match token.token_type {
                TokenType::Plus => (BinaryOperator::Add, token),
                TokenType::Minus => (BinaryOperator::Subtract, token),
                _ => break,
            };
            self.advance();
            self.eat_rhs_trivia(&op_tok, "binary operator")?;
            let right = self.parse_multiplicative()?;
            let span = Self::merge_spans(left.span(), right.span());
            left = AstNode::BinaryOp {
                left: Box::new(left),
                operator: op,
                right: Box::new(right),
                span,
            };
            self.cst_start_node_at(cp, SyntaxKind::BinaryExpr);
            self.cst_finish_node();
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> WqResult<AstNode> {
        let cp = self.cst_checkpoint();
        let mut left = self.parse_range()?;
        while let Some(token) = self.current_token().cloned() {
            let (op, op_tok) = match token.token_type {
                TokenType::Multiply => (BinaryOperator::Multiply, token),
                TokenType::Divide => (BinaryOperator::Divide, token),
                TokenType::DivideDot => (BinaryOperator::DivideDot, token),
                TokenType::Modulo => (BinaryOperator::Modulo, token),

                TokenType::Matmul => (BinaryOperator::Matmul, token),

                TokenType::FloorDiv => (BinaryOperator::FloorDiv, token),

                _ => break,
            };
            self.advance();
            self.eat_rhs_trivia(&op_tok, "binary operator")?;
            let right = self.parse_range()?;
            let span = Self::merge_spans(left.span(), right.span());
            left = AstNode::BinaryOp {
                left: Box::new(left),
                operator: op,
                right: Box::new(right),
                span,
            };
            self.cst_start_node_at(cp, SyntaxKind::BinaryExpr);
            self.cst_finish_node();
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> WqResult<AstNode> {
        let pending = self.cst_open();
        let mut ops: Vec<UnaryOperator> = Vec::new();
        let mut negate_parity = 0u8;
        let mut unary_applied = false;
        while let Some(token) = self.current_token().cloned() {
            if let Some(next) = self.peek_next_real_token() {
                if Self::is_terminator(&next.token_type) {
                    break;
                }
            } else {
                break;
            }

            match token.token_type {
                TokenType::Minus => {
                    self.advance();
                    self.ensure_rhs(&token, "unary operator -")?;
                    negate_parity ^= 1;
                    unary_applied = true;
                }
                TokenType::Sharp => {
                    if negate_parity == 1 {
                        ops.push(UnaryOperator::Negate);
                        negate_parity = 0;
                    }
                    self.advance();
                    self.ensure_rhs(&token, "unary operator #")?;
                    ops.push(UnaryOperator::Count);
                    unary_applied = true;
                }
                TokenType::NotEqual => {
                    if negate_parity == 1 {
                        ops.push(UnaryOperator::Negate);
                        negate_parity = 0;
                    }
                    self.advance();
                    self.ensure_rhs(&token, "unary operator ~")?;
                    ops.push(UnaryOperator::Not);
                    unary_applied = true;
                }
                _ => break,
            }
        }
        let mut node = self.parse_power()?;
        // The unary's AST span covers from the first prefix operator through
        // the end of the operand. Previously this required the operand to
        // carry its own span (so `.span().1` was readable); now we derive
        // it from the byte cursor, which is always well defined.
        let span = if unary_applied {
            Some((pending.byte_start, self.last_consumed_byte_end()))
        } else {
            None
        };
        if negate_parity == 1 {
            node = match node {
                AstNode::Literal(Value::Int(n), _) => AstNode::Literal(Value::Int(-n), span),
                AstNode::Literal(Value::Float(f), _) => AstNode::Literal(Value::Float(-f), span),
                _ => AstNode::UnaryOp {
                    operator: UnaryOperator::Negate,
                    operand: Box::new(node),
                    span,
                },
            };
        }
        while let Some(op) = ops.pop() {
            node = AstNode::UnaryOp {
                operator: op,
                operand: Box::new(node),
                span,
            };
        }
        if unary_applied {
            self.cst_close(pending, SyntaxKind::UnaryExpr);
        }
        Ok(node)
    }

    fn parse_power(&mut self) -> WqResult<AstNode> {
        let cp = self.cst_checkpoint();
        let mut operands = Vec::new();
        let mut operators = Vec::new();
        operands.push(self.parse_postfix()?);
        // slurp all "^ unary" / "^. unary" pairs first
        while let Some(tok) = self.current_token().cloned() {
            let op = match tok.token_type {
                TokenType::Power => BinaryOperator::Power,
                TokenType::PowerDot => BinaryOperator::PowerDot,
                _ => break,
            };
            self.advance();
            self.eat_rhs_trivia(&tok, "binary operator")?;
            operators.push(op);
            operands.push(self.parse_unary()?);
        }
        let chain_applied = !operators.is_empty();
        // fold right: a ^ b ^ c => a ^ (b ^ c)
        let mut it = operands.into_iter().rev();
        let mut acc = it.next().expect("parse_postfix always pushes one operand");
        for (left, op) in it.zip(operators.into_iter().rev()) {
            let span = Self::merge_spans(left.span(), acc.span());
            acc = AstNode::BinaryOp {
                left: Box::new(left),
                operator: op,
                right: Box::new(acc),
                span,
            };
        }
        if chain_applied {
            // Phase 2B keeps the entire `^` chain as one BinaryExpr in the
            // CST. The AST is right-associative; the formatter can reflect
            // that from operator children when needed.
            self.cst_start_node_at(cp, SyntaxKind::BinaryExpr);
            self.cst_finish_node();
        }
        Ok(acc)
    }

    // postfix ====================================================================================

    fn parse_bracket_items(&mut self) -> WqResult<(Vec<AstNode>, bool)> {
        // Accept [] and ;]
        if self.is_token(&TokenType::Semicolon) {
            self.advance();
            self.consume(TokenType::RightBracket)?;
            return Ok((Vec::new(), true));
        }
        if self.is_token(&TokenType::RightBracket) {
            self.advance();
            return Ok((Vec::new(), true));
        }
        if self.is_token(&TokenType::Eof) {
            return Err(self.eof_error_here("unexpected end of input in bracket"));
        }
        let mut items = Vec::new();
        let mut trailing = false;
        loop {
            // trivia allowed before item
            self.eat_trivia(true, true);
            if self.is_token(&TokenType::RightBracket) {
                self.advance();
                break;
            }
            let expr = self.parse_expression()?;
            items.push(expr);
            // after item: either ']' or a required ';'
            self.eat_trivia(false, true);
            if self.is_token(&TokenType::RightBracket) {
                self.advance();
                break;
            }
            // self.require_semicolon("bracket items")?;
            self.require_control_separator("bracket items")?;
            // allow trailing '; ]'
            self.eat_trivia(true, true);
            if self.is_token(&TokenType::RightBracket) {
                trailing = true;
                self.advance();
                break;
            }
            if self.is_token(&TokenType::Eof) {
                return Err(self.eof_error_here("unexpected end of input in bracket"));
            }
        }
        Ok((items, trailing))
    }

    fn parse_lazy_bool_form(
        &mut self,
        keyword: &str,
        operator: BinaryOperator,
        header_start_byte: usize,
        cp_outer: Option<Checkpoint>,
    ) -> WqResult<AstNode> {
        let cp_args = self.cst_checkpoint();
        self.advance();
        let (items, _) = self.parse_bracket_items()?;
        self.cst_start_node_at(cp_args, SyntaxKind::ArgList);
        self.cst_finish_node();
        let end_token = self.tokens[self.current.saturating_sub(1)].clone();
        let end_byte = end_token.byte_end;
        if items.len() < 2 {
            return Err(self.syntax_err(
                &end_token,
                format!("{keyword}[...] requires at least two expressions"),
            ));
        }

        let item_count = items.len();
        let mut iter = items.into_iter();
        let mut acc = iter.next().expect("len checked above");
        for (idx, right) in iter.enumerate() {
            let span = if idx + 2 == item_count {
                Some((header_start_byte, end_byte))
            } else {
                Self::merge_spans(acc.span(), right.span())
            };
            acc = AstNode::BinaryOp {
                left: Box::new(acc),
                operator,
                right: Box::new(right),
                span,
            };
        }

        self.cst_start_node_at(cp_outer, SyntaxKind::PostfixExpr);
        self.cst_finish_node();
        Ok(acc)
    }

    fn is_operator_node(expr: &AstNode) -> bool {
        let name_str = match expr {
            AstNode::Variable(name, _) => Some(name.as_str()),
            AstNode::Postfix { object, .. } => {
                if let AstNode::Variable(name, _) = &**object {
                    Some(name.as_str())
                } else {
                    None
                }
            }
            _ => None,
        };
        #[rustfmt::skip]
        let res = matches!(
            name_str,
            Some("+" | "-" | "*" | "/" | "/." | "/%" | "%" |
                "^" | "^." | "**" | "=" | "~" | "<" | "<=" |
                ">" | ">=" | "<<" | ">>" | "," | "#"
            )
        );
        res
    }

    fn parse_postfix_internal<F>(&mut self, mut parse_arg: F) -> WqResult<AstNode>
    where
        F: FnMut(&mut Self) -> WqResult<AstNode>,
    {
        let start_byte = self.current_token().map(|t| t.byte_start).unwrap_or(0);
        // Outer checkpoint: covers `expr [args]` / `expr arg`. Each iteration
        // that adds a postfix wraps everything since this checkpoint.
        let cp_outer = self.cst_checkpoint();
        let mut expr = self.parse_primary()?;
        let mut pending_depth: Option<(i64, Token)> = None;
        loop {
            // ignore comments but do not skip newlines.
            while matches!(
                self.current_token().map(|t| &t.token_type),
                Some(TokenType::Comment(_))
            ) {
                self.advance();
            }

            if let Some(token) = self.current_token().cloned()
                && let TokenType::AtDepth(depth) = token.token_type
            {
                if pending_depth.is_some() {
                    return Err(self.syntax_err(&token, "duplicate depth modifier"));
                }
                pending_depth = Some((depth, token));
                self.advance();
                continue;
            }

            let is_operator = Self::is_operator_node(&expr);

            match self.current_token().map(|t| &t.token_type) {
                Some(TokenType::LeftBracket) => {
                    // Inner checkpoint for `[ ! items ]` so it forms an
                    // ArgList subtree separate from the postfix wrapper.
                    let cp_args = self.cst_checkpoint();
                    self.advance();
                    let is_mutating = self.is_token(&TokenType::Bang);
                    if is_mutating {
                        self.advance(); // consume Bang
                    }
                    let (items, _call_flag) = self.parse_bracket_items()?;
                    self.cst_start_node_at(cp_args, SyntaxKind::ArgList);
                    self.cst_finish_node();
                    let end_byte = self.tokens[self.current.saturating_sub(1)].byte_end;
                    if is_mutating {
                        if let Some((_, token)) = pending_depth {
                            return Err(self.syntax_err(
                                &token,
                                "depth modifier must be followed by a call",
                            ));
                        }
                        let index = if items.len() == 1 {
                            Box::new(items.into_iter().next().expect("len == 1"))
                        } else {
                            Box::new(AstNode::List(items, None))
                        };
                        expr = AstNode::MutatingIndex {
                            object: Box::new(expr),
                            index,
                            span: Some((start_byte, end_byte)),
                        };
                        self.cst_start_node_at(cp_outer, SyntaxKind::MutatingIndexExpr);
                        self.cst_finish_node();
                    } else {
                        expr = AstNode::Postfix {
                            object: Box::new(expr),
                            items,
                            explicit_call: _call_flag,
                            depth: pending_depth.take().map(|(depth, _)| depth),
                            span: Some((start_byte, end_byte)),
                        };
                        self.cst_start_node_at(cp_outer, SyntaxKind::PostfixExpr);
                        self.cst_finish_node();
                    }
                }
                // allow minus as arg ONLY if the object is an operator
                Some(TokenType::Minus) if is_operator => {
                    let arg = parse_arg(self)?;
                    let end_byte = self.tokens[self.current.saturating_sub(1)].byte_end;
                    expr = AstNode::Postfix {
                        object: Box::new(expr),
                        items: vec![arg],
                        explicit_call: false,
                        depth: pending_depth.take().map(|(depth, _)| depth),
                        span: Some((start_byte, end_byte)),
                    };
                    self.cst_start_node_at(cp_outer, SyntaxKind::PostfixExpr);
                    self.cst_finish_node();
                }
                // Call candidates (newline not allowed)
                Some(TokenType::Integer(_))
                | Some(TokenType::BigInteger(_))
                | Some(TokenType::Tag(_))
                | Some(TokenType::Identifier(_))
                | Some(TokenType::Apostrophe)
                // No minus allowed here
                | Some(TokenType::Sharp)
                | Some(TokenType::Dollar)
                | Some(TokenType::DollarDot)
                | Some(TokenType::DollarDollar)
                | Some(TokenType::AtDebug)
                | Some(TokenType::AtPause)
                | Some(TokenType::FormatString(_, _, _))
                | Some(TokenType::AtSymbolic)
                | Some(TokenType::LeftParen) => {
                    let arg = parse_arg(self)?;
                    let end_byte = self.tokens[self.current.saturating_sub(1)].byte_end;
                    expr = AstNode::Postfix {
                        object: Box::new(expr),
                        items: vec![arg],
                        explicit_call: false,
                        depth: pending_depth.take().map(|(depth, _)| depth),
                        span: Some((start_byte, end_byte)),
                    };
                    self.cst_start_node_at(cp_outer, SyntaxKind::PostfixExpr);
                    self.cst_finish_node();
                }
                // definitely fn calls
                Some(TokenType::Float(_))
                | Some(TokenType::Imaginary(_))
                | Some(TokenType::Character(_))
                | Some(TokenType::String(_))
                | Some(TokenType::Inf)
                | Some(TokenType::LeftBrace)
                | Some(TokenType::True)
                | Some(TokenType::False) => {
                    let arg = parse_arg(self)?;
                    let end_byte = self.tokens[self.current.saturating_sub(1)].byte_end;
                    expr = AstNode::Postfix {
                        object: Box::new(expr),
                        items: vec![arg],
                        explicit_call: true,
                        depth: pending_depth.take().map(|(depth, _)| depth),
                        span: Some((start_byte, end_byte)),
                    };
                    self.cst_start_node_at(cp_outer, SyntaxKind::PostfixExpr);
                    self.cst_finish_node();
                }
                _ => break,
            }
        }
        if let Some((_, token)) = pending_depth {
            return Err(self.syntax_err(&token, "depth modifier must be followed by a call"));
        }
        Ok(expr)
    }

    fn parse_postfix(&mut self) -> WqResult<AstNode> {
        self.parse_postfix_internal(Self::parse_range)
    }

    fn parse_pipe_rhs_checkpoint_assignment(&mut self) -> WqResult<AstNode> {
        let pending = self.cst_open();
        let expr = self.parse_postfix_internal(Self::parse_comparison)?;
        let Some(token) = self.current_token().cloned() else {
            return Ok(expr);
        };
        let Some(assign_op) = Self::assignment_op_from_token_type(&token.token_type) else {
            return Ok(expr);
        };
        if !self.token_after_current_ends_pipe_stage() {
            if Self::is_checkpoint_assignment_target(&expr) {
                return Err(self
                    .syntax_err(
                        &token,
                        "pipe assignment stages take no explicit RHS; the pipe value is the RHS",
                    )
                    .attach_note("write `value|name:` to bind the current pipe value")
                    .attach_note("use a block if the pipe stage needs separate assignment logic"));
            }
            return Ok(expr);
        }

        match expr {
            AstNode::Variable(name, var_span) => {
                if self.builtins.has_function(&name) {
                    return Err(
                        self.syntax_err(&token, format!("cannot assign to builtin '{name}'"))
                    );
                }
                self.advance();
                let span = self.cst_close_with_span(pending, SyntaxKind::AssignExpr);
                Ok(AstNode::Assignment {
                    name,
                    op: assign_op,
                    value: Box::new(AstNode::PipeInput),
                    span,
                    name_span: var_span,
                })
            }
            AstNode::OuterVariable(name, var_span) => {
                if self.builtins.has_function(&name) {
                    return Err(
                        self.syntax_err(&token, format!("cannot assign to builtin '{name}'"))
                    );
                }
                self.advance();
                let span = self.cst_close_with_span(pending, SyntaxKind::OuterAssignExpr);
                Ok(AstNode::OuterAssignment {
                    name,
                    op: assign_op,
                    value: Box::new(AstNode::PipeInput),
                    span,
                    name_span: var_span,
                })
            }
            AstNode::Index { object, index, .. } => {
                self.advance();
                let span = self.cst_close_with_span(pending, SyntaxKind::IndexAssignExpr);
                Ok(AstNode::IndexAssign {
                    object,
                    index,
                    op: assign_op,
                    value: Box::new(AstNode::PipeInput),
                    span,
                })
            }
            AstNode::MutatingIndex { object, index, .. } => {
                self.advance();
                let span = self.cst_close_with_span(pending, SyntaxKind::MutatingIndexAssignExpr);
                Ok(AstNode::MutatingIndexAssign {
                    object,
                    index,
                    value: Box::new(AstNode::PipeInput),
                    span,
                })
            }
            AstNode::Postfix {
                object,
                items,
                explicit_call: false,
                ..
            } => {
                self.advance();
                let span = self.cst_close_with_span(pending, SyntaxKind::IndexAssignExpr);
                let index = if items.len() == 1 {
                    Box::new(items.into_iter().next().expect("len == 1"))
                } else {
                    Box::new(AstNode::List(items, None))
                };
                Ok(AstNode::IndexAssign {
                    object,
                    index,
                    op: assign_op,
                    value: Box::new(AstNode::PipeInput),
                    span,
                })
            }
            _ => Ok(expr),
        }
    }

    fn parse_pipe_rhs_expr(&mut self) -> WqResult<AstNode> {
        if let Some(token) = self.current_token().cloned()
            && token.token_type == TokenType::AtDebug
            && self.token_after_current_ends_optional_probe_operand()
        {
            self.advance();
            return Ok(AstNode::Debug {
                expr: Box::new(AstNode::PipeInput),
                span: Some((token.byte_start, token.byte_end)),
            });
        }
        self.parse_pipe_rhs_checkpoint_assignment()
    }

    // list/dict ===============================================================

    fn parse_paren_list(&mut self, lparen_start: usize) -> WqResult<AstNode> {
        self.bracket_depth += 1;
        let result = (|| {
            let mut elements = Vec::new();
            loop {
                self.eat_trivia(true, true);
                if self.is_token(&TokenType::RightParen) {
                    self.advance();
                    break;
                }
                if self.is_token(&TokenType::Eof) {
                    return Err(self.eof_error_here("unexpected end of input in list"));
                }
                let expr = self.parse_expression()?;
                elements.push(expr);
                self.eat_trivia(false, true);
                if self.is_token(&TokenType::RightParen) {
                    self.advance();
                    break;
                }
                self.require_control_separator("list")?;
                self.eat_trivia(true, true);
                if self.is_token(&TokenType::RightParen) {
                    self.advance();
                    break;
                }
                if self.is_token(&TokenType::Eof) {
                    return Err(self.eof_error_here("unexpected end of input in list"));
                }
            }
            Ok(if elements.len() == 1 {
                let node = elements.remove(0);
                // The group spans from `(` to `)`. The last consumed token
                // is the closing paren, so its byte_end is the rparen end.
                let span = Some((lparen_start, self.last_consumed_byte_end()));
                AstNode::Group {
                    expr: Box::new(node),
                    span,
                }
            } else {
                let span = Some((lparen_start, self.last_consumed_byte_end()));
                AstNode::List(elements, span)
            })
        })();
        self.bracket_depth -= 1;
        result
    }

    fn parse_paren_dict(&mut self, lparen_start: usize) -> WqResult<AstNode> {
        self.bracket_depth += 1;
        let result = {
            let mut pairs = Vec::new();
            let res: WqResult<()> = loop {
                self.eat_trivia(true, true);
                if self.is_token(&TokenType::RightParen) {
                    self.advance();
                    break Ok(());
                }
                if self.is_token(&TokenType::Eof) {
                    break Err(self.eof_error_here("unexpected end of input in dict"));
                }
                // Each pair gets its own DictPair wrap so the formatter can
                // re-flow them independently of their siblings.
                let cp_pair = self.cst_checkpoint();
                let key_tok = match self.current_token() {
                    Some(t) => t,
                    None => break Err(self.eof_error_here("unexpected end of input in dict")),
                };
                let key = match &key_tok.token_type {
                    TokenType::Tag(s) => {
                        let s = s.clone();
                        self.advance();
                        s
                    }
                    TokenType::Eof => {
                        break Err(self.eof_error_here("unexpected end of input in dict"));
                    }
                    _ => break Err(self.syntax_err(key_tok, "expected symbol key in dict")),
                };
                if let Err(e) = self.consume(TokenType::Colon) {
                    break Err(e);
                }
                let value = match self.parse_expression() {
                    Ok(v) => v,
                    Err(e) => break Err(e),
                };
                pairs.push((key, value));
                self.cst_start_node_at(cp_pair, SyntaxKind::DictPair);
                self.cst_finish_node();
                self.eat_trivia(false, true);
                if self.is_token(&TokenType::RightParen) {
                    self.advance();
                    break Ok(());
                }
                if self.is_token(&TokenType::Eof) {
                    break Err(self.eof_error_here("unexpected end of input in dict"));
                }
                if let Err(e) = self.require_control_separator("dict") {
                    break Err(e);
                }
                self.eat_trivia(true, true);
                if self.is_token(&TokenType::RightParen) {
                    self.advance();
                    break Ok(());
                }
            };
            res.map(|()| {
                let span = Some((lparen_start, self.last_consumed_byte_end()));
                AstNode::Dict(pairs, span)
            })
        };
        self.bracket_depth -= 1;
        result
    }

    fn parse_primary(&mut self) -> WqResult<AstNode> {
        // Single CST checkpoint at the syntactic start of the primary. After
        // the inner dispatch returns, we wrap the consumed tokens in the
        // appropriate kind based on the AstNode variant. This keeps every
        // primary-level branch (literal, identifier, paren list, function,
        // control form, etc.) uniformly tagged in the green tree without
        // threading checkpoints through every callee.
        let cp = self.cst_checkpoint();
        let result = self.parse_primary_inner()?;
        if let Some(kind) = Self::primary_syntax_kind(&result) {
            self.cst_start_node_at(cp, kind);
            self.cst_finish_node();
        }
        Ok(result)
    }

    /// Map an [`AstNode`] returned from [`Self::parse_primary_inner`] onto the
    /// CST kind that should wrap the corresponding source bytes.
    ///
    /// `None` means "do not wrap": either the variant carries no source bytes
    /// (e.g. [`AstNode::PipeInput`], a parser-internal placeholder) or it
    /// represents a higher-level construction that the corresponding parse
    /// function already wrapped (e.g. an `Assignment` returned from below
    /// only when the assignment infrastructure spliced its way in).
    fn primary_syntax_kind(node: &AstNode) -> Option<SyntaxKind> {
        Some(match node {
            AstNode::Literal(..) => SyntaxKind::LiteralExpr,
            AstNode::Variable(..) => SyntaxKind::VarExpr,
            AstNode::OuterVariable(..) => SyntaxKind::OuterVarExpr,
            AstNode::Function { .. } => SyntaxKind::FunctionExpr,
            AstNode::Conditional { .. } => SyntaxKind::CondExpr,
            AstNode::ConditionalDot { .. } => SyntaxKind::CondDotExpr,
            AstNode::ConditionalChain { .. } => SyntaxKind::CondChainExpr,
            AstNode::WLoop { .. } => SyntaxKind::WLoopExpr,
            AstNode::NLoop { .. } => SyntaxKind::NLoopExpr,
            AstNode::BlockExpr(..) => SyntaxKind::BlockExpr,
            AstNode::List(..) => SyntaxKind::ListExpr,
            AstNode::Dict(..) => SyntaxKind::DictExpr,
            AstNode::Group { .. } => SyntaxKind::ParenExpr,
            AstNode::FString { .. } => SyntaxKind::FStringExpr,
            AstNode::Return(..) => SyntaxKind::ReturnExpr,
            AstNode::Break(_) => SyntaxKind::BreakExpr,
            AstNode::Continue(_) => SyntaxKind::ContinueExpr,
            AstNode::Assert { .. } => SyntaxKind::AssertExpr,
            AstNode::Debug { .. } => SyntaxKind::DebugExpr,
            AstNode::Pause { .. } => SyntaxKind::PauseExpr,
            AstNode::Try(..) => SyntaxKind::TryExpr,

            AstNode::Ellipsis(_) => SyntaxKind::EllipsisExpr,
            AstNode::Error(..) => SyntaxKind::ErrorNode,
            // The following variants never come out of `parse_primary_inner`
            // -- they are produced by higher-precedence parse layers that
            // wrap themselves. If one of them shows up here it means a parse
            // function was refactored to bypass the precedence ladder; the
            // green tree stays correct (just unwrapped at this level), and
            // the reachable sites of those variants will continue to wrap
            // themselves.
            AstNode::PipeInput
            | AstNode::Postfix { .. }
            | AstNode::CallName { .. }
            | AstNode::CallAnonymous { .. }
            | AstNode::Index { .. }
            | AstNode::MutatingIndex { .. }
            | AstNode::IndexAssign { .. }
            | AstNode::MutatingIndexAssign { .. }
            | AstNode::Assignment { .. }
            | AstNode::OuterAssignment { .. }
            | AstNode::UnpackAssignment { .. }
            | AstNode::BinaryOp { .. }
            | AstNode::UnaryOp { .. }
            | AstNode::ComparisonChain { .. }
            | AstNode::Range { .. }
            | AstNode::Pipe { .. }
            | AstNode::PipeTap { .. }
            | AstNode::NamedArg { .. }
            | AstNode::Cat(..)
            | AstNode::Block(..) => return None,
        })
    }

    fn parse_primary_inner(&mut self) -> WqResult<AstNode> {
        if let Some(token) = self.current_token() {
            let token = token.clone();
            if let TokenType::FormatString(parts, _, _) = &token.token_type {
                let parts = parts.clone();
                let span = (token.byte_start, token.byte_end);
                self.advance();
                return self.build_fstring_ast_from_parts(parts, Some(span));
            }

            match &token.token_type {
                TokenType::Integer(n) => {
                    let v = *n;
                    self.advance();
                    Ok(AstNode::Literal(
                        Value::Int(v),
                        Some((token.byte_start, token.byte_end)),
                    ))
                }
                TokenType::BigInteger(n) => {
                    let v = n.clone();
                    self.advance();
                    Ok(AstNode::Literal(
                        Value::BigInt(Arc::new(v)),
                        Some((token.byte_start, token.byte_end)),
                    ))
                }
                TokenType::Float(f) => {
                    let v = *f;
                    self.advance();
                    Ok(AstNode::Literal(
                        Value::float(v),
                        Some((token.byte_start, token.byte_end)),
                    ))
                }
                TokenType::Imaginary(im) => {
                    let v = *im;
                    self.advance();
                    Ok(AstNode::Literal(
                        Value::from_complex64(num_complex::Complex64::new(0.0, v)),
                        Some((token.byte_start, token.byte_end)),
                    ))
                }
                TokenType::Character(c) => {
                    let v = *c;
                    self.advance();
                    Ok(AstNode::Literal(
                        Value::Char(v),
                        Some((token.byte_start, token.byte_end)),
                    ))
                }
                TokenType::String(s) => {
                    let v = s.clone();
                    self.advance();
                    Ok(AstNode::Literal(
                        v.into_wq_value(),
                        Some((token.byte_start, token.byte_end)),
                    ))
                }
                TokenType::Tag(s) => {
                    let v = s.clone();
                    self.advance();
                    Ok(AstNode::Literal(
                        Value::Tag(v.into()),
                        Some((token.byte_start, token.byte_end)),
                    ))
                }
                TokenType::True => {
                    self.advance();
                    Ok(AstNode::Literal(
                        Value::Bool(true),
                        Some((token.byte_start, token.byte_end)),
                    ))
                }
                TokenType::False => {
                    self.advance();
                    Ok(AstNode::Literal(
                        Value::Bool(false),
                        Some((token.byte_start, token.byte_end)),
                    ))
                }
                TokenType::Inf => {
                    self.advance();
                    Ok(AstNode::Literal(
                        Value::float(f64::INFINITY),
                        Some((token.byte_start, token.byte_end)),
                    ))
                }
                TokenType::Dollar => self.parse_conditional(),
                TokenType::DollarDot => self.parse_conditional_dot(),
                TokenType::DollarDollar => self.parse_conditional_chain(),
                TokenType::LeftBracket => {
                    let header_start_byte = token.byte_start;
                    let header_start_idx = self.current;
                    self.advance();
                    self.parse_block_expr(header_start_byte, header_start_idx)
                }

                TokenType::AtBreak => {
                    self.advance();
                    Ok(AstNode::Break(Some((token.byte_start, token.byte_end))))
                }
                TokenType::AtContinue => {
                    self.advance();
                    Ok(AstNode::Continue(Some((token.byte_start, token.byte_end))))
                }
                TokenType::AtReturn => {
                    let start = token.byte_start;
                    self.advance();
                    if let Some(tok) = self.current_token() {
                        if matches!(
                            tok.token_type,
                            TokenType::Semicolon
                                | TokenType::RightBracket
                                | TokenType::RightParen
                                | TokenType::RightBrace
                                | TokenType::Newline
                                | TokenType::Eof
                        ) {
                            Ok(AstNode::Return(None, Some((start, token.byte_end))))
                        } else {
                            let expr = self.parse_expression()?;
                            Ok(AstNode::Return(
                                Some(Box::new(expr)),
                                Some((start, self.last_consumed_byte_end())),
                            ))
                        }
                    } else {
                        Ok(AstNode::Return(None, Some((start, token.byte_end))))
                    }
                }

                TokenType::AtAssert => {
                    let start = token.byte_start;
                    self.advance();
                    let expr = self.parse_expression()?;
                    Ok(AstNode::Assert {
                        expr: Box::new(expr),
                        span: Some((start, self.last_consumed_byte_end())),
                    })
                }
                TokenType::AtDebug => {
                    let start = token.byte_start;
                    self.advance();
                    let expr = self.parse_unary()?;
                    Ok(AstNode::Debug {
                        expr: Box::new(expr),
                        span: Some((start, self.last_consumed_byte_end())),
                    })
                }
                TokenType::AtPause => {
                    let start = token.byte_start;
                    self.advance();
                    let expr = if self.current_token_ends_optional_probe_operand() {
                        None
                    } else {
                        Some(Box::new(self.parse_unary()?))
                    };
                    let end = if expr.is_some() {
                        self.last_consumed_byte_end()
                    } else {
                        token.byte_end
                    };
                    Ok(AstNode::Pause {
                        expr,
                        span: Some((start, end)),
                    })
                }
                TokenType::AtTry => {
                    let start = token.byte_start;
                    self.advance();
                    let e = self.parse_expression()?;
                    Ok(AstNode::Try(
                        Box::new(e),
                        Some((start, self.last_consumed_byte_end())),
                    ))
                }

                TokenType::Identifier(name) => {
                    let val = name.clone();
                    let span = (token.byte_start, token.byte_end);
                    let postfix_cp = self.cst_checkpoint();
                    // Capture the byte position of the identifier itself so
                    // the W/N/B/S branches below have a byte-based span
                    // start without reaching back into `self.tokens` by
                    // index after the sigils have been consumed.
                    let header_start_byte = span.0;
                    let header_start_idx = self.current;
                    self.advance();

                    // Allow comments between W/N and '['; newline not allowed
                    while matches!(
                        self.current_token().map(|t| &t.token_type),
                        Some(TokenType::Comment(_))
                    ) {
                        self.advance();
                    }
                    if let Some(Token {
                        token_type: TokenType::LeftBracket,
                        ..
                    }) = self.current_token()
                    {
                        if val == "W" {
                            self.advance();
                            return self.parse_w_loop(header_start_byte, header_start_idx);
                        } else if val == "N" {
                            self.advance();
                            return self.parse_n_loop(header_start_byte, header_start_idx);
                        } else if val == "B" {
                            self.advance();
                            return self.parse_block_expr(header_start_byte, header_start_idx);
                        } else if val == "A" {
                            return self.parse_lazy_bool_form(
                                "A",
                                BinaryOperator::BoolAnd,
                                header_start_byte,
                                postfix_cp,
                            );
                        } else if val == "O" {
                            return self.parse_lazy_bool_form(
                                "O",
                                BinaryOperator::BoolOr,
                                header_start_byte,
                                postfix_cp,
                            );
                        }
                    }
                    // Allow comments between S and '('; newline not allowed
                    while matches!(
                        self.current_token().map(|t| &t.token_type),
                        Some(TokenType::Comment(_))
                    ) {
                        self.advance();
                    }

                    Ok(AstNode::Variable(val, Some(span)))
                }
                TokenType::Apostrophe => {
                    let start = token.clone();
                    let start_byte = start.byte_start;
                    self.advance();
                    if matches!(
                        self.current_token().map(|t| &t.token_type),
                        Some(TokenType::LeftBrace)
                    ) {
                        return self.parse_function(true, start_byte);
                    }
                    let (name, end_byte) = match self.current_token().map(|t| (&t.token_type, t)) {
                        Some((TokenType::Identifier(name), t)) => {
                            let name = name.clone();
                            let end_byte = t.byte_end;
                            self.advance();
                            (name, end_byte)
                        }
                        Some((_, bad)) => {
                            return Err(
                                self.syntax_err(bad, "expected identifier after apostrophe")
                            );
                        }
                        None => {
                            return Err(
                                self.eof_error_here("unexpected end of input after apostrophe")
                            );
                        }
                    };
                    Ok(AstNode::OuterVariable(name, Some((start_byte, end_byte))))
                }
                TokenType::AtSymbolic => self.parse_symbolic_quote(),
                TokenType::LeftBrace => self.parse_function(false, token.byte_start),
                TokenType::LeftParen => {
                    let lparen_start = token.byte_start;
                    self.advance(); // '('
                    self.eat_trivia(true, true);
                    // Special empty-dict syntax: (` [trivia])
                    if matches!(
                        self.current_token().map(|t| &t.token_type),
                        Some(TokenType::Backtick)
                    ) {
                        self.advance(); // consume backtick
                        // allow optional trivia before ')'
                        self.eat_trivia(true, true);
                        if self.is_token(&TokenType::RightParen) {
                            self.advance();
                            let span = Some((lparen_start, self.last_consumed_byte_end()));
                            return Ok(AstNode::Dict(Vec::new(), span));
                        } else if let Some(t) = self.current_token().cloned() {
                            return Err(self.syntax_err(&t, "expected closing ')' for empty dict"));
                        } else {
                            return Err(self.eof_error_here("unexpected end of input"));
                        }
                    }
                    if self.is_token(&TokenType::RightParen) {
                        self.advance();
                        let span = Some((lparen_start, self.last_consumed_byte_end()));
                        return Ok(AstNode::List(Vec::new(), span));
                    }
                    // Decide dict vs list: Symbol ':' lookahead (no deep skipping)
                    let mut is_dict = false;
                    if let Some(Token {
                        token_type: TokenType::Tag(_),
                        ..
                    }) = self.current_token()
                        && let Some(next) = self.peek_token()
                        && next.token_type == TokenType::Colon
                    {
                        is_dict = true;
                    }
                    if is_dict {
                        self.parse_paren_dict(lparen_start)
                    } else {
                        self.parse_paren_list(lparen_start)
                    }
                }
                TokenType::Ellipsis => {
                    self.advance();
                    Ok(AstNode::Ellipsis(Some((token.byte_start, token.byte_end))))
                }
                TokenType::Plus => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("+".into(), Some(span)))
                }
                TokenType::Minus => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("-".into(), Some(span)))
                }
                TokenType::Multiply => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("*".into(), Some(span)))
                }
                TokenType::Divide => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("/".into(), Some(span)))
                }
                TokenType::DivideDot => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("/.".into(), Some(span)))
                }
                TokenType::PowerDot => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("^.".into(), Some(span)))
                }
                TokenType::Modulo => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("%".into(), Some(span)))
                }

                TokenType::Power => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("^".into(), Some(span)))
                }
                TokenType::Matmul => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("**".into(), Some(span)))
                }
                TokenType::Equal => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("=".into(), Some(span)))
                }
                TokenType::EqualDot => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("=.".into(), Some(span)))
                }
                TokenType::NotEqual => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("~".into(), Some(span)))
                }
                TokenType::NotEqualDot => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("~.".into(), Some(span)))
                }
                TokenType::LessThan => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("<".into(), Some(span)))
                }
                TokenType::LessThanOrEqual => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("<=".into(), Some(span)))
                }
                TokenType::GreaterThan => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable(">".into(), Some(span)))
                }
                TokenType::GreaterThanOrEqual => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable(">=".into(), Some(span)))
                }
                TokenType::Sharp => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("#".into(), Some(span)))
                }
                TokenType::Comma => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable(",".into(), Some(span)))
                }
                TokenType::Shl => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("<<".into(), Some(span)))
                }
                TokenType::Shr => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable(">>".into(), Some(span)))
                }
                TokenType::FloorDiv => {
                    let span = (token.byte_start, token.byte_end);
                    self.advance();
                    Ok(AstNode::Variable("/%".into(), Some(span)))
                }
                TokenType::Eof => Err(self.eof_error_here("unexpected end of input")),
                _ => {
                    Err(self
                        .syntax_err(&token, format!("unexpected token: {:?}", token.token_type)))
                }
            }
        } else {
            Err(self.eof_error_here("unexpected end of input"))
        }
    }

    /// Recursively add `offset` to every span in an AST so that sub-expressions
    /// parsed from a snippet (e.g. inside an f-string `{…}`) map back to the
    /// correct positions in the original source.
    pub(crate) fn offset_spans(node: &mut AstNode, offset: usize) {
        #![deny(clippy::wildcard_enum_match_arm)]
        match node {
            AstNode::Error(err, span) => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                if let Some(span) = &mut err.span {
                    span.0 += offset;
                    span.1 += offset;
                }
            }
            AstNode::Literal(_, span)
            | AstNode::Variable(_, span)
            | AstNode::OuterVariable(_, span) => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
            }
            AstNode::PipeInput => {}
            AstNode::Ellipsis(span) | AstNode::Break(span) | AstNode::Continue(span) => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
            }
            AstNode::NamedArg { value, span, .. } => {
                Self::offset_spans(value, offset);
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
            }
            AstNode::Assignment {
                value,
                span,
                name_span,
                ..
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                if let Some(name_span) = name_span {
                    name_span.0 += offset;
                    name_span.1 += offset;
                }
                Self::offset_spans(value, offset);
            }
            AstNode::OuterAssignment {
                value,
                span,
                name_span,
                ..
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                if let Some(name_span) = name_span {
                    name_span.0 += offset;
                    name_span.1 += offset;
                }
                Self::offset_spans(value, offset);
            }
            AstNode::UnpackAssignment { lhs, rhs, span, .. } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                for n in lhs {
                    Self::offset_spans(n, offset);
                }
                Self::offset_spans(rhs, offset);
            }
            AstNode::Postfix {
                object,
                items,
                span,
                ..
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(object, offset);
                for item in items {
                    Self::offset_spans(item, offset);
                }
            }
            AstNode::Pipe {
                input,
                effect,
                span,
                ..
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(input, offset);
                Self::offset_spans(effect, offset);
            }
            AstNode::PipeTap {
                input,
                effect,
                span,
                ..
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(input, offset);
                Self::offset_spans(effect, offset);
            }
            AstNode::CallName {
                args,
                span,
                name_span,
                ..
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                if let Some(name_span) = name_span {
                    name_span.0 += offset;
                    name_span.1 += offset;
                }
                for arg in args {
                    Self::offset_spans(arg, offset);
                }
            }
            AstNode::CallAnonymous {
                object, args, span, ..
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(object, offset);
                for arg in args {
                    Self::offset_spans(arg, offset);
                }
            }
            AstNode::IndexAssign {
                object,
                index,
                value,
                span,
                ..
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(object, offset);
                Self::offset_spans(index, offset);
                Self::offset_spans(value, offset);
            }
            AstNode::Assert { expr, span, .. } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(expr, offset);
            }
            AstNode::Debug { expr, span, .. } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(expr, offset);
            }
            AstNode::Pause { expr, span } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                if let Some(expr) = expr {
                    Self::offset_spans(expr, offset);
                }
            }
            AstNode::FString { parts, span, .. } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                for part in parts {
                    if let crate::astnode::FStringPart::Expr {
                        expr, spec_exprs, ..
                    } = part
                    {
                        Self::offset_spans(expr, offset);
                        for se in spec_exprs {
                            Self::offset_spans(se, offset);
                        }
                    }
                }
            }

            AstNode::BinaryOp {
                left, right, span, ..
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(left, offset);
                Self::offset_spans(right, offset);
            }
            AstNode::UnaryOp { operand, span, .. } => {
                if let Some(s) = span {
                    s.0 += offset;
                    s.1 += offset;
                }
                Self::offset_spans(operand, offset);
            }
            AstNode::Group { expr, span } => {
                if let Some(s) = span {
                    s.0 += offset;
                    s.1 += offset;
                }
                Self::offset_spans(expr, offset);
            }
            AstNode::ComparisonChain { first, rest, span } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(first, offset);
                for (_, n) in rest {
                    Self::offset_spans(n, offset);
                }
            }
            AstNode::Range {
                start,
                end,
                step,
                span,
                ..
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(start, offset);
                Self::offset_spans(end, offset);
                if let Some(s) = step {
                    Self::offset_spans(s, offset);
                }
            }
            AstNode::Conditional {
                condition,
                true_branch,
                false_branch,
                span,
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(condition, offset);
                Self::offset_spans(true_branch, offset);
                if let Some(fb) = false_branch {
                    Self::offset_spans(fb, offset);
                }
            }
            AstNode::ConditionalDot {
                condition,
                true_branch,
                span,
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(condition, offset);
                Self::offset_spans(true_branch, offset);
            }
            AstNode::ConditionalChain {
                pairs,
                default_branch,
                span,
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                for (cond, branch) in pairs {
                    Self::offset_spans(cond, offset);
                    Self::offset_spans(branch, offset);
                }
                Self::offset_spans(default_branch, offset);
            }
            AstNode::WLoop {
                condition,
                body,
                span,
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(condition, offset);
                Self::offset_spans(body, offset);
            }
            AstNode::NLoop { count, body, span } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(count, offset);
                Self::offset_spans(body, offset);
            }
            AstNode::Return(expr, span) => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                if let Some(expr) = expr {
                    Self::offset_spans(expr, offset);
                }
            }
            AstNode::Try(expr, span) => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(expr, offset);
            }
            AstNode::Cat(items, span)
            | AstNode::List(items, span)
            | AstNode::Block(items, span) => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                for item in items {
                    Self::offset_spans(item, offset);
                }
            }
            AstNode::BlockExpr(items, span) => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                for item in items {
                    Self::offset_spans(item, offset);
                }
            }
            AstNode::Dict(pairs, span) => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                for (_, v) in pairs {
                    Self::offset_spans(v, offset);
                }
            }

            AstNode::Index {
                object,
                index,
                span,
                ..
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(object, offset);
                Self::offset_spans(index, offset);
            }
            AstNode::MutatingIndex {
                object,
                index,
                span,
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(object, offset);
                Self::offset_spans(index, offset);
            }
            AstNode::MutatingIndexAssign {
                object,
                index,
                value,
                span,
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                Self::offset_spans(object, offset);
                Self::offset_spans(index, offset);
                Self::offset_spans(value, offset);
            }
            AstNode::Function {
                params, body, span, ..
            } => {
                if let Some(span) = span {
                    span.0 += offset;
                    span.1 += offset;
                }
                if let Some(ps) = params {
                    for p in ps.iter_mut() {
                        match p {
                            Parameter::Pos { span, .. } | Parameter::Named { span, .. } => {
                                if let Some((s, e)) = span {
                                    *s += offset;
                                    *e += offset;
                                }
                            }
                        }
                    }
                }
                Self::offset_spans(body, offset);
            }
        }
    }

    /// Split a format-string brace contents like `value!{width}` or `num!>10`
    /// into `(expression, optional_format_spec)`.
    ///
    /// `!` is treated as a separator when it is at brace-depth 0.
    /// wq does not use `!` as an operator (`~=` is not-equal), so there is no
    /// ambiguity to resolve.
    pub(crate) fn split_expr_and_format_spec(inner: &str) -> (&str, Option<&str>) {
        let mut depth = 0i32;
        let mut in_str = false;
        let mut prev_escape = false;

        for (i, c) in inner.char_indices() {
            if in_str {
                if c == '\\' && !prev_escape {
                    prev_escape = true;
                } else if c == '"' && !prev_escape {
                    in_str = false;
                    prev_escape = false;
                } else {
                    prev_escape = false;
                }
                continue;
            }

            match c {
                '"' => in_str = true,
                '{' => depth += 1,
                '}' => depth -= 1,
                '!' if depth == 0 => {
                    let expr = &inner[..i];
                    let spec = &inner[i + c.len_utf8()..].trim_start();
                    if expr.trim().is_empty() {
                        return (inner, None);
                    }
                    return (expr, if spec.is_empty() { None } else { Some(spec) });
                }
                _ => {}
            }
        }

        (inner, None)
    }

    pub(crate) fn matching_fstring_brace(source: &str, open: usize) -> Option<usize> {
        if !source.is_char_boundary(open) || !source[open..].starts_with('{') {
            return None;
        }

        let mut depth = 1usize;
        let mut i = open + '{'.len_utf8();
        let mut in_string = false;
        let mut escaped = false;

        while i < source.len() {
            let c = source[i..].chars().next().expect("i is inside source");
            if in_string {
                if c == '\\' && !escaped {
                    escaped = true;
                } else if c == '"' && !escaped {
                    in_string = false;
                    escaped = false;
                } else {
                    escaped = false;
                }
                i += c.len_utf8();
                continue;
            }

            match c {
                '"' => in_string = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
            i += c.len_utf8();
        }
        None
    }

    /// Scan a format-spec string for `{expr}` dynamic width/precision
    /// placeholders. Each one is parsed into an AstNode and replaced with `{}`
    /// in the returned encoded spec.
    fn encode_format_spec(
        &self,
        spec_str: &str,
        base_offset: usize,
    ) -> WqResult<(String, Vec<AstNode>)> {
        let mut result = String::new();
        let mut exprs = Vec::new();
        let mut i = 0;

        while i < spec_str.len() {
            let c = spec_str[i..].chars().next().unwrap();
            if c == '{' {
                let Some(j) = Self::matching_fstring_brace(spec_str, i) else {
                    return Err(
                        WqError::new(WqErrorType::Syntax).msg("unmatched '{' in format spec")
                    );
                };

                let inner_start = i + c.len_utf8();
                let inner = &spec_str[inner_start..j];
                let mut lex = Lexer::new(inner);
                let tokens = lex.tokenize()?;
                let mut p2 = Parser::new(tokens, inner.to_string());
                let mut node = p2.parse()?;
                Self::offset_spans(&mut node, base_offset + inner_start);
                exprs.push(node);

                result.push_str("{}");
                i = j + '}'.len_utf8();
            } else {
                result.push(c);
                i += c.len_utf8();
            }
        }

        Ok((result, exprs))
    }

    fn build_fstring_ast_from_parts(
        &self,
        parts: Vec<crate::token::FmtPart>,
        span: AstSpan,
    ) -> WqResult<AstNode> {
        use crate::escape::unescape_string_inner;
        use crate::token::FmtPart;

        let mut fstring_parts: Vec<crate::astnode::FStringPart> = Vec::new();

        for part in parts {
            match part {
                FmtPart::Text { content, .. } => {
                    let text = match unescape_string_inner(&content) {
                        Ok(s) => s,
                        Err(_) => content,
                    };
                    fstring_parts.push(FStringPart::Text(text));
                }
                FmtPart::Expr { source, start, .. } => {
                    let inner = &source[1..source.len().saturating_sub(1)];
                    let (expr_str, spec_opt) = Self::split_expr_and_format_spec(inner);

                    let mut lex = Lexer::new(expr_str);
                    let tokens = lex.tokenize()?;
                    let mut p2 = Parser::new(tokens, expr_str.to_string());
                    let mut node = p2.parse()?;
                    let offset = start + 1;
                    Self::offset_spans(&mut node, offset);

                    let (spec, encoded_spec, spec_exprs) = if let Some(spec_str) = spec_opt {
                        let spec_offset = start + 1 + expr_str.len() + 1;
                        let (enc, mut spec_exprs) =
                            self.encode_format_spec(spec_str, spec_offset)?;
                        for spec_expr in &mut spec_exprs {
                            Self::offset_spans(spec_expr, spec_offset);
                        }
                        (Some(spec_str.to_string()), Some(enc), spec_exprs)
                    } else {
                        (None, None, Vec::new())
                    };

                    fstring_parts.push(FStringPart::Expr {
                        expr: node,
                        spec,
                        encoded_spec,
                        spec_exprs,
                    });
                }
            }
        }

        Ok(AstNode::FString {
            parts: fstring_parts,
            span,
        })
    }

    fn parse_symbolic_quote(&mut self) -> WqResult<AstNode> {
        let start = self
            .current_token()
            .cloned()
            .ok_or_else(|| self.eof_error_here("unexpected end of input after @s"))?;
        self.advance();
        self.eat_trivia(true, true);
        let expr = self.parse_comma()?;
        Ok(AstNode::Literal(
            self.quote_symbolic_value(expr, &start)?,
            None,
        ))
    }

    fn quote_symbolic_value(&self, node: AstNode, start: &Token) -> WqResult<Value> {
        use AstNode::*;

        let source_span = |span: Option<(usize, usize)>| {
            if let Some((s, e)) = span {
                if let Some(gs) = &self.global_source {
                    (
                        gs.clone(),
                        Some((s + self.base_offset, e + self.base_offset)),
                    )
                } else {
                    (self.source.clone(), Some((s, e)))
                }
            } else {
                let s = start.byte_start;
                let e = start.byte_end;
                if let Some(gs) = &self.global_source {
                    (
                        gs.clone(),
                        Some((s + self.base_offset, e + self.base_offset)),
                    )
                } else {
                    (self.source.clone(), Some((s, e)))
                }
            }
        };
        let mk_err = |span: Option<(usize, usize)>, msg: String| {
            let (text, abs_span) = source_span(span);
            let path = self.source_path.as_deref().unwrap_or("?");
            WqError::new(WqErrorType::Syntax)
                .src("parser")
                .msg(msg)
                .span(abs_span)
                .source_ctx(text, path)
        };
        let with_parser_ctx = |span: Option<(usize, usize)>, mut err: WqError| {
            let (text, abs_span) = source_span(span);
            let path = self.source_path.as_deref().unwrap_or("?").to_string();
            err.err_type = WqErrorType::Syntax;
            err.src = Some("parser".to_string());
            err.span = abs_span;
            err.source_ctx = Some(Box::new(crate::wqerror::SourceCtx { text, path }));
            err
        };

        let node_span = node.span();
        let quote_call = |name: &str, args: Vec<AstNode>, span: Option<(usize, usize)>| {
            let mut positional = Vec::new();
            let mut named = Vec::new();
            for arg in args {
                match arg {
                    NamedArg { name, value, .. } => {
                        named.push((name, self.quote_symbolic_value(*value, start)?));
                    }
                    arg => positional.push(self.quote_symbolic_value(arg, start)?),
                }
            }
            cas_symbolic_call_expr(name, &positional, &named)
                .map_err(|err| with_parser_ctx(span, err))
        };

        match node {
            Literal(Value::Float(f), _) if f.is_infinite() && f.is_sign_positive() => {
                Ok(Value::from_cas_const(CasConst::Infinity))
            }
            Literal(Value::Float(f), _) if f.is_infinite() && f.is_sign_negative() => {
                Ok(Value::from_cas_const(CasConst::NegInfinity))
            }
            Literal(value, _) => Ok(value),
            Variable(name, _) => {
                if let Some(konst) = CasConst::from_name(&name) {
                    Ok(Value::from_cas_const(konst))
                } else {
                    Ok(Value::from_cas_var(name))
                }
            }
            UnaryOp {
                operator: UnaryOperator::Negate,
                operand,
                ..
            } => cas_unary_expr(
                CasOp::Subtract,
                &self.quote_symbolic_value(*operand, start)?,
            ),
            UnaryOp {
                operator: UnaryOperator::Count,
                ..
            } => Err(mk_err(
                node_span,
                "@s: count operator '#' is not supported in symbolic expressions".to_string(),
            )),
            BinaryOp {
                left,
                operator: BinaryOperator::Equal,
                right,
                ..
            } => Ok(Value::from_cas_eq(
                self.quote_symbolic_value(*left, start)?,
                self.quote_symbolic_value(*right, start)?,
            )),
            BinaryOp {
                operator:
                    BinaryOperator::EqualDot
                    | BinaryOperator::NotEqual
                    | BinaryOperator::NotEqualDot
                    | BinaryOperator::Lt
                    | BinaryOperator::Lte
                    | BinaryOperator::Gt
                    | BinaryOperator::Gte,
                ..
            } => Err(mk_err(
                node_span,
                "@s: comparison operators are not supported in symbolic expressions".to_string(),
            )),
            BinaryOp {
                left,
                operator,
                right,
                ..
            } => {
                let lhs = self.quote_symbolic_value(*left, start)?;
                let rhs = self.quote_symbolic_value(*right, start)?;
                let op = match operator {
                    BinaryOperator::Add => CasOp::Add,
                    BinaryOperator::Subtract => CasOp::Subtract,
                    BinaryOperator::Multiply => CasOp::Multiply,
                    BinaryOperator::Power => CasOp::Power,
                    BinaryOperator::Divide => CasOp::Divide,
                    _ => {
                        return Err(mk_err(
                            node_span,
                            "@s: this binary operator is not supported in symbolic expressions"
                                .to_string(),
                        ));
                    }
                };
                cas_binary_expr(op, &lhs, &rhs)
            }
            Group { expr, .. } => self.quote_symbolic_value(*expr, start),
            ComparisonChain { .. } => Err(mk_err(
                node_span,
                "@s: comparison chains are not supported in symbolic expressions".to_string(),
            )),
            CallName { name, args, .. } => quote_call(&name, args, node_span),
            Postfix { object, items, .. } => {
                let object_span = object.span();
                match *object {
                    Variable(name, _) => quote_call(&name, items, object_span),
                    Literal(value, ..)
                        if matches!(value, Value::Int(_) | Value::BigInt(_) | Value::Float(_)) =>
                    {
                        let args = items
                            .into_iter()
                            .map(|arg| self.quote_symbolic_value(arg, start))
                            .collect::<WqResult<Vec<_>>>()?;
                        args.into_iter().try_fold(value, |acc, arg| {
                            cas_binary_expr(CasOp::Multiply, &acc, &arg)
                        })
                    }
                    _ => Err(mk_err(
                        object_span,
                        "@s: dynamic call targets are not supported in symbolic expressions"
                            .to_string(),
                    )),
                }
            }
            List(items, _) => Ok(Value::List(Arc::new(
                items
                    .into_iter()
                    .map(|item| self.quote_symbolic_value(item, start))
                    .collect::<WqResult<Vec<_>>>()?,
            ))),
            Dict(pairs, _) => Ok(Value::Dict(Arc::new(
                pairs
                    .into_iter()
                    .map(|(key, value)| Ok((key.into(), self.quote_symbolic_value(value, start)?)))
                    .collect::<WqResult<_>>()?,
            ))),
            _ => Err(mk_err(
                node_span,
                "@s: this expression is not supported in symbolic expressions".to_string(),
            )),
        }
    }

    // fn ====================================================================================

    fn parse_function(&mut self, ref_capture: bool, start_byte: usize) -> WqResult<AstNode> {
        self.advance(); // '{'
        let mut params = None;
        // Optional parameter list: {[a;b]}
        if let Some(tok) = self.current_token()
            && tok.token_type == TokenType::LeftBracket
        {
            // Wrap the `[ a ; b ]` portion in a ParamList subtree so the
            // formatter can re-flow parameters independently of the body.
            let cp_params = self.cst_checkpoint();
            self.advance(); // '['
            let mut names = Vec::new();
            let mut seen_param_names: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            loop {
                // allow trivia inside params
                self.eat_trivia(true, true);
                match self.current_token().map(|t| (&t.token_type, t)) {
                    Some((TokenType::Identifier(name), tok)) => {
                        if self.builtins.has_function(name) {
                            return Err(self.syntax_err(
                                tok,
                                format!(
                                    "cannot use '{name}' as a parameter because a builtin with the same name exists"
                                ),
                            ));
                        }
                        if !seen_param_names.insert(name.clone()) {
                            return Err(
                                self.syntax_err(tok, format!("duplicate parameter name '{name}'"))
                            );
                        }
                        // Capture the borrow-only data before taking the
                        // mutable checkpoint borrow.
                        let name_owned = name.clone();
                        let span = (tok.byte_start, tok.byte_end);
                        // Per-parameter wrap so `Param` is locatable in the
                        // CST. Single token, but the kind tag matters for
                        // hover/definition lookups.
                        let cp_p = self.cst_checkpoint();
                        names.push(Parameter::Pos {
                            name: name_owned,
                            span: Some(span),
                        });
                        self.advance();
                        self.cst_start_node_at(cp_p, SyntaxKind::Param);
                        self.cst_finish_node();
                    }
                    Some((TokenType::Tag(tag_name), tok)) => {
                        let name_owned = tag_name.clone();
                        if !seen_param_names.insert(name_owned.clone()) {
                            return Err(self.syntax_err(
                                tok,
                                format!("duplicate parameter name '{}'", name_owned),
                            ));
                        }
                        let span = (tok.byte_start, tok.byte_end);
                        let cp_p = self.cst_checkpoint();
                        self.advance(); // consume tag
                        // Optional default: `name:expr
                        let default = if self.is_token(&TokenType::Colon) {
                            self.advance(); // consume ':'
                            Some(Box::new(self.parse_pipe()?))
                        } else {
                            None
                        };
                        names.push(Parameter::Named {
                            name: name_owned,
                            span: Some(span),
                            default,
                        });
                        self.cst_start_node_at(cp_p, SyntaxKind::Param);
                        self.cst_finish_node();
                    }
                    Some((TokenType::Semicolon, _)) => {
                        self.advance();
                    }
                    Some((TokenType::RightBracket, _)) => {
                        self.advance();
                        break;
                    }
                    Some((TokenType::Eof, _)) => {
                        return Err(
                            self.eof_error_here("unexpected end of input in parameter list")
                        );
                    }
                    Some((_, bad)) => {
                        return Err(self.syntax_err(bad, "expected identifier, ';' or ']'"));
                    }
                    None => {
                        return Err(
                            self.eof_error_here("unexpected end of input in parameter list")
                        );
                    }
                }
            }
            self.cst_start_node_at(cp_params, SyntaxKind::ParamList);
            self.cst_finish_node();
            params = Some(names);
        }
        // Reserve a slot in fn_spans so this function appears before its nested
        // children.
        let slot = self.fn_spans.len();
        self.fn_spans.push(Vec::new());
        // Start collecting nested statement spans while parsing this function.
        self.fn_span_stack.push(Vec::new());
        let (body, _) = self.parse_block_with_spans()?;
        self.consume(TokenType::RightBrace)?;
        let span = Some((start_byte, self.last_consumed_byte_end()));
        let mut spans = self.fn_span_stack.pop().unwrap_or_default();
        Self::normalize_stmt_spans(&mut spans);
        self.fn_spans[slot] = spans;
        Ok(AstNode::Function {
            params,
            ref_capture,
            body: Box::new(body),
            span,
        })
    }

    // control forms
    // ====================================================================================

    fn parse_branch_sequence(&mut self, ends: &[TokenType]) -> WqResult<AstNode> {
        let mut stmts = Vec::new();
        let is_end =
            |tt: &TokenType, x: &TokenType| std::mem::discriminant(tt) == std::mem::discriminant(x);
        loop {
            // Allow comments before items, and only treat newlines as trivia when they are
            // not a caller-specified branch boundary.
            loop {
                match self.current_token().map(|t| &t.token_type) {
                    Some(TokenType::Comment(_)) => {
                        self.advance();
                    }
                    Some(TokenType::Newline)
                        if !ends.iter().any(|e| is_end(&TokenType::Newline, e)) =>
                    {
                        self.advance();
                    }
                    _ => break,
                }
            }
            // stop if at an end token (do not consume); if Eof token, propagate EOF
            match self.current_token() {
                Some(tok) => {
                    if matches!(tok.token_type, TokenType::Eof) {
                        return Err(self.eof_error_here("unexpected end of input in branch"));
                    }
                    if ends.iter().any(|e| is_end(&tok.token_type, e)) {
                        break;
                    }
                }
                None => return Err(self.eof_error_here("unexpected end of input in branch")),
            }
            // Record span for this branch statement relative to the current position
            let start_idx = self.current;
            if let Some(reused) = self.try_reuse_sequence_item() {
                self.merge_reused_stmt_spans(reused.stmt_spans);
                self.merge_reused_fn_spans(reused.fn_spans);
                stmts.push(reused.ast);
            } else {
                let expr = self.parse_expression()?;
                let end_idx = self.current.saturating_sub(1);
                self.record_stmt_span_idx(start_idx, end_idx);
                stmts.push(expr);
            }
            // Between statements:
            // - skip comments (never a separator)
            // - newline separates
            // - semicolon separates only if it's not an end token
            loop {
                while matches!(
                    self.current_token().map(|t| &t.token_type),
                    Some(TokenType::Comment(_))
                ) {
                    self.advance();
                }
                match self.current_token().map(|t| &t.token_type) {
                    Some(TokenType::Newline) => {
                        if ends.iter().any(|e| is_end(&TokenType::Newline, e)) {
                            // boundary for caller; don't consume
                            break;
                        } else {
                            self.advance();
                            continue;
                        }
                    }
                    Some(TokenType::Semicolon) => {
                        if ends.iter().any(|e| is_end(&TokenType::Semicolon, e)) {
                            // boundary for caller; don't consume
                            break;
                        } else {
                            self.advance();
                            continue;
                        }
                    }
                    _ => break,
                }
            }
        }
        Ok(if stmts.len() == 1 {
            stmts.remove(0)
        } else {
            let span = Self::span_for_items(&stmts);
            AstNode::Block(stmts, span)
        })
    }

    fn parse_conditional(&mut self) -> WqResult<AstNode> {
        // The `$` sigil is still the current token. Capture its byte start
        // before any advance() so the span derivation is byte-based all the
        // way through.
        let header_start_byte = self.current_byte_start();
        let header_start_idx = self.current;
        self.advance(); // '$'
        self.consume(TokenType::LeftBracket)?;
        self.bracket_depth += 1;
        let result = (|| {
            self.eat_trivia(true, true);
            let condition = self.parse_expression()?;
            if self.is_token(&TokenType::RightBracket) {
                // Single-branch $[cond] -- same as $.[cond]
                self.consume(TokenType::RightBracket)?;
                let header_end_byte = self.last_consumed_byte_end();
                self.record_stmt_span_idx(header_start_idx, self.current.saturating_sub(1));
                return Ok(AstNode::Conditional {
                    condition: Box::new(condition),
                    true_branch: Box::new(AstNode::Block(Vec::new(), None)),
                    false_branch: None,
                    span: Some((header_start_byte, header_end_byte)),
                });
            }
            self.require_control_separator("$[condition;true;false]")?;
            let true_branch = self.parse_branch_sequence(&[
                TokenType::Semicolon,
                TokenType::Newline,
                TokenType::RightBracket,
            ])?;
            if self.is_token(&TokenType::RightBracket) {
                // Single-branch $[cond;true] -- same as $.[cond;true]
                self.consume(TokenType::RightBracket)?;
                let header_end_byte = self.last_consumed_byte_end();
                self.record_stmt_span_idx(header_start_idx, self.current.saturating_sub(1));
                return Ok(AstNode::Conditional {
                    condition: Box::new(condition),
                    true_branch: Box::new(true_branch),
                    false_branch: None,
                    span: Some((header_start_byte, header_end_byte)),
                });
            }
            self.require_control_separator("$[condition;true;false]")?;
            let false_branch = self.parse_branch_sequence(&[TokenType::RightBracket])?;
            self.consume(TokenType::RightBracket)?;
            let header_end_byte = self.last_consumed_byte_end();
            self.record_stmt_span_idx(header_start_idx, self.current.saturating_sub(1));
            Ok(AstNode::Conditional {
                condition: Box::new(condition),
                true_branch: Box::new(true_branch),
                false_branch: Some(Box::new(false_branch)),
                span: Some((header_start_byte, header_end_byte)),
            })
        })();
        self.bracket_depth -= 1;
        result
    }

    fn parse_conditional_dot(&mut self) -> WqResult<AstNode> {
        let header_start_byte = self.current_byte_start();
        let header_start_idx = self.current;
        self.advance(); // '$.'
        self.consume(TokenType::LeftBracket)?;
        self.bracket_depth += 1;
        let result = (|| {
            self.eat_trivia(true, true);
            let condition = self.parse_expression()?;
            if self.is_token(&TokenType::RightBracket) {
                self.consume(TokenType::RightBracket)?;
                let header_end_byte = self.last_consumed_byte_end();
                self.record_stmt_span_idx(header_start_idx, self.current.saturating_sub(1));
                return Ok(AstNode::ConditionalDot {
                    condition: Box::new(condition),
                    true_branch: Box::new(AstNode::Block(Vec::new(), None)),
                    span: Some((header_start_byte, header_end_byte)),
                });
            }
            self.require_control_separator("$.[condition;true]")?;
            let true_branch = self.parse_branch_sequence(&[TokenType::RightBracket])?;
            self.consume(TokenType::RightBracket)?;
            let header_end_byte = self.last_consumed_byte_end();
            self.record_stmt_span_idx(header_start_idx, self.current.saturating_sub(1));
            Ok(AstNode::ConditionalDot {
                condition: Box::new(condition),
                true_branch: Box::new(true_branch),
                span: Some((header_start_byte, header_end_byte)),
            })
        })();
        self.bracket_depth -= 1;
        result
    }

    fn parse_conditional_chain(&mut self) -> WqResult<AstNode> {
        let header_start_byte = self.current_byte_start();
        let header_start_idx = self.current;
        self.advance(); // '$$'
        self.consume(TokenType::LeftBracket)?;
        let mut items: Vec<AstNode> = Vec::new();
        loop {
            if items.is_empty() {
                self.eat_trivia(true, true);
            } else {
                self.eat_trivia(false, true);
            }
            if self.is_token(&TokenType::RightBracket) {
                break;
            }
            if self.is_token(&TokenType::Eof) {
                return Err(self.eof_error_here("unexpected end of input in $$[...]"));
            }
            let item = if self.is_token(&TokenType::Semicolon) || self.is_token(&TokenType::Newline)
            {
                AstNode::Block(Vec::new(), None)
            } else {
                self.parse_expression()?
            };
            items.push(item);
            self.eat_trivia(false, true);
            if self.is_token(&TokenType::RightBracket) {
                break;
            }
            self.require_control_separator("$$[condition;true;...;default]")?;
        }
        if items.is_empty() {
            return Err(self.syntax_err(
                self.current_token().expect("loop saw a non-Eof token"),
                "'$$': expected condition",
            ));
        }
        if items.len() == 1 {
            return Err(self.syntax_err(
                self.current_token().expect("loop saw a non-Eof token"),
                "'$$': expected condition/branch pairs",
            ));
        }
        self.consume(TokenType::RightBracket)?;
        if items.len().is_multiple_of(2) {
            items.push(AstNode::Block(Vec::new(), None));
        }
        let default_branch = Box::new(items.pop().expect("items non-empty"));
        let mut pairs = Vec::new();
        let mut iter = items.into_iter();
        while let Some(cond) = iter.next() {
            let true_branch = iter.next().ok_or_else(|| {
                self.syntax_err(
                    self.current_token().expect("inside $$[...]"),
                    "'$$': expected condition/branch pairs",
                )
            })?;
            pairs.push((cond, true_branch));
        }
        let header_end_byte = self.last_consumed_byte_end();
        self.record_stmt_span_idx(header_start_idx, self.current.saturating_sub(1));
        Ok(AstNode::ConditionalChain {
            pairs,
            default_branch,
            span: Some((header_start_byte, header_end_byte)),
        })
    }

    /// `header_start_byte` is the byte position of the opening `[` or
    /// legacy leading `B`. The caller in `parse_primary_inner` consumed the
    /// opening bracket already.
    fn parse_block_expr(
        &mut self,
        header_start_byte: usize,
        header_start_idx: usize,
    ) -> WqResult<AstNode> {
        self.bracket_depth += 1;
        let result = (|| {
            let body = self.parse_branch_sequence(&[TokenType::RightBracket])?;
            self.consume(TokenType::RightBracket)?;
            let header_end_byte = self.last_consumed_byte_end();
            self.record_stmt_span_idx(header_start_idx, self.current.saturating_sub(1));
            let stmts = match body {
                AstNode::Block(stmts, _) => stmts,
                other => vec![other],
            };
            Ok(AstNode::BlockExpr(
                stmts,
                Some((header_start_byte, header_end_byte)),
            ))
        })();
        self.bracket_depth -= 1;
        result
    }

    /// Like [`Self::parse_block_expr`] but for `W[...]`.
    fn parse_w_loop(
        &mut self,
        header_start_byte: usize,
        header_start_idx: usize,
    ) -> WqResult<AstNode> {
        self.bracket_depth += 1;
        let result = (|| {
            self.eat_trivia(true, true);
            let spans_len_before = self
                .fn_span_stack
                .last()
                .map(|v| v.len())
                .unwrap_or(self.stmt_spans.len());
            let condition = self.parse_expression()?;
            if let Some(last) = self.fn_span_stack.last_mut() {
                last.truncate(spans_len_before);
            } else {
                self.stmt_spans.truncate(spans_len_before);
            }
            if self.is_token(&TokenType::RightBracket) {
                self.consume(TokenType::RightBracket)?;
                let header_end_byte = self.last_consumed_byte_end();
                self.record_stmt_span_idx(header_start_idx, self.current.saturating_sub(1));
                return Ok(AstNode::WLoop {
                    condition: Box::new(condition),
                    body: Box::new(AstNode::Block(Vec::new(), None)),
                    span: Some((header_start_byte, header_end_byte)),
                });
            }
            self.require_control_separator("W[condition;body]")?;
            let body = self.parse_branch_sequence(&[TokenType::RightBracket])?;
            self.consume(TokenType::RightBracket)?;
            let header_end_byte = self.last_consumed_byte_end();
            self.record_stmt_span_idx(header_start_idx, self.current.saturating_sub(1));
            Ok(AstNode::WLoop {
                condition: Box::new(condition),
                body: Box::new(body),
                span: Some((header_start_byte, header_end_byte)),
            })
        })();
        self.bracket_depth -= 1;
        result
    }

    /// Like [`Self::parse_block_expr`] but for `N[...]`.
    fn parse_n_loop(
        &mut self,
        header_start_byte: usize,
        header_start_idx: usize,
    ) -> WqResult<AstNode> {
        self.bracket_depth += 1;
        let result = (|| {
            self.eat_trivia(true, true);
            let spans_len_before = self
                .fn_span_stack
                .last()
                .map(|v| v.len())
                .unwrap_or(self.stmt_spans.len());
            let count_expr = self.parse_expression()?;
            if let Some(last) = self.fn_span_stack.last_mut() {
                last.truncate(spans_len_before);
            } else {
                self.stmt_spans.truncate(spans_len_before);
            }
            if self.is_token(&TokenType::RightBracket) {
                self.consume(TokenType::RightBracket)?;
                let header_end_byte = self.last_consumed_byte_end();
                self.record_stmt_span_idx(header_start_idx, self.current.saturating_sub(1));
                return Ok(AstNode::NLoop {
                    count: Box::new(count_expr),
                    body: Box::new(AstNode::Block(Vec::new(), None)),
                    span: Some((header_start_byte, header_end_byte)),
                });
            }
            self.require_control_separator("N[count;body]")?;
            let body = self.parse_branch_sequence(&[TokenType::RightBracket])?;
            self.consume(TokenType::RightBracket)?;
            let header_end_byte = self.last_consumed_byte_end();
            self.record_stmt_span_idx(header_start_idx, self.current.saturating_sub(1));
            Ok(AstNode::NLoop {
                count: Box::new(count_expr),
                body: Box::new(body),
                span: Some((header_start_byte, header_end_byte)),
            })
        })();
        self.bracket_depth -= 1;
        result
    }
}

#[cfg(test)]
mod fstring_span_tests {
    use super::*;

    #[test]
    fn fstring_expr_span_is_offset_to_original_source() {
        let src = "echo@f\"123{x}\"";
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().unwrap();
        let mut p = Parser::new(tokens, src.to_string());
        let ast = p.parse().unwrap();

        // ast should be a Postfix call: echo(@f"123{x}")
        // We need to dig into the FString to find the Variable "x"
        let AstNode::Postfix { object, items, .. } = ast else {
            panic!("expected Postfix, got {ast:?}");
        };
        assert!(matches!(object.as_ref(), AstNode::Variable(name, _) if name == "echo"));
        assert_eq!(items.len(), 1);
        let AstNode::FString { parts, .. } = &items[0] else {
            panic!("expected FString, got {:?}", items[0]);
        };
        assert_eq!(parts.len(), 2);
        let expr = match &parts[1] {
            crate::astnode::FStringPart::Expr { expr, .. } => expr,
            other => panic!("expected Expr part, got {other:?}"),
        };
        let AstNode::Variable(name, span) = expr else {
            panic!("expected Variable, got {:?}", expr);
        };
        assert_eq!(name, "x");
        // "x" is at byte 11 in "echo@f\"123{x}\""
        assert_eq!(
            *span,
            Some((11, 12)),
            "span should point to 'x' in original source"
        );
    }

    #[test]
    fn fstring_format_spec_produces_encoded_template() {
        let src = "@f\"{value!>10}\"";
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().unwrap();
        let mut p = Parser::new(tokens, src.to_string());
        let ast = p.parse().unwrap();

        let AstNode::FString { parts, .. } = ast else {
            panic!("expected FString, got {ast:?}");
        };
        assert_eq!(parts.len(), 1);
        let (expr, spec, encoded_spec) = match &parts[0] {
            crate::astnode::FStringPart::Expr {
                expr,
                spec,
                encoded_spec,
                ..
            } => (expr, spec, encoded_spec),
            other => panic!("expected Expr part, got {other:?}"),
        };
        let AstNode::Variable(name, _) = expr else {
            panic!("expected Variable, got {:?}", expr);
        };
        assert_eq!(name, "value");
        assert_eq!(
            spec.as_deref(),
            Some(">10"),
            "raw format spec should be preserved"
        );
        assert_eq!(
            encoded_spec.as_deref(),
            Some(">10"),
            "encoded format spec should match raw spec when no dynamic placeholders"
        );
    }

    #[test]
    fn fstring_dynamic_width_puts_spec_expr_before_value() {
        let src = "@f\"{value!{width}}\"";
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().unwrap();
        let mut p = Parser::new(tokens, src.to_string());
        let ast = p.parse().unwrap();

        let AstNode::FString { parts, .. } = ast else {
            panic!("expected FString, got {ast:?}");
        };
        assert_eq!(parts.len(), 1);
        let (expr, spec, encoded_spec, spec_exprs) = match &parts[0] {
            crate::astnode::FStringPart::Expr {
                expr,
                spec,
                encoded_spec,
                spec_exprs,
                ..
            } => (expr, spec, encoded_spec, spec_exprs),
            other => panic!("expected Expr part, got {other:?}"),
        };
        let AstNode::Variable(v_name, _) = expr else {
            panic!("expected Variable for value, got {:?}", expr);
        };
        assert_eq!(v_name, "value");
        assert_eq!(
            spec.as_deref(),
            Some("{width}"),
            "raw format spec should be preserved"
        );
        assert_eq!(
            encoded_spec.as_deref(),
            Some("{}"),
            "encoded format spec should collapse dynamic placeholders"
        );
        assert_eq!(spec_exprs.len(), 1);
        let AstNode::Variable(w_name, _) = &spec_exprs[0] else {
            panic!("expected Variable for width, got {:?}", spec_exprs[0]);
        };
        assert_eq!(w_name, "width");
    }

    #[test]
    fn fstring_dynamic_spec_expr_allows_quoted_brace() {
        let src = r##"@f"{value!>{"}"}}""##;
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().unwrap();
        let mut p = Parser::new(tokens, src.to_string());
        let ast = p.parse().unwrap();

        let AstNode::FString { parts, .. } = ast else {
            panic!("expected FString, got {ast:?}");
        };
        assert_eq!(parts.len(), 1);
        let (spec, encoded_spec, spec_exprs) = match &parts[0] {
            crate::astnode::FStringPart::Expr {
                spec,
                encoded_spec,
                spec_exprs,
                ..
            } => (spec, encoded_spec, spec_exprs),
            other => panic!("expected Expr part, got {other:?}"),
        };

        assert_eq!(spec.as_deref(), Some(r#">{"}"}"#));
        assert_eq!(encoded_spec.as_deref(), Some(">{}"));
        assert_eq!(spec_exprs.len(), 1);
        assert!(matches!(spec_exprs[0], AstNode::Literal(..)));
    }

    #[test]
    fn fstring_format_spec_unmatched_dynamic_brace_still_errors() {
        let parser = Parser::new(Vec::new(), String::new());
        let err = parser
            .encode_format_spec(">{x", 0)
            .expect_err("unmatched dynamic spec brace should fail");

        assert!(
            err.to_string().contains("unmatched '{' in format spec"),
            "unexpected error: {err}"
        );
    }
}

#[cfg(test)]
mod sync_tests {
    use super::*;

    fn parse_input(input: &str) -> WqResult<AstNode> {
        let tokens = Lexer::new(input).tokenize().unwrap();
        let mut parser =
            Parser::new_with_builtins(tokens, input.to_string(), crate::builtins::Builtins::new());
        parser.parse()
    }

    #[test]
    fn sync_skips_broken_stmt_and_parses_rest() {
        let ast = parse_input("a:1\nx +\nb:2").unwrap();
        let stmts = match ast {
            AstNode::Block(stmts, _) => stmts,
            other => vec![other],
        };
        assert_eq!(stmts.len(), 3);
        assert!(matches!(&stmts[0], AstNode::Assignment { name, .. } if name == "a"));
        assert!(matches!(&stmts[1], AstNode::Error(..)));
        assert!(matches!(&stmts[2], AstNode::Assignment { name, .. } if name == "b"));
    }

    #[test]
    fn sync_does_not_consume_semicolon_inside_bracket() {
        let ast = parse_input("N[3; x + ; y]\na:1").unwrap();
        let stmts = match ast {
            AstNode::Block(stmts, _) => stmts,
            other => vec![other],
        };
        // Should get: Error(N[...), Variable(y), Error(]), Assignment(a)
        // or possibly other arrangements, but a:1 must survive
        let has_a = stmts
            .iter()
            .any(|s| matches!(s, AstNode::Assignment { name, .. } if name == "a"));
        assert!(has_a, "a:1 should be parsed after broken N[...]");
    }

    #[test]
    fn sync_consumes_right_bracket_to_avoid_spin() {
        let ast = parse_input("N[3; x +]\na:1").unwrap();
        let stmts = match ast {
            AstNode::Block(stmts, _) => stmts,
            other => vec![other],
        };
        let has_a = stmts
            .iter()
            .any(|s| matches!(s, AstNode::Assignment { name, .. } if name == "a"));
        assert!(has_a, "a:1 should be parsed after broken N[...]");
    }

    #[test]
    fn sync_inside_block() {
        let ast = parse_input("{a:1\nx +\nb:2}").unwrap();
        let stmts = match ast {
            AstNode::Block(stmts, _) => stmts,
            other => vec![other],
        };
        // outer block has one stmt: the function literal
        assert_eq!(stmts.len(), 1);
        let fn_body = match &stmts[0] {
            AstNode::Function { body, .. } => match body.as_ref() {
                AstNode::Block(b, _) => b.clone(),
                other => vec![other.clone()],
            },
            _ => panic!("expected Function, got {:?}", stmts[0]),
        };
        assert_eq!(fn_body.len(), 3);
        assert!(matches!(&fn_body[0], AstNode::Assignment { name, .. } if name == "a"));
        assert!(matches!(&fn_body[1], AstNode::Error(..)));
        assert!(matches!(&fn_body[2], AstNode::Assignment { name, .. } if name == "b"));
    }

    #[test]
    fn sync_in_paren_list() {
        let ast = parse_input("(a; b + ; c)\nd:1").unwrap();
        let stmts = match ast {
            AstNode::Block(stmts, _) => stmts,
            other => vec![other],
        };
        let has_d = stmts
            .iter()
            .any(|s| matches!(s, AstNode::Assignment { name, .. } if name == "d"));
        assert!(has_d, "d:1 should be parsed after broken (a; b + ; c)");
    }

    #[test]
    fn eof_is_not_recovered_at_top_level() {
        let tokens = Lexer::new("t:{k:3").tokenize().unwrap();
        let mut parser = Parser::new_with_builtins(
            tokens,
            "t:{k:3".to_string(),
            crate::builtins::Builtins::new(),
        );
        let result = parser.parse();
        assert!(
            result.is_ok(),
            "incomplete block should return Ok, with eof_error set"
        );
        let err = parser.eof_error().expect("eof_error should be set");
        assert_eq!(err.err_type, WqErrorType::Eof);
    }

    #[test]
    fn nested_bracket_sync() {
        let ast = parse_input("N[3; W[x > ; y]]\na:1").unwrap();
        let stmts = match ast {
            AstNode::Block(stmts, _) => stmts,
            other => vec![other],
        };
        let has_a = stmts
            .iter()
            .any(|s| matches!(s, AstNode::Assignment { name, .. } if name == "a"));
        assert!(has_a, "a:1 should be parsed after broken nested brackets");
    }
}

#[cfg(test)]
mod symbolic_quote_tests {
    use super::*;
    fn parse_input(input: &str) -> WqResult<AstNode> {
        let tokens = Lexer::new(input).tokenize().unwrap();
        let mut parser =
            Parser::new_with_builtins(tokens, input.to_string(), crate::builtins::Builtins::new());
        parser.parse()
    }

    #[test]
    fn symbolic_quote_accepts_unknown_application() {
        let ast = parse_input("@s f[x]").expect("unknown CAS application should parse");
        let AstNode::Literal(value, _) = ast else {
            panic!("expected CAS literal, got {ast:?}");
        };
        assert!(value.is_cas_expr());
        let (head, args) = value
            .cas_apply_parts()
            .expect("expected symbolic application");
        assert_eq!(head.as_str(), "f");
        assert_eq!(args, [Value::from_cas_var("x")]);
        assert_eq!(value.to_string(), "f[x]");
    }

    #[test]
    fn symbolic_quote_accepts_unknown_application_with_builtin_args() {
        let ast = parse_input("@s f[x+1;sin[y]]").expect("unknown CAS application should parse");
        let AstNode::Literal(value, _) = ast else {
            panic!("expected CAS literal, got {ast:?}");
        };
        let (head, args) = value
            .cas_apply_parts()
            .expect("expected symbolic application");
        assert_eq!(head.as_str(), "f");
        assert_eq!(args.len(), 2);
        assert_eq!(value.to_string(), "f[x + 1;sin[y]]");
    }

    #[test]
    fn symbolic_quote_accepts_known_call() {
        let ast = parse_input("@s sin[x]").expect("known CAS call should parse");
        let AstNode::Literal(value, _) = ast else {
            panic!("expected CAS literal, got {ast:?}");
        };
        assert!(value.is_cas_expr());
        assert!(value.cas_function_parts().is_some());
        assert!(value.cas_apply_parts().is_none());
        assert_eq!(value.to_string(), "sin[x]");
    }

    #[test]
    fn symbolic_quote_limit_accepts_named_direction() {
        let ast =
            parse_input("@s limit[1/x;0;`d:@s+]").expect("named limit direction should parse");
        let AstNode::Literal(value, _) = ast else {
            panic!("expected CAS literal, got {ast:?}");
        };
        let (_expr, var, point, direction) = value
            .cas_limit_parts()
            .expect("expected symbolic limit node");
        assert_eq!(var, &Value::from_cas_var("x"));
        assert_eq!(point, &Value::Int(0));
        assert_eq!(direction, Some(crate::cas::limit::LimitDirection::Right));
        assert_eq!(value.to_string(), "limit[x^-1;x;0;`d:+]");
    }

    #[test]
    fn symbolic_quote_limit_rejects_unknown_named_arg() {
        let ast =
            parse_input("@s limit[1/x;0;`dir:@s+]").expect("parser should recover with error node");
        let AstNode::Error(err, _) = ast else {
            panic!("expected parser error node, got {ast:?}");
        };
        assert!(
            err.msg
                .as_deref()
                .is_some_and(|msg| msg.contains("unknown named argument 'dir'")),
            "unexpected error: {err:?}",
        );
    }

    #[test]
    fn symbolic_quote_preserves_named_args_in_unknown_application() {
        let ast = parse_input("@s f[x;`a:y]").expect("named symbolic arg should parse");
        let AstNode::Literal(value, _) = ast else {
            panic!("expected CAS literal, got {ast:?}");
        };
        let (head, args) = value
            .cas_apply_parts()
            .expect("expected symbolic application");
        assert_eq!(head.as_str(), "f");
        assert_eq!(args.len(), 2);
        let (name, named_value) = args[1]
            .cas_named_arg_parts()
            .expect("expected named symbolic argument");
        assert_eq!(name.as_str(), "a");
        assert_eq!(named_value, &Value::from_cas_var("y"));
        assert_eq!(value.to_string(), "f[x;`a:y]");
    }

    #[test]
    fn symbolic_quote_limit_rejects_positional_direction() {
        let ast =
            parse_input("@s limit[1/x;x;0;@s+]").expect("parser should recover with error node");
        let AstNode::Error(err, _) = ast else {
            panic!("expected parser error node, got {ast:?}");
        };
        assert!(
            err.msg
                .as_deref()
                .is_some_and(|msg| msg
                    .contains("limit expects expr;point or expr followed by var;point pairs")),
            "unexpected error: {err:?}",
        );
    }

    #[test]
    fn symbolic_quote_does_not_swallow_statement_newline() {
        let ast = parse_input("expr:@s x^2+2*x+1\nexpr|echo").expect("script should parse");
        let AstNode::Block(stmts, _) = ast else {
            panic!("expected block, got {ast:?}");
        };

        assert_eq!(stmts.len(), 2);
        assert!(matches!(&stmts[0], AstNode::Assignment { name, .. } if name == "expr"));
        assert!(matches!(&stmts[1], AstNode::Pipe { .. }));
    }

    #[test]
    fn symbolic_quote_does_not_swallow_pipe() {
        let ast = parse_input("@s x^2|diff").expect("script should parse");
        let AstNode::Pipe { input, effect, .. } = ast else {
            panic!("expected pipe, got {ast:?}");
        };
        let AstNode::Literal(value, _) = input.as_ref() else {
            panic!("expected symbolic input, got {input:?}");
        };
        assert_eq!(value.to_string(), "x^2");
        assert!(matches!(effect.as_ref(), AstNode::Variable(name, _) if name == "diff"));
    }
}

#[cfg(test)]
mod err_span_tests {
    use super::*;

    fn parse_input(input: &str) -> WqResult<AstNode> {
        let tokens = Lexer::new(input).tokenize().unwrap();
        let mut parser =
            Parser::new_with_builtins(tokens, input.to_string(), crate::builtins::Builtins::new());
        parser.parse()
    }

    #[test]
    fn eof_error_has_span_at_eof_location() {
        let tokens = Lexer::new("echo [1").tokenize().unwrap();
        let mut parser = Parser::new_with_builtins(
            tokens,
            "echo [1".to_string(),
            crate::builtins::Builtins::new(),
        );
        let result = parser.parse();
        assert!(result.is_ok());
        let err = parser.eof_error().expect("expected eof error");
        assert_eq!(err.err_type, WqErrorType::Eof);
        // Eof should point at the position *after* the '1', not the start
        assert_eq!(
            err.span,
            Some((7, 7)),
            "eof span should be at byte 7 (after '1')"
        );
        assert!(err.msg.as_ref().unwrap().contains("bracket"));
    }

    #[test]
    fn syntax_error_span_is_exact_token() {
        let ast = parse_input("1+").unwrap();
        let stmts = match ast {
            AstNode::Block(s, _) => s,
            other => vec![other],
        };
        assert_eq!(stmts.len(), 1);
        let AstNode::Error(err, span) = &stmts[0] else {
            panic!("expected Error node");
        };
        assert_eq!(
            *span,
            Some((1, 2)),
            "Plus span should be exactly [1,2), got {:?}",
            span
        );
        assert_eq!(err.span, Some((1, 2)));
    }

    #[test]
    fn error_span_does_not_bleed_across_expression() {
        let ast = parse_input("echo [1+").unwrap();
        let stmts = match ast {
            AstNode::Block(s, _) => s,
            other => vec![other],
        };
        assert_eq!(stmts.len(), 1);
        let AstNode::Error(err, span) = &stmts[0] else {
            panic!("expected Error node");
        };
        // The '+' is at byte 7..8; span should NOT cover the whole "echo [1+"
        assert_eq!(
            *span,
            Some((7, 8)),
            "span should be exactly the '+' token, got {:?}",
            span
        );
        assert_eq!(err.span, Some((7, 8)));
    }

    #[test]
    fn multi_line_error_span_is_exact() {
        let ast = parse_input("{ a }\n1+").unwrap();
        let stmts = match ast {
            AstNode::Block(s, _) => s,
            other => vec![other],
        };
        assert_eq!(stmts.len(), 2);
        let AstNode::Error(err, span) = &stmts[1] else {
            panic!("expected second stmt to be Error");
        };
        // '+' starts at byte 7 after "{ a }\n1".
        assert_eq!(
            *span,
            Some((7, 8)),
            "span should be exactly the '+' on line 2, got {:?}",
            span
        );
        assert_eq!(err.span, Some((7, 8)));
    }

    // #[test]
    // fn syntax_err_width_is_single_char_for_bracket() {
    //     let input = "[ b ]";
    //     let tokens = Lexer::new(input).tokenize().unwrap();
    //     let parser = Parser::new_with_builtins(
    //         tokens.clone(),
    //         input.to_string(),
    //         crate::builtins::Builtins::new(),
    //     );
    //     let tok = &tokens[0]; // LeftBracket
    //     let err = parser.syntax_err(tok, "test");
    //     let display = err.to_string();
    //     // The underline should have exactly one tilde for a single-char
    // token     let underline_line = display.lines().find(|l|
    // l.contains('~')).unwrap();     let tildes: Vec<_> =
    // underline_line.matches('~').collect();     assert_eq!(
    //         tildes.len(),
    //         1,
    //         "pointer should be exactly one tilde for a single-char token,
    // display was: {display}"     );
    // }
}

#[cfg(test)]
mod coloncolon_conditional_tests {
    use super::*;

    fn parse_input(input: &str) -> WqResult<AstNode> {
        let tokens = Lexer::new(input).tokenize().unwrap();
        let mut parser =
            Parser::new_with_builtins(tokens, input.to_string(), crate::builtins::Builtins::new());
        parser.parse()
    }

    fn assert_conditional(ast: &AstNode) -> (&AstNode, &AstNode, Option<&AstNode>) {
        match ast {
            AstNode::Conditional {
                condition,
                true_branch,
                false_branch,
                ..
            } => (condition, true_branch, false_branch.as_deref()),
            other => panic!("expected Conditional, got {other:?}"),
        }
    }

    // --- $[ ... ] ---

    #[test]
    fn dollar_cond_basic() {
        let ast = parse_input("$[1;2;3]").unwrap();
        let (cond, true_br, false_br) = assert_conditional(&ast);
        assert!(matches!(cond, AstNode::Literal(Value::Int(1), _)));
        assert!(matches!(true_br, AstNode::Literal(Value::Int(2), _)));
        assert!(matches!(false_br, Some(AstNode::Literal(Value::Int(3), _))));
    }

    #[test]
    fn dollar_cond_multi_stmt_false_branch() {
        let ast = parse_input("$[1;2;3;4]").unwrap();
        let (cond, true_br, false_br) = assert_conditional(&ast);
        assert!(matches!(cond, AstNode::Literal(Value::Int(1), _)));
        assert!(matches!(true_br, AstNode::Literal(Value::Int(2), _)));
        let AstNode::Block(stmts, _) = false_br.unwrap() else {
            panic!("expected Block for false branch, got {false_br:?}");
        };
        assert_eq!(stmts.len(), 2);
        assert!(matches!(&stmts[0], AstNode::Literal(Value::Int(3), _)));
        assert!(matches!(&stmts[1], AstNode::Literal(Value::Int(4), _)));
    }

    #[test]
    fn dollar_cond_single_branch() {
        let ast = parse_input("$[1;2]").unwrap();
        let (cond, true_br, false_br) = assert_conditional(&ast);
        assert!(matches!(cond, AstNode::Literal(Value::Int(1), _)));
        assert!(matches!(true_br, AstNode::Literal(Value::Int(2), _)));
        assert_eq!(false_br, None);
    }

    #[test]
    fn dollar_cond_empty_true_branch() {
        let ast = parse_input("$[1;;3]").unwrap();
        let (cond, true_br, false_br) = assert_conditional(&ast);
        assert!(matches!(cond, AstNode::Literal(Value::Int(1), _)));
        assert_eq!(true_br, &AstNode::Block(Vec::new(), None));
        assert!(matches!(false_br, Some(AstNode::Literal(Value::Int(3), _))));
    }

    #[test]
    fn dollar_cond_empty_false_branch() {
        let ast = parse_input("$[1;2;]").unwrap();
        let (cond, true_br, false_br) = assert_conditional(&ast);
        assert!(matches!(cond, AstNode::Literal(Value::Int(1), _)));
        assert!(matches!(true_br, AstNode::Literal(Value::Int(2), _)));
        assert_eq!(false_br, Some(&AstNode::Block(Vec::new(), None)));
    }

    // --- $$[ ... ] ---

    fn assert_conditional_chain(ast: &AstNode) -> (&[(AstNode, AstNode)], &AstNode) {
        match ast {
            AstNode::ConditionalChain {
                pairs,
                default_branch,
                ..
            } => (pairs.as_slice(), default_branch.as_ref()),
            other => panic!("expected ConditionalChain, got {other:?}"),
        }
    }

    #[test]
    fn dollar_dollar_cond_basic() {
        let ast = parse_input("$$[0>1;1;0<1;2;3]").unwrap();
        let (pairs, default_branch) = assert_conditional_chain(&ast);
        assert_eq!(pairs.len(), 2);
        assert!(matches!(&pairs[0].0, AstNode::BinaryOp { .. }));
        assert!(matches!(&pairs[0].1, AstNode::Literal(Value::Int(1), _)));
        assert!(matches!(&pairs[1].0, AstNode::BinaryOp { .. }));
        assert!(matches!(&pairs[1].1, AstNode::Literal(Value::Int(2), _)));
        assert!(matches!(default_branch, AstNode::Literal(Value::Int(3), _)));
    }

    #[test]
    fn dollar_dollar_cond_empty_branch() {
        let ast = parse_input("$$[0>1;;0]").unwrap();
        let (pairs, default_branch) = assert_conditional_chain(&ast);
        assert_eq!(pairs.len(), 1);
        assert!(matches!(&pairs[0].0, AstNode::BinaryOp { .. }));
        assert_eq!(&pairs[0].1, &AstNode::Block(Vec::new(), None));
        assert!(matches!(default_branch, AstNode::Literal(Value::Int(0), _)));
    }

    // --- W[ ... ] and N[ ... ] ---

    fn assert_w_loop(ast: &AstNode) -> (&AstNode, &AstNode) {
        match ast {
            AstNode::WLoop {
                condition, body, ..
            } => (condition, body),
            other => panic!("expected WLoop, got {other:?}"),
        }
    }

    fn assert_n_loop(ast: &AstNode) -> (&AstNode, &AstNode) {
        match ast {
            AstNode::NLoop { count, body, .. } => (count, body),
            other => panic!("expected NLoop, got {other:?}"),
        }
    }

    #[test]
    fn w_loop_basic() {
        let ast = parse_input("W[1;2]").unwrap();
        let (cond, body) = assert_w_loop(&ast);
        assert!(matches!(cond, AstNode::Literal(Value::Int(1), _)));
        assert!(matches!(body, AstNode::Literal(Value::Int(2), _)));
    }

    #[test]
    fn w_loop_empty_body() {
        let ast = parse_input("W[1;]").unwrap();
        let (cond, body) = assert_w_loop(&ast);
        assert!(matches!(cond, AstNode::Literal(Value::Int(1), _)));
        assert_eq!(body, &AstNode::Block(Vec::new(), None));
    }

    #[test]
    fn n_loop_basic() {
        let ast = parse_input("N[1;2]").unwrap();
        let (count, body) = assert_n_loop(&ast);
        assert!(matches!(count, AstNode::Literal(Value::Int(1), _)));
        assert!(matches!(body, AstNode::Literal(Value::Int(2), _)));
    }

    #[test]
    fn n_loop_empty_body() {
        let ast = parse_input("N[1;]").unwrap();
        let (count, body) = assert_n_loop(&ast);
        assert!(matches!(count, AstNode::Literal(Value::Int(1), _)));
        assert_eq!(body, &AstNode::Block(Vec::new(), None));
    }

    // --- $.[ ... ] ---

    fn assert_conditional_dot(ast: &AstNode) -> (&AstNode, &AstNode) {
        match ast {
            AstNode::ConditionalDot {
                condition,
                true_branch,
                ..
            } => (condition, true_branch),
            other => panic!("expected ConditionalDot, got {other:?}"),
        }
    }

    #[test]
    fn dollar_dot_cond_basic() {
        let ast = parse_input("$.[1;2]").unwrap();
        let (cond, true_br) = assert_conditional_dot(&ast);
        assert!(matches!(cond, AstNode::Literal(Value::Int(1), _)));
        assert!(matches!(true_br, AstNode::Literal(Value::Int(2), _)));
    }

    #[test]
    fn dollar_dot_cond_empty_true_branch() {
        let ast = parse_input("$.[1;]").unwrap();
        let (cond, true_br) = assert_conditional_dot(&ast);
        assert!(matches!(cond, AstNode::Literal(Value::Int(1), _)));
        assert_eq!(true_br, &AstNode::Block(Vec::new(), None));
    }
}

// ===== Phase 2B CST integration tests =====
//
// These tests verify that:
//   * the parser-driven CST round-trips every byte of source identically across
//     a representative grammar surface,
//   * enabling CST building does not perturb the AST the parser produces,
//   * error recovery wraps the offending tokens in an `ErrorNode` rather than
//     dropping them on the floor.
//
// Structural assertions (e.g. "this BinaryExpr has these three children")
// land in Phase 2B-2 once the structural wrappings are sprinkled in. Until
// then the round-trip property is the single load-bearing invariant.
#[cfg(test)]
mod cst_integration_tests {
    use super::*;
    use crate::cst::{SyntaxKind, SyntaxNode};

    fn parse_with_cst(src: &str) -> (AstNode, crate::cst::GreenNode) {
        let tokens = Lexer::new(src).tokenize().unwrap();
        let mut p =
            Parser::new_with_builtins(tokens, src.to_string(), crate::builtins::Builtins::new());
        p.enable_cst();
        let ast = p.parse().expect("parse");
        let cst = p.take_cst().expect("cst was enabled");
        (ast, cst)
    }

    fn parse_without_cst(src: &str) -> AstNode {
        let tokens = Lexer::new(src).tokenize().unwrap();
        let mut p =
            Parser::new_with_builtins(tokens, src.to_string(), crate::builtins::Builtins::new());
        p.parse().expect("parse")
    }

    fn round_trip(src: &str) {
        let (ast_with, cst) = parse_with_cst(src);
        let ast_without = parse_without_cst(src);
        assert_eq!(cst.text(), src, "CST text round-trip mismatch on `{src}`",);
        assert_eq!(
            ast_with, ast_without,
            "AST differs depending on whether CST is enabled (for `{src}`)",
        );
        assert_eq!(cst.kind(), SyntaxKind::Root);
    }

    #[test]
    fn empty_source() {
        round_trip("");
    }

    #[test]
    fn whitespace_only() {
        round_trip("   ");
        round_trip("\n\n");
        round_trip(" \t\n  \t");
    }

    #[test]
    fn simple_expressions() {
        round_trip("1");
        round_trip("1+2");
        round_trip("1 + 2");
        round_trip("a:1");
        round_trip("foo[1; 2; 3]");
    }

    #[test]
    fn comments_survive_round_trip() {
        round_trip("// hi\n1+2");
        round_trip("1 /* inline */ + 2");
        round_trip("1+2 // trailing\n");
    }

    #[test]
    fn function_block_round_trips() {
        round_trip("{[a;b]a+b}");
        round_trip("{[a; b]\n  a + b\n}");
    }

    #[test]
    fn control_flow_round_trips() {
        round_trip("$[c;t;f]");
        round_trip("$.[c;t]");
        round_trip("$$[c1;t1;c2;t2;d]");
        round_trip("W[c;b]");
        round_trip("N[10;@b]");
        round_trip("[1;2;3]");
        round_trip("B[1;2;3]");
        round_trip("B.[1;2;3]");
    }

    #[test]
    fn bare_bracket_block_parses_as_block_expr() {
        let ast = parse_without_cst("[x:1;x+1]");
        let AstNode::BlockExpr(stmts, _) = ast else {
            panic!("expected BlockExpr, got {ast:?}");
        };

        assert_eq!(stmts.len(), 2);
        assert!(matches!(stmts[0], AstNode::Assignment { .. }));
        assert!(matches!(stmts[1], AstNode::BinaryOp { .. }));

        let ast = parse_without_cst("B.[x:1;x+1]");
        assert!(matches!(ast, AstNode::BlockExpr(_, _)));
    }

    #[test]
    fn assignments_round_trip() {
        round_trip("x:1");
        round_trip("x+:1");
        round_trip("(a;b):1,2");
    }

    #[test]
    fn pipes_round_trip() {
        round_trip("xs|sum");
        round_trip("xs|.print");
        round_trip("xs|f|g|h");
        round_trip("xs|a:|sum");
    }

    #[test]
    fn pipe_checkpoint_assignment_uses_pipe_input() {
        let ast = parse_without_cst("42|a:");
        let AstNode::Pipe { effect, .. } = ast else {
            panic!("expected pipe, got {ast:?}");
        };
        let AstNode::Assignment { name, value, .. } = effect.as_ref() else {
            panic!("expected assignment pipe RHS, got {effect:?}");
        };

        assert_eq!(name, "a");
        assert!(matches!(value.as_ref(), AstNode::PipeInput));
    }

    #[test]
    fn fstring_round_trips() {
        round_trip(r#"@f"x={1+2}""#);
        round_trip(r#"echo@f"v={value!>10}""#);
    }

    #[test]
    fn unicode_round_trips() {
        round_trip("\"héllo, 世界\"");
        round_trip("// 中文\nx:1");
    }

    #[test]
    fn error_recovery_wraps_in_error_node() {
        // The parser's existing recovery turns `x +` into AstNode::Error;
        // the CST must wrap the same tokens in an ErrorNode so coverage
        // stays total.
        let src = "a:1\nx +\nb:2";
        let (ast, cst) = parse_with_cst(src);
        assert_eq!(cst.text(), src);
        // AST has the middle stmt as Error.
        let stmts = match ast {
            AstNode::Block(s, _) => s,
            other => vec![other],
        };
        assert!(matches!(&stmts[1], AstNode::Error(..)));
        // CST has at least one ErrorNode descendant.
        let root = SyntaxNode::new_root(cst);
        let saw_error = root
            .descendants()
            .any(|n| n.kind() == SyntaxKind::ErrorNode);
        assert!(
            saw_error,
            "expected an ErrorNode somewhere under Root for `{src}`",
        );
    }

    #[test]
    fn token_at_offset_finds_lexer_kind() {
        // Sanity check that the byte positions in the green tree line up
        // with the source: pick a few offsets and verify the token at each
        // is the expected kind.
        let src = "1 + foo";
        //          0 1 2 3 4 5 6
        let (_, cst) = parse_with_cst(src);
        let root = SyntaxNode::new_root(cst);
        let t = root.token_at_offset(0).expect("at 0");
        assert_eq!(t.kind(), SyntaxKind::IntLit);
        assert_eq!(t.text(), "1");
        let t = root.token_at_offset(2).expect("at 2");
        assert_eq!(t.kind(), SyntaxKind::Plus);
        let t = root.token_at_offset(4).expect("at 4");
        assert_eq!(t.kind(), SyntaxKind::Ident);
        assert_eq!(t.text(), "foo");
    }

    #[test]
    fn structural_wraps_match_construct_kinds() {
        // Build a snippet exercising every wrap kind we've added in Phase 2B
        // and assert each kind shows up at least once in the green tree.
        let src = r#"a:1+2; xs|sum; (1;2); (`k:1); {[x;y]x+y}; W[c;b]; N[3;@b]; [1]; B[1]; $[c;t;f]; $.[c;t]; $$[c;t;d]; foo[1;2]; foo!arg; bar:2"#;
        let (_, cst) = parse_with_cst(src);
        let root = SyntaxNode::new_root(cst);
        let mut seen = std::collections::HashSet::new();
        for n in root.descendants() {
            seen.insert(n.kind());
        }
        let expected = [
            SyntaxKind::AssignExpr,
            SyntaxKind::BinaryExpr,
            SyntaxKind::PipeExpr,
            SyntaxKind::ListExpr,
            SyntaxKind::DictExpr,
            SyntaxKind::DictPair,
            SyntaxKind::FunctionExpr,
            SyntaxKind::ParamList,
            SyntaxKind::Param,
            SyntaxKind::WLoopExpr,
            SyntaxKind::NLoopExpr,
            SyntaxKind::BlockExpr,
            SyntaxKind::CondExpr,
            SyntaxKind::CondDotExpr,
            SyntaxKind::CondChainExpr,
            SyntaxKind::PostfixExpr,
            SyntaxKind::ArgList,
            SyntaxKind::VarExpr,
            SyntaxKind::LiteralExpr,
        ];
        for k in expected {
            assert!(
                seen.contains(&k),
                "expected to see a {} node in the CST of `{src}`, only saw: {:?}",
                k.name(),
                seen.iter().map(|k| k.name()).collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn binary_wrap_holds_three_relevant_children() {
        // `1 + 2` should yield BinaryExpr with three meaningful children:
        // LiteralExpr, Plus token, LiteralExpr (modulo whitespace tokens).
        let src = "1 + 2";
        let (_, cst) = parse_with_cst(src);
        let root = SyntaxNode::new_root(cst);
        let bin = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::BinaryExpr)
            .expect("BinaryExpr present");
        let kinds: Vec<_> = bin
            .children_with_tokens()
            .filter(|e| !matches!(e.kind(), SyntaxKind::Whitespace))
            .map(|e| e.kind())
            .collect();
        assert_eq!(
            kinds,
            vec![
                SyntaxKind::LiteralExpr,
                SyntaxKind::Plus,
                SyntaxKind::LiteralExpr
            ],
        );
    }

    #[test]
    fn postfix_wraps_arglist_inside() {
        // `f[1;2]` -> PostfixExpr { VarExpr(f), ArgList[ ; 1 ; 2 ; ] }
        let src = "f[1;2]";
        let (_, cst) = parse_with_cst(src);
        let root = SyntaxNode::new_root(cst);
        let pf = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::PostfixExpr)
            .expect("PostfixExpr present");
        let arglist = pf
            .children()
            .find(|n| n.kind() == SyntaxKind::ArgList)
            .expect("ArgList inside PostfixExpr");
        // ArgList must start with `[` and end with `]`.
        let first = arglist.first_token().expect("first token");
        let last = arglist.last_token().expect("last token");
        assert_eq!(first.kind(), SyntaxKind::LBrack);
        assert_eq!(last.kind(), SyntaxKind::RBrack);
        assert_eq!(arglist.text(), "[1;2]");
    }

    #[test]
    fn depth_modifier_attaches_to_postfix_call() {
        let src = "has?@1[(1;2);2]";
        let (ast, cst) = parse_with_cst(src);
        let AstNode::Postfix {
            object,
            items,
            depth,
            ..
        } = ast
        else {
            panic!("expected Postfix, got {ast:?}");
        };
        assert!(matches!(object.as_ref(), AstNode::Variable(name, _) if name == "has?"));
        assert_eq!(items.len(), 2);
        assert_eq!(depth, Some(1));

        let root = SyntaxNode::new_root(cst);
        assert!(
            root.descendants_with_tokens()
                .any(|elem| elem.kind() == SyntaxKind::AtDepth),
            "expected AtDepth token in CST",
        );
    }

    #[test]
    fn function_param_list_is_isolated() {
        // `{[x;y]x+y}` -> FunctionExpr containing ParamList containing Params.
        let src = "{[x;y]x+y}";
        let (_, cst) = parse_with_cst(src);
        let root = SyntaxNode::new_root(cst);
        let func = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FunctionExpr)
            .expect("FunctionExpr present");
        let params = func
            .descendants()
            .find(|n| n.kind() == SyntaxKind::ParamList)
            .expect("ParamList present");
        assert_eq!(params.text(), "[x;y]");
        let names: Vec<_> = params
            .children()
            .filter(|n| n.kind() == SyntaxKind::Param)
            .map(|n| n.text())
            .collect();
        assert_eq!(names, vec!["x".to_string(), "y".to_string()]);
    }

    /// Returns the parser-stored span field. `None` is valid for synthetic
    /// nodes that do not correspond to a source range.
    fn stored_span(node: &AstNode) -> AstSpan {
        match node {
            AstNode::Literal(_, s)
            | AstNode::Variable(_, s)
            | AstNode::OuterVariable(_, s)
            | AstNode::Error(_, s) => *s,
            AstNode::BinaryOp { span, .. }
            | AstNode::ComparisonChain { span, .. }
            | AstNode::Range { span, .. }
            | AstNode::Assignment { span, .. }
            | AstNode::OuterAssignment { span, .. }
            | AstNode::Postfix { span, .. }
            | AstNode::Pipe { span, .. }
            | AstNode::PipeTap { span, .. }
            | AstNode::CallName { span, .. }
            | AstNode::CallAnonymous { span, .. }
            | AstNode::Index { span, .. }
            | AstNode::IndexAssign { span, .. }
            | AstNode::MutatingIndex { span, .. }
            | AstNode::MutatingIndexAssign { span, .. }
            | AstNode::Function { span, .. }
            | AstNode::Assert { span, .. }
            | AstNode::Debug { span, .. }
            | AstNode::Pause { span, .. }
            | AstNode::FString { span, .. }
            | AstNode::UnaryOp { span, .. }
            | AstNode::Group { span, .. }
            | AstNode::BlockExpr(_, span)
            | AstNode::Conditional { span, .. }
            | AstNode::ConditionalDot { span, .. }
            | AstNode::ConditionalChain { span, .. }
            | AstNode::WLoop { span, .. }
            | AstNode::NLoop { span, .. }
            | AstNode::NamedArg { span, .. }
            | AstNode::UnpackAssignment { span, .. }
            | AstNode::Return(_, span)
            | AstNode::Try(_, span)
            | AstNode::Break(span)
            | AstNode::Continue(span)
            | AstNode::Ellipsis(span) => *span,
            AstNode::List(_, span)
            | AstNode::Cat(_, span)
            | AstNode::Block(_, span)
            | AstNode::Dict(_, span) => *span,
            AstNode::PipeInput => None,
        }
    }

    /// For every AST node whose variant carries a parser-stored span field,
    /// the byte range must match the `text_range()` of *some* descendant
    /// green node in the CST.
    #[test]
    fn ast_spans_correspond_to_cst_ranges() {
        let snippets: &[&str] = &[
            "x:1",
            "x+:1",
            "'x:1",
            "(a;b):1,2",
            "foo[1;2]:3",
            "$[c;t;f]",
            "$.[c;t]",
            "$$[c1;t1;d]",
            "W[c;b]",
            "N[3;@b]",
            "B[1;2]",
            "[1;2]",
            "S(1;2;3)",
            "(1;2;3)",
            "(`k:1)",
            "-x",
            "#xs",
            "~y",
            "foo[1;2]",
            "x|f",
        ];

        // Collect every byte-range present anywhere in the CST so we can
        // do a single set-membership check per AST span.
        for src in snippets {
            let (ast, cst) = parse_with_cst(src);
            let root = SyntaxNode::new_root(cst);
            let mut cst_ranges: std::collections::HashSet<(usize, usize)> =
                std::collections::HashSet::new();
            for elem in root.descendants_with_tokens() {
                let r = elem.text_range();
                cst_ranges.insert((r.start() as usize, r.end() as usize));
            }
            let r = root.text_range();
            cst_ranges.insert((r.start() as usize, r.end() as usize));

            // Walk every AST node, ensure parser-stored spans align with
            // a CST range.
            fn check(
                node: &AstNode,
                cst_ranges: &std::collections::HashSet<(usize, usize)>,
                src: &str,
            ) {
                if let Some(span) = stored_span(node) {
                    assert!(
                        cst_ranges.contains(&span),
                        "stored AST span {:?} does not match any CST byte range \
                         for source `{src}` (variant: {})\nfull AST: {node:#?}\nCST ranges: {:?}",
                        span,
                        ast_variant_name(node),
                        cst_ranges,
                    );
                }
                for child in collect_children(node) {
                    check(child, cst_ranges, src);
                }
            }
            check(&ast, &cst_ranges, src);
        }
    }

    /// Returns the AstNode variant's name as a short string.
    fn ast_variant_name(node: &AstNode) -> &'static str {
        match node {
            AstNode::Literal(..) => "Literal",
            AstNode::Variable(..) => "Variable",
            AstNode::OuterVariable(..) => "OuterVariable",
            AstNode::Function { .. } => "Function",
            AstNode::Conditional { .. } => "Conditional",
            AstNode::ConditionalDot { .. } => "ConditionalDot",
            AstNode::ConditionalChain { .. } => "ConditionalChain",
            AstNode::WLoop { .. } => "WLoop",
            AstNode::NLoop { .. } => "NLoop",
            AstNode::BlockExpr(..) => "BlockExpr",
            AstNode::List(..) => "List",
            AstNode::Cat(..) => "Cat",
            AstNode::Dict(..) => "Dict",

            AstNode::Group { .. } => "Group",
            AstNode::FString { .. } => "FString",
            AstNode::Return(..) => "Return",
            AstNode::Break(_) => "Break",
            AstNode::Continue(_) => "Continue",
            AstNode::Assert { .. } => "Assert",
            AstNode::Debug { .. } => "Debug",
            AstNode::Pause { .. } => "Pause",
            AstNode::Try(..) => "Try",
            AstNode::Ellipsis(_) => "Ellipsis",
            AstNode::Error(..) => "Error",
            AstNode::PipeInput => "PipeInput",
            AstNode::Postfix { .. } => "Postfix",
            AstNode::CallName { .. } => "CallName",
            AstNode::CallAnonymous { .. } => "CallAnonymous",
            AstNode::Index { .. } => "Index",
            AstNode::MutatingIndex { .. } => "MutatingIndex",
            AstNode::IndexAssign { .. } => "IndexAssign",
            AstNode::MutatingIndexAssign { .. } => "MutatingIndexAssign",
            AstNode::Assignment { .. } => "Assignment",
            AstNode::OuterAssignment { .. } => "OuterAssignment",
            AstNode::UnpackAssignment { .. } => "UnpackAssignment",
            AstNode::BinaryOp { .. } => "BinaryOp",
            AstNode::UnaryOp { .. } => "UnaryOp",
            AstNode::ComparisonChain { .. } => "ComparisonChain",
            AstNode::Range { .. } => "Range",
            AstNode::Pipe { .. } => "Pipe",
            AstNode::PipeTap { .. } => "PipeTap",
            AstNode::NamedArg { .. } => "NamedArg",
            AstNode::Block(..) => "Block",
        }
    }

    /// Helper for [`ast_spans_correspond_to_cst_ranges`]. Returns
    /// references to every direct child AST node of `node` that itself can
    /// carry a span. Kept minimal -- we don't recurse here, the caller does.
    fn collect_children(node: &AstNode) -> Vec<&AstNode> {
        let mut out = Vec::new();
        match node {
            AstNode::Block(items, _) | AstNode::List(items, _) | AstNode::Cat(items, _) => {
                out.extend(items.iter())
            }
            AstNode::BlockExpr(items, _) => out.extend(items.iter()),
            AstNode::Dict(pairs, _) => out.extend(pairs.iter().map(|(_, v)| v)),
            AstNode::Assignment { value, .. } | AstNode::OuterAssignment { value, .. } => {
                out.push(value)
            }
            AstNode::UnpackAssignment { lhs, rhs, .. } => {
                out.extend(lhs.iter());
                out.push(rhs);
            }
            AstNode::IndexAssign {
                object,
                index,
                value,
                ..
            }
            | AstNode::MutatingIndexAssign {
                object,
                index,
                value,
                ..
            } => {
                out.push(object);
                out.push(index);
                out.push(value);
            }
            AstNode::Index { object, index, .. } | AstNode::MutatingIndex { object, index, .. } => {
                out.push(object);
                out.push(index);
            }
            AstNode::Postfix { object, items, .. } => {
                out.push(object);
                out.extend(items.iter());
            }
            AstNode::CallName { args, .. } => out.extend(args.iter()),
            AstNode::CallAnonymous { object, args, .. } => {
                out.push(object);
                out.extend(args.iter());
            }
            AstNode::BinaryOp { left, right, .. } => {
                out.push(left);
                out.push(right);
            }
            AstNode::UnaryOp { operand, .. } => out.push(operand),
            AstNode::ComparisonChain { first, rest, .. } => {
                out.push(first);
                out.extend(rest.iter().map(|(_, n)| n));
            }
            AstNode::Range {
                start, end, step, ..
            } => {
                out.push(start);
                out.push(end);
                if let Some(s) = step {
                    out.push(s);
                }
            }
            AstNode::Group { expr, .. } => out.push(expr),
            AstNode::Conditional {
                condition,
                true_branch,
                false_branch,
                ..
            } => {
                out.push(condition);
                out.push(true_branch);
                if let Some(fb) = false_branch {
                    out.push(fb);
                }
            }
            AstNode::ConditionalDot {
                condition,
                true_branch,
                ..
            } => {
                out.push(condition);
                out.push(true_branch);
            }
            AstNode::ConditionalChain {
                pairs,
                default_branch,
                ..
            } => {
                for (c, b) in pairs {
                    out.push(c);
                    out.push(b);
                }
                out.push(default_branch);
            }
            AstNode::WLoop {
                condition, body, ..
            } => {
                out.push(condition);
                out.push(body);
            }
            AstNode::NLoop { count, body, .. } => {
                out.push(count);
                out.push(body);
            }
            AstNode::Function { body, .. } => out.push(body),
            AstNode::Pipe { input, effect, .. } => {
                out.push(input);
                out.push(effect);
            }
            AstNode::PipeTap { input, effect, .. } => {
                out.push(input);
                out.push(effect);
            }
            AstNode::FString { parts, .. } => {
                for part in parts {
                    if let crate::astnode::FStringPart::Expr {
                        expr, spec_exprs, ..
                    } = part
                    {
                        out.push(expr);
                        out.extend(spec_exprs.iter());
                    }
                }
            }
            AstNode::Return(Some(e), _) | AstNode::Try(e, _) => out.push(e),
            AstNode::Assert { expr, .. } | AstNode::Debug { expr, .. } => out.push(expr),
            AstNode::Pause {
                expr: Some(expr), ..
            } => out.push(expr),
            AstNode::NamedArg { value, .. } => out.push(value),
            AstNode::Literal(_, _)
            | AstNode::Variable(_, _)
            | AstNode::OuterVariable(_, _)
            | AstNode::Break(_)
            | AstNode::Continue(_)
            | AstNode::Return(None, _)
            | AstNode::Pause { expr: None, .. }
            | AstNode::PipeInput
            | AstNode::Ellipsis(_)
            | AstNode::Error(_, _) => {}
        }
        out
    }

    #[test]
    fn corpus_round_trip_through_parser() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("e");
        if !dir.exists() {
            return;
        }
        let entries =
            std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot read {dir:?}: {e}"));
        let mut checked = 0;
        for entry in entries {
            let entry = entry.expect("read_dir entry");
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("wq") {
                continue;
            }
            let src = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {path:?}: {e}"));
            // Some examples in the corpus may legitimately contain syntax
            // the strict parser rejects (e.g. wq-only meta lines). Use the
            // tolerant lexer path; if the parse itself errors, just skip.
            let tokens = match Lexer::new(&src).tokenize() {
                Ok(t) => t,
                Err(_) => continue,
            };
            let mut p =
                Parser::new_with_builtins(tokens, src.clone(), crate::builtins::Builtins::new());
            p.enable_cst();
            let _ = p.parse();
            let cst = match p.take_cst() {
                Some(c) => c,
                None => continue,
            };
            assert_eq!(
                cst.text(),
                src,
                "CST round-trip mismatch on {}",
                path.display(),
            );
            checked += 1;
        }
        assert!(checked > 0, "no .wq examples were checked under {dir:?}");
    }
}
