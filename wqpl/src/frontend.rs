//! Stateless parser and language-tooling services.
//!
//! [`Frontend`] owns only immutable builtin configuration. It does not create
//! or retain evaluator, VM, random generator, profiler, debugger, or runtime
//! environment state, so one instance can be shared by editors and language
//! servers.

use std::sync::Arc;

use crate::ast::{AstNode, FStringPart};
use crate::builtins::{BuiltinPreset, Builtins};
use crate::cst::{GreenChild, GreenNode, GreenNodeBuilder, GreenToken, SyntaxKind, SyntaxNode};
use crate::lex::Lexer;
use crate::parse::resolve::Resolver;
use crate::parse::{Parser, fold};
use crate::script::{
    ScriptDirective, ScriptItem, ScriptSpan, might_have_script_meta, parse_script_items,
};
use crate::style::{ColorMode, TextStyle, paint};
use crate::symbol::SymbolIndex;
use crate::token::{Token, TokenType, rebase_token};
use crate::value::WqResult;
use crate::wqerror::{SourceCtx, WqError, WqErrorType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxDisplayKind {
    Ast,
    Cst,
}

impl SyntaxDisplayKind {
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.trim();
        if name.eq_ignore_ascii_case("ast") {
            Some(Self::Ast)
        } else if name.eq_ignore_ascii_case("cst") {
            Some(Self::Cst)
        } else {
            None
        }
    }
}

/// Immutable language frontend configured with a set of builtins.
///
/// Cloning a frontend is cheap because its builtin configuration is shared.
#[derive(Clone)]
pub struct Frontend {
    builtins: Arc<Builtins>,
}

impl Frontend {
    pub fn new(builtins: Builtins) -> Self {
        Self {
            builtins: Arc::new(builtins),
        }
    }

    pub fn with_preset(preset: BuiltinPreset) -> Self {
        Self::new(Builtins::with_preset(preset))
    }

    pub fn builtins(&self) -> &Builtins {
        &self.builtins
    }

    /// Parse and analyze definitions, uses, literals, and recoverable errors.
    pub fn analyze_symbols(&self, input: &str) -> WqResult<SymbolIndex> {
        if might_have_script_meta(input) {
            let items = parse_script_items(input);
            if has_script_meta(&items) {
                let (ast, eof_errors) = self.parse_script_ast(input, &items)?;
                let ast = Resolver::with_builtins(self.builtins_cloned()).resolve(ast);
                let mut index = SymbolIndex::analyze(&ast, self.builtins());
                for eof_error in eof_errors {
                    let span = eof_error.span.unwrap_or((input.len(), input.len()));
                    index.errors.push((span, eof_error));
                }
                for item in &items {
                    let ScriptItem::Directive(directive) = item else {
                        continue;
                    };
                    if let Some(error) = unknown_script_directive_error(input, directive) {
                        let span = error.span.unwrap_or((input.len(), input.len()));
                        index.errors.push((span, error));
                    }
                }
                index.errors.sort_by_key(|(span, _)| *span);
                return Ok(index);
            }
        }

        self.analyze_symbols_code(input)
    }

    /// Tokenize all recoverable code regions, including lexical error tokens.
    ///
    /// Shebangs and loader directives are script metadata rather than wq code,
    /// so they are intentionally omitted from the returned token stream.
    pub fn tokenize_recovery(&self, input: &str) -> Vec<Token> {
        if might_have_script_meta(input) {
            let items = parse_script_items(input);
            if has_script_meta(&items) {
                let mut tokens = Vec::new();
                for item in &items {
                    let ScriptItem::Code { span } = item else {
                        continue;
                    };
                    let mut lexer = Lexer::new(&input[span.as_range()]);
                    let chunk_tokens = lexer.tokenize_recovery();
                    for mut token in chunk_tokens {
                        if matches!(token.token_type, TokenType::Eof) {
                            continue;
                        }
                        rebase_token(&mut token, input, span.start);
                        tokens.push(token);
                    }
                }
                tokens.push(eof_token_for(input));
                return tokens;
            }
        }

        Lexer::new(input).tokenize_recovery()
    }

    /// Parse `input` into an AST and a byte-preserving green CST.
    pub fn parse_with_cst(&self, input: &str) -> WqResult<(AstNode, GreenNode)> {
        if might_have_script_meta(input) {
            let items = parse_script_items(input);
            if has_script_meta(&items) {
                return self.parse_script_with_cst(input, &items);
            }
        }

        let tokens = Lexer::new(input).tokenize()?;
        let mut parser =
            Parser::new_with_builtins(tokens, input.to_string(), self.builtins_cloned());
        parser.enable_cst();
        let ast = parser.parse()?;
        let cst = parser
            .take_cst()
            .expect("enable_cst was called, so take_cst yields Some");
        Ok((ast, cst))
    }

    /// Parse `input` while reusing unaffected statement nodes from `previous`.
    ///
    /// The previous tree carries its original source, so callers only need to
    /// retain the last successful [`GreenNode`]. Script metadata is reparsed
    /// from scratch because directives divide the source into independent code
    /// regions.
    pub fn parse_with_cst_using_cache(
        &self,
        input: &str,
        previous: &GreenNode,
    ) -> WqResult<(AstNode, GreenNode)> {
        if might_have_script_meta(input) {
            let items = parse_script_items(input);
            if has_script_meta(&items) {
                return self.parse_script_with_cst(input, &items);
            }
        }

        let previous_text = previous.text();
        let (old_start, old_end, new_start, new_end) =
            compute_dirty_byte_range(&previous_text, input);
        let tokens = Lexer::new(input).tokenize()?;
        let mut parser =
            Parser::new_with_builtins(tokens, input.to_string(), self.builtins_cloned());
        parser.enable_cst_with_cache(previous, old_start, old_end, new_start, new_end);
        let ast = parser.parse()?;
        let cst = parser
            .take_cst()
            .expect("enable_cst_with_cache was called, so take_cst yields Some");
        Ok((ast, cst))
    }

    /// Render the folded AST or CST without evaluating source code.
    pub fn format_syntax_display(
        &self,
        input: &str,
        kind: SyntaxDisplayKind,
        color_mode: ColorMode,
    ) -> WqResult<String> {
        self.format_syntax_display_with_palette(input, kind, color_mode, false)
    }

    /// Render the folded AST or CST with semantic ANSI colors for CSS
    /// consumers.
    pub fn format_syntax_display_semantic_ansi(
        &self,
        input: &str,
        kind: SyntaxDisplayKind,
        color_mode: ColorMode,
    ) -> WqResult<String> {
        self.format_syntax_display_with_palette(input, kind, color_mode, true)
    }

    fn format_syntax_display_with_palette(
        &self,
        input: &str,
        kind: SyntaxDisplayKind,
        color_mode: ColorMode,
        semantic_ansi: bool,
    ) -> WqResult<String> {
        let (ast, cst) = self.parse_with_cst(input)?;
        let (header, body) = match kind {
            SyntaxDisplayKind::Ast => {
                let ast = Resolver::with_builtins(self.builtins_cloned()).resolve(ast);
                let ast = fold::fold(ast);
                let body = if semantic_ansi {
                    ast.sexpr_pretty_with_source_semantic_ansi(input)
                } else {
                    ast.sexpr_pretty_with_source(input)
                };
                ("AST @ fold - final", body)
            }
            SyntaxDisplayKind::Cst => ("CST", SyntaxNode::new_root(cst).pretty_print()),
        };

        let mut output = paint(header, TextStyle::new().bold().underline(), color_mode);
        output.push('\n');
        output.push_str(&body);
        Ok(output)
    }

    /// Return whether `input` is a complete wq snippet.
    ///
    /// Syntax errors that do not require more input are complete. Lexer or
    /// parser EOF errors are incomplete.
    pub fn is_complete_input(&self, input: &str) -> bool {
        if might_have_script_meta(input) {
            let items = parse_script_items(input);
            if has_script_meta(&items) {
                if has_unknown_script_directive(&items) {
                    return true;
                }
                let trailing_code = items.iter().rev().find_map(|item| match item {
                    ScriptItem::Code { span } => Some(*span),
                    ScriptItem::Shebang { .. } | ScriptItem::Directive(_) => None,
                });
                return trailing_code
                    .map(|span| self.is_complete_code(&input[span.as_range()]))
                    .unwrap_or(true);
            }
        }

        self.is_complete_code(input)
    }

    fn is_complete_code(&self, input: &str) -> bool {
        let tokens = match Lexer::new(input).tokenize() {
            Ok(tokens) => tokens,
            Err(error) => return error.err_type != WqErrorType::Eof,
        };
        let mut parser =
            Parser::new_with_builtins(tokens, input.to_string(), self.builtins_cloned());
        match parser.parse() {
            Ok(_) => parser.eof_error().is_none(),
            Err(error) => error.err_type != WqErrorType::Eof,
        }
    }

    fn builtins_cloned(&self) -> Builtins {
        self.builtins.as_ref().clone()
    }

    fn analyze_symbols_code(&self, input: &str) -> WqResult<SymbolIndex> {
        let tokens = Lexer::new(input).tokenize()?;
        let mut parser =
            Parser::new_with_builtins(tokens, input.to_string(), self.builtins_cloned());
        let ast = parser.parse()?;
        let ast = Resolver::with_builtins(self.builtins_cloned()).resolve(ast);
        let mut index = SymbolIndex::analyze(&ast, self.builtins());
        if let Some(eof_error) = parser.eof_error() {
            let span = eof_error.span.unwrap_or((input.len(), input.len()));
            index.errors.push((span, eof_error.clone()));
        }
        Ok(index)
    }

    fn parse_script_ast(
        &self,
        input: &str,
        items: &[ScriptItem],
    ) -> WqResult<(AstNode, Vec<WqError>)> {
        let mut statements = Vec::new();
        let mut eof_errors = Vec::new();
        for item in items {
            let ScriptItem::Code { span } = item else {
                continue;
            };
            let (ast, eof_error) = self.parse_script_code_ast(input, *span)?;
            push_script_ast(&mut statements, ast);
            if let Some(eof_error) = eof_error {
                eof_errors.push(eof_error);
            }
        }
        Ok((script_ast_from_statements(statements), eof_errors))
    }

    fn parse_script_code_ast(
        &self,
        input: &str,
        span: ScriptSpan,
    ) -> WqResult<(AstNode, Option<WqError>)> {
        let source = &input[span.as_range()];
        let tokens = Lexer::new(source).with_ctx(input, span.start).tokenize()?;
        let mut parser =
            Parser::new_with_builtins(tokens, source.to_string(), self.builtins_cloned());
        let mut ast = match parser.parse() {
            Ok(ast) => ast,
            Err(mut error) => {
                offset_error_span(&mut error, span.start);
                attach_full_source_context(&mut error, input);
                return Err(error);
            }
        };
        Parser::offset_spans(&mut ast, span.start);
        attach_full_source_context_to_ast_errors(&mut ast, input);
        let eof_error = parser.eof_error().cloned().map(|mut error| {
            offset_error_span(&mut error, span.start);
            attach_full_source_context(&mut error, input);
            error
        });
        Ok((ast, eof_error))
    }

    fn parse_script_with_cst(
        &self,
        input: &str,
        items: &[ScriptItem],
    ) -> WqResult<(AstNode, GreenNode)> {
        if let Some(error) = items.iter().find_map(|item| {
            let ScriptItem::Directive(directive) = item else {
                return None;
            };
            unknown_script_directive_error(input, directive)
        }) {
            return Err(error);
        }

        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::Root);
        let mut cursor = 0usize;
        let mut statements = Vec::new();

        for item in items {
            let span = item.span();
            if span.start > cursor {
                push_script_trivia(&mut builder, &input[cursor..span.start]);
            }

            match item {
                ScriptItem::Shebang { span } => {
                    push_script_line_node(&mut builder, input, *span, SyntaxKind::Shebang);
                }
                ScriptItem::Directive(directive) => {
                    push_script_line_node(
                        &mut builder,
                        input,
                        directive.span(),
                        SyntaxKind::ScriptDirective,
                    );
                }
                ScriptItem::Code { span } => {
                    let (ast, cst) = self.parse_script_code_cst(input, *span)?;
                    push_script_ast(&mut statements, ast);
                    append_root_children(&mut builder, &cst);
                }
            }

            cursor = span.end;
        }

        if cursor < input.len() {
            push_script_trivia(&mut builder, &input[cursor..]);
        }

        builder.finish_node();
        Ok((script_ast_from_statements(statements), builder.finish()))
    }

    fn parse_script_code_cst(
        &self,
        input: &str,
        span: ScriptSpan,
    ) -> WqResult<(AstNode, GreenNode)> {
        let source = &input[span.as_range()];
        let tokens = Lexer::new(source).with_ctx(input, span.start).tokenize()?;
        let mut parser =
            Parser::new_with_builtins(tokens, source.to_string(), self.builtins_cloned());
        parser.enable_cst();
        let mut ast = match parser.parse() {
            Ok(ast) => ast,
            Err(mut error) => {
                offset_error_span(&mut error, span.start);
                attach_full_source_context(&mut error, input);
                return Err(error);
            }
        };
        Parser::offset_spans(&mut ast, span.start);
        attach_full_source_context_to_ast_errors(&mut ast, input);
        let cst = parser
            .take_cst()
            .expect("enable_cst was called, so take_cst yields Some");
        Ok((ast, cst))
    }
}

impl Default for Frontend {
    fn default() -> Self {
        Self::new(Builtins::new())
    }
}

impl From<Builtins> for Frontend {
    fn from(builtins: Builtins) -> Self {
        Self::new(builtins)
    }
}

fn has_script_meta(items: &[ScriptItem]) -> bool {
    items
        .iter()
        .any(|item| matches!(item, ScriptItem::Shebang { .. } | ScriptItem::Directive(_)))
}

fn has_unknown_script_directive(items: &[ScriptItem]) -> bool {
    items
        .iter()
        .any(|item| matches!(item, ScriptItem::Directive(ScriptDirective::Unknown { .. })))
}

fn unknown_script_directive_error(input: &str, directive: &ScriptDirective) -> Option<WqError> {
    let ScriptDirective::Unknown { text, span } = directive else {
        return None;
    };
    let span = (span.start, span.end);
    Some(
        WqError::new(WqErrorType::Syntax)
            .src("script directive")
            .msg(format!(
                "unknown or invalid script directive '{}'",
                text.trim()
            ))
            .span(Some(span))
            .source_ctx(input, "?"),
    )
}

fn push_script_ast(statements: &mut Vec<AstNode>, ast: AstNode) {
    match ast {
        AstNode::Block(nodes, _) => statements.extend(nodes),
        node => statements.push(node),
    }
}

fn script_ast_from_statements(mut statements: Vec<AstNode>) -> AstNode {
    if statements.len() == 1 {
        statements.remove(0)
    } else {
        let span = ast_span_for_items(&statements);
        AstNode::Block(statements, span)
    }
}

fn ast_span_for_items(items: &[AstNode]) -> Option<(usize, usize)> {
    match (
        items.first().and_then(AstNode::span),
        items.last().and_then(AstNode::span),
    ) {
        (Some((start, _)), Some((_, end))) => Some((start, end)),
        (Some(span), None) | (None, Some(span)) => Some(span),
        (None, None) => None,
    }
}

fn push_script_line_node(
    builder: &mut GreenNodeBuilder,
    input: &str,
    span: ScriptSpan,
    kind: SyntaxKind,
) {
    let text = &input[span.as_range()];
    let (line, newline) = text
        .strip_suffix('\n')
        .map_or((text, None), |line| (line, Some("\n")));
    builder.start_node(kind);
    if !line.is_empty() {
        builder.token(SyntaxKind::ScriptLine, line);
    }
    builder.finish_node();
    if let Some(newline) = newline {
        builder.token(SyntaxKind::Newline, newline);
    }
}

fn push_script_trivia(builder: &mut GreenNodeBuilder, text: &str) {
    let mut line_start = 0usize;
    for (idx, ch) in text.char_indices() {
        if ch != '\n' {
            continue;
        }
        push_script_trivia_line(builder, &text[line_start..idx]);
        builder.token(SyntaxKind::Newline, "\n");
        line_start = idx + ch.len_utf8();
    }
    if line_start < text.len() {
        push_script_trivia_line(builder, &text[line_start..]);
    }
}

fn push_script_trivia_line(builder: &mut GreenNodeBuilder, line: &str) {
    let trimmed = line.trim_start_matches([' ', '\t', '\r']);
    let leading_len = line.len() - trimmed.len();
    let rest = &line[leading_len..];
    if rest.starts_with("//") {
        if leading_len > 0 {
            builder.token(SyntaxKind::Whitespace, &line[..leading_len]);
        }
        builder.token(SyntaxKind::Comment, rest);
    } else if !line.is_empty() {
        builder.token(SyntaxKind::Whitespace, line);
    }
}

fn append_root_children(builder: &mut GreenNodeBuilder, root: &GreenNode) {
    for child in root.children() {
        append_green_child(builder, child);
    }
}

fn append_green_child(builder: &mut GreenNodeBuilder, child: &GreenChild) {
    match child {
        GreenChild::Node(node) => builder.append_node(node.clone()),
        GreenChild::Token(token) => append_green_token(builder, token),
    }
}

fn append_green_token(builder: &mut GreenNodeBuilder, token: &GreenToken) {
    builder.token(token.kind(), token.text());
}

fn offset_error_span(error: &mut WqError, offset: usize) {
    if let Some((start, end)) = &mut error.span {
        *start += offset;
        *end += offset;
    }
}

fn attach_full_source_context(error: &mut WqError, input: &str) {
    let path = error
        .source_ctx
        .as_deref()
        .map_or_else(|| "?".to_string(), |context| context.path.clone());
    error.source_ctx = Some(Box::new(SourceCtx {
        text: input.to_string(),
        path,
    }));
}

fn attach_full_source_context_to_ast_errors(node: &mut AstNode, input: &str) {
    match node {
        AstNode::Error(error, _) => attach_full_source_context(error, input),
        AstNode::Literal(_, _)
        | AstNode::Import { .. }
        | AstNode::Variable(_, _)
        | AstNode::OuterVariable(_, _)
        | AstNode::UnpackValue { .. }
        | AstNode::PipeInput
        | AstNode::Ellipsis(_)
        | AstNode::Break(_)
        | AstNode::Continue(_) => {}
        AstNode::NamedArg { value, .. }
        | AstNode::Assignment { value, .. }
        | AstNode::OuterAssignment { value, .. }
        | AstNode::Debug { expr: value, .. }
        | AstNode::Try(value, _) => attach_full_source_context_to_ast_errors(value, input),
        AstNode::Pause { expr, .. } | AstNode::Return(expr, _) => {
            if let Some(expr) = expr {
                attach_full_source_context_to_ast_errors(expr, input);
            }
        }
        AstNode::UnpackAssignment { lhs, rhs, .. } => {
            for node in lhs {
                attach_full_source_context_to_ast_errors(node, input);
            }
            attach_full_source_context_to_ast_errors(rhs, input);
        }
        AstNode::DictUnpackPattern(entries, _) => {
            for entry in entries {
                attach_full_source_context_to_ast_errors(&mut entry.target, input);
            }
        }
        AstNode::Postfix { object, items, .. } => {
            attach_full_source_context_to_ast_errors(object, input);
            for item in items {
                attach_full_source_context_to_ast_errors(item, input);
            }
        }
        AstNode::Pipe {
            input: lhs, effect, ..
        }
        | AstNode::PipeTap {
            input: lhs, effect, ..
        } => {
            attach_full_source_context_to_ast_errors(lhs, input);
            attach_full_source_context_to_ast_errors(effect, input);
        }
        AstNode::CallName { args, .. } => {
            for arg in args {
                attach_full_source_context_to_ast_errors(arg, input);
            }
        }
        AstNode::CallAnonymous { object, args, .. } => {
            attach_full_source_context_to_ast_errors(object, input);
            for arg in args {
                attach_full_source_context_to_ast_errors(arg, input);
            }
        }
        AstNode::Index { object, index, .. } | AstNode::MutatingIndex { object, index, .. } => {
            attach_full_source_context_to_ast_errors(object, input);
            attach_full_source_context_to_ast_errors(index, input);
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
            attach_full_source_context_to_ast_errors(object, input);
            attach_full_source_context_to_ast_errors(index, input);
            attach_full_source_context_to_ast_errors(value, input);
        }
        AstNode::Function { body, .. } => attach_full_source_context_to_ast_errors(body, input),
        AstNode::BinaryOp { left, right, .. } => {
            attach_full_source_context_to_ast_errors(left, input);
            attach_full_source_context_to_ast_errors(right, input);
        }
        AstNode::LazyBool { operands, .. } => {
            for operand in operands {
                attach_full_source_context_to_ast_errors(operand, input);
            }
        }
        AstNode::UnaryOp { operand, .. } | AstNode::Group { expr: operand, .. } => {
            attach_full_source_context_to_ast_errors(operand, input);
        }
        AstNode::ComparisonChain { first, rest, .. } => {
            attach_full_source_context_to_ast_errors(first, input);
            for (_, node) in rest {
                attach_full_source_context_to_ast_errors(node, input);
            }
        }
        AstNode::Range {
            start, end, step, ..
        } => {
            attach_full_source_context_to_ast_errors(start, input);
            attach_full_source_context_to_ast_errors(end, input);
            if let Some(step) = step {
                attach_full_source_context_to_ast_errors(step, input);
            }
        }
        AstNode::Conditional {
            condition,
            true_branch,
            false_branch,
            ..
        } => {
            attach_full_source_context_to_ast_errors(condition, input);
            attach_full_source_context_to_ast_errors(true_branch, input);
            if let Some(false_branch) = false_branch {
                attach_full_source_context_to_ast_errors(false_branch, input);
            }
        }
        AstNode::ConditionalDot {
            condition,
            true_branch,
            ..
        } => {
            attach_full_source_context_to_ast_errors(condition, input);
            attach_full_source_context_to_ast_errors(true_branch, input);
        }
        AstNode::ConditionalChain {
            pairs,
            default_branch,
            ..
        } => {
            for (condition, branch) in pairs {
                attach_full_source_context_to_ast_errors(condition, input);
                attach_full_source_context_to_ast_errors(branch, input);
            }
            attach_full_source_context_to_ast_errors(default_branch, input);
        }
        AstNode::WLoop {
            condition, body, ..
        } => {
            attach_full_source_context_to_ast_errors(condition, input);
            attach_full_source_context_to_ast_errors(body, input);
        }
        AstNode::NLoop { count, body, .. } => {
            attach_full_source_context_to_ast_errors(count, input);
            attach_full_source_context_to_ast_errors(body, input);
        }
        AstNode::Cat(items, _)
        | AstNode::List(items, _)
        | AstNode::Block(items, _)
        | AstNode::BlockExpr(items, _) => {
            for item in items {
                attach_full_source_context_to_ast_errors(item, input);
            }
        }
        AstNode::Dict(pairs, _) => {
            for (_, value) in pairs {
                attach_full_source_context_to_ast_errors(value, input);
            }
        }
        AstNode::FString { parts, .. } => {
            for part in parts {
                match part {
                    FStringPart::Text(_) => {}
                    FStringPart::Expr {
                        expr, spec_exprs, ..
                    } => {
                        attach_full_source_context_to_ast_errors(expr, input);
                        for spec_expr in spec_exprs {
                            attach_full_source_context_to_ast_errors(spec_expr, input);
                        }
                    }
                }
            }
        }
    }
}

fn eof_token_for(input: &str) -> Token {
    let line = input.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = input
        .rsplit('\n')
        .next()
        .map_or(1, |line| line.chars().count() + 1);
    Token::new(
        TokenType::Eof,
        input.chars().count(),
        line,
        column,
        input.len(),
        input.len(),
    )
}

fn compute_dirty_byte_range(old: &str, new: &str) -> (usize, usize, usize, usize) {
    let old_bytes = old.as_bytes();
    let new_bytes = new.as_bytes();

    let mut prefix = 0usize;
    while prefix < old_bytes.len()
        && prefix < new_bytes.len()
        && old_bytes[prefix] == new_bytes[prefix]
    {
        prefix += 1;
    }

    let mut old_end = old_bytes.len();
    let mut new_end = new_bytes.len();
    while old_end > prefix && new_end > prefix && old_bytes[old_end - 1] == new_bytes[new_end - 1] {
        old_end -= 1;
        new_end -= 1;
    }

    (prefix, old_end, prefix, new_end)
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use super::*;
    use crate::token::FmtPart;

    fn root_nodes(root: &GreenNode) -> Vec<GreenNode> {
        root.children()
            .iter()
            .filter_map(|child| match child {
                GreenChild::Node(node) => Some(node.clone()),
                GreenChild::Token(_) => None,
            })
            .collect()
    }

    #[test]
    fn state_is_only_shared_builtin_configuration() {
        fn assert_shareable<T: Clone + Send + Sync>() {}

        assert_shareable::<Frontend>();
        assert_eq!(size_of::<Frontend>(), size_of::<Arc<Builtins>>());

        let frontend = Frontend::with_preset(BuiltinPreset::Minimal);
        let cloned = frontend.clone();
        assert!(Arc::ptr_eq(&frontend.builtins, &cloned.builtins));
        assert!(!frontend.builtins().is_enabled_name("print"));

        let index = frontend
            .analyze_symbols("x:1; f:{[y] y+x}")
            .expect("frontend symbol analysis");
        assert!(index.defs.iter().any(|def| def.name == "x"));
    }

    #[test]
    fn cached_parse_reuses_unchanged_prefix_and_suffix_statements() {
        let frontend = Frontend::default();
        let old_source = "a:1\nb:2\nc:3\n";
        let new_source = "a:1\nb:20\nc:3\n";
        let (_, first) = frontend.parse_with_cst(old_source).expect("initial parse");
        let (_, second) = frontend
            .parse_with_cst_using_cache(new_source, &first)
            .expect("cached parse");

        let first_nodes = root_nodes(&first);
        let second_nodes = root_nodes(&second);
        assert_eq!(first_nodes.len(), second_nodes.len());
        assert!(first_nodes[0].ptr_eq(&second_nodes[0]));
        assert!(!first_nodes[1].ptr_eq(&second_nodes[1]));
        assert!(first_nodes[2].ptr_eq(&second_nodes[2]));
        assert_eq!(second.text(), new_source);
    }

    #[test]
    fn cached_parse_handles_dirty_ranges_inside_utf8_scalars() {
        let frontend = Frontend::default();
        let old_source = "a:\"é\"\nb:2\n";
        let new_source = "a:\"ê\"\nb:2\n";
        let (_, first) = frontend.parse_with_cst(old_source).expect("initial parse");
        let (_, second) = frontend
            .parse_with_cst_using_cache(new_source, &first)
            .expect("cached UTF-8 parse");

        let first_nodes = root_nodes(&first);
        let second_nodes = root_nodes(&second);
        assert!(!first_nodes[0].ptr_eq(&second_nodes[0]));
        assert!(first_nodes[1].ptr_eq(&second_nodes[1]));
        assert_eq!(second.text(), new_source);
    }

    #[test]
    fn script_services_preserve_offsets_and_skip_metadata_tokens() {
        let frontend = Frontend::default();
        let source = "#!/usr/bin/env wq\na:1\n\\l ./lib.wq\nb:2\n";

        let index = frontend
            .analyze_symbols(source)
            .expect("script symbol analysis");
        assert!(index.errors.is_empty());
        let b = index
            .defs
            .iter()
            .find(|def| def.name == "b")
            .expect("b definition");
        assert_eq!(
            b.name_span,
            source.rfind('b').map(|offset| (offset, offset + 1))
        );

        let tokens = frontend.tokenize_recovery(source);
        assert!(
            tokens
                .iter()
                .all(|token| !matches!(token.token_type, TokenType::Error))
        );
        assert!(tokens.iter().all(|token| {
            !matches!(&token.token_type, TokenType::Identifier(name) if name == "l")
        }));

        let (_, cst) = frontend.parse_with_cst(source).expect("script CST");
        assert_eq!(cst.text(), source);
        assert!(
            SyntaxNode::new_root(cst)
                .children()
                .any(|node| node.kind() == SyntaxKind::ScriptDirective)
        );
    }

    #[test]
    fn script_recovery_tokens_use_absolute_source_coordinates() {
        let frontend = Frontend::default();
        let source = concat!(
            "#!/usr/bin/env wq\n",
            "head:\"é\"\n",
            "\\l first.wq\n",
            "\\p\n",
            "last:@f \"🦀 {head}\"\n",
        );

        let tokens = frontend.tokenize_recovery(source);
        for token in &tokens {
            let prefix = &source[..token.byte_start];
            let expected_position =
                prefix.chars().count() + usize::from(!matches!(token.token_type, TokenType::Eof));
            let expected_line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
            let expected_column = prefix
                .rsplit('\n')
                .next()
                .expect("split always yields one segment")
                .chars()
                .count()
                + 1;
            assert_eq!(
                (token.position, token.line, token.column),
                (expected_position, expected_line, expected_column),
                "wrong absolute coordinates for {:?}",
                token.token_type,
            );
        }

        let last_start = source.find("last").expect("last binding");
        let last = tokens
            .iter()
            .find(
                |token| matches!(&token.token_type, TokenType::Identifier(name) if name == "last"),
            )
            .expect("last identifier token");
        assert_eq!(
            (last.byte_start, last.byte_end),
            (last_start, last_start + 4)
        );

        let format = tokens
            .iter()
            .find(|token| matches!(token.token_type, TokenType::FormatString(..)))
            .expect("format string token");
        assert_eq!(
            &source[format.byte_start..format.byte_end],
            "@f \"🦀 {head}\""
        );
        let TokenType::FormatString(parts, open_quote, close_quote) = &format.token_type else {
            unreachable!("format string token was selected")
        };
        assert_eq!(*open_quote, source.find("\"🦀").expect("opening quote"));
        assert_eq!(*close_quote, source.rfind('"').expect("closing quote"));
        for part in parts {
            match part {
                FmtPart::Text {
                    content,
                    start,
                    end,
                } => assert_eq!(&source[*start..*end], content),
                FmtPart::Expr {
                    source: expression,
                    start,
                    end,
                } => assert_eq!(&source[*start..*end], expression),
            }
        }
        let expr = parts
            .iter()
            .find(|part| matches!(part, FmtPart::Expr { .. }))
            .expect("format expression part");
        let FmtPart::Expr { start, end, .. } = expr else {
            unreachable!("format expression part was selected")
        };
        assert_eq!(&source[*start..*end], "{head}");

        let eof = tokens.last().expect("synthetic EOF token");
        assert!(matches!(eof.token_type, TokenType::Eof));
        assert_eq!(eof.position, source.chars().count());
    }

    #[test]
    fn script_ast_errors_keep_full_unicode_source_context() {
        let frontend = Frontend::default();
        let source = concat!(
            "#!/usr/bin/env wq\n",
            "\\l library.wq\n",
            "label:\"界\"\n",
            "broken:)\n",
            "after:2\n",
        );
        let error_start = source.find(')').expect("malformed token");
        let expected_span = (error_start, error_start + 1);

        let index = frontend
            .analyze_symbols(source)
            .expect("recoverable script symbol analysis");
        let error = index
            .errors
            .iter()
            .find_map(|(span, error)| (*span == expected_span).then_some(error))
            .expect("absolute recovered syntax diagnostic");
        assert_full_source_context(error, source, expected_span);

        let (ast, _) = frontend
            .parse_with_cst(source)
            .expect("recoverable script CST");
        let ast_index = SymbolIndex::analyze(&ast, frontend.builtins());
        let ast_error = ast_index
            .errors
            .iter()
            .find_map(|(span, error)| (*span == expected_span).then_some(error))
            .expect("AST error node with absolute diagnostic span");
        assert_full_source_context(ast_error, source, expected_span);
    }

    #[test]
    fn script_eof_errors_keep_full_multiline_source_context() {
        let frontend = Frontend::default();
        let source = concat!("#!/usr/bin/env wq\n", "\\p\n", "f:{[x]\n", "  \"界\",x+1\n",);
        let expected_span = (source.len(), source.len());

        let index = frontend
            .analyze_symbols(source)
            .expect("incomplete script symbol analysis");
        let error = index
            .errors
            .iter()
            .find_map(|(span, error)| {
                (*span == expected_span && error.err_type == WqErrorType::Eof).then_some(error)
            })
            .expect("absolute synthetic EOF diagnostic");
        assert_full_source_context(error, source, expected_span);
    }

    fn assert_full_source_context(error: &WqError, source: &str, expected_span: (usize, usize)) {
        assert_eq!(error.span, Some(expected_span));
        assert_eq!(
            error
                .source_ctx
                .as_deref()
                .map(|context| context.text.as_str()),
            Some(source)
        );
    }

    #[test]
    fn script_completeness_uses_the_trailing_code_region() {
        let frontend = Frontend::default();
        let metadata = "#!/usr/bin/env wq\n\\l first.wq\n\\p\n";

        assert!(!frontend.is_complete_input(&format!("{metadata}f:{{[x]\n  x+1\n")));
        assert!(frontend.is_complete_input(&format!("{metadata}f:{{[x]\n  x+1\n}}\n")));
        assert!(frontend.is_complete_input(&format!("{metadata})\n")));
        assert!(frontend.is_complete_input(&format!("{metadata}\"ab\"\n")));
        assert!(frontend.is_complete_input(metadata));
    }

    #[test]
    fn unknown_script_directives_are_structured_syntax_errors() {
        let frontend = Frontend::default();
        let source = "#!/usr/bin/env wq\na:1\n\\bogus option\nb:2\n";
        let directive_start = source.find("\\bogus").expect("unknown directive");
        let directive_end = source[directive_start..]
            .find('\n')
            .map(|end| directive_start + end + 1)
            .expect("directive newline");
        let expected_span = (directive_start, directive_end);

        let index = frontend
            .analyze_symbols(source)
            .expect("symbol analysis should retain recoverable code");
        assert!(index.defs.iter().any(|definition| definition.name == "a"));
        assert!(index.defs.iter().any(|definition| definition.name == "b"));
        let [(span, diagnostic)] = index.errors.as_slice() else {
            panic!("expected one directive diagnostic: {:#?}", index.errors)
        };
        assert_eq!(*span, expected_span);
        assert_unknown_directive_error(diagnostic, source, expected_span);

        let cst_error = frontend
            .parse_with_cst(source)
            .expect_err("unknown directive should reject the CST result");
        assert_unknown_directive_error(&cst_error, source, expected_span);

        for kind in [SyntaxDisplayKind::Ast, SyntaxDisplayKind::Cst] {
            let display_error = frontend
                .format_syntax_display(source, kind, ColorMode::Never)
                .expect_err("unknown directive should reject syntax display");
            assert_unknown_directive_error(&display_error, source, expected_span);
        }

        assert!(frontend.is_complete_input("\\bogus\nf:{[x]\n"));
    }

    fn assert_unknown_directive_error(
        error: &WqError,
        source: &str,
        expected_span: (usize, usize),
    ) {
        assert_eq!(error.err_type, WqErrorType::Syntax);
        assert_eq!(error.span, Some(expected_span));
        assert!(
            error
                .msg
                .as_deref()
                .is_some_and(|message| message.contains("unknown or invalid script directive"))
        );
        assert!(
            error
                .msg
                .as_deref()
                .is_some_and(|message| message.contains("\\bogus"))
        );
        assert_eq!(
            error
                .source_ctx
                .as_deref()
                .map(|context| context.text.as_str()),
            Some(source)
        );
    }

    #[test]
    fn syntax_display_and_completeness_do_not_evaluate() {
        let frontend = Frontend::default();
        let display = frontend
            .format_syntax_display(
                "raise \"must not run\"",
                SyntaxDisplayKind::Ast,
                ColorMode::Never,
            )
            .expect("AST display");
        assert!(display.starts_with("AST @ fold - final\n"));
        assert!(display.contains("raise"));

        assert!(!frontend.is_complete_input("f:{[x]"));
        assert!(frontend.is_complete_input(")"));
    }
}
