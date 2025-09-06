use crate::astnode::{AstNode, BinaryOperator, UnaryOperator};
use crate::lexer::{Token, TokenType};
use crate::value::{Value, WqResult};
use crate::wqerror::WqError;

#[cfg(not(target_arch = "wasm32"))]
use colored::Colorize;

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
    source: String,
    builtins: crate::builtins::Builtins,
    // Byte spans for statements parsed at the current (top-level) scope
    stmt_spans: Vec<(usize, usize)>,
    // Function body statement spans, in encounter order
    fn_spans: Vec<Vec<(usize, usize)>>,
    // Stack of span collectors for the current function context; when parsing a function,
    // we push a new collector and record every statement (including nested branch bodies)
    // into the top of this stack. On function end, we pop and store into fn_spans.
    fn_span_stack: Vec<Vec<(usize, usize)>>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>, source: String) -> Self {
        Parser {
            tokens,
            current: 0,
            source,
            builtins: crate::builtins::Builtins::new(),
            stmt_spans: Vec::new(),
            fn_spans: Vec::new(),
            fn_span_stack: Vec::new(),
        }
    }

    // helpers
    // =======

    fn current_token(&self) -> Option<&Token> {
        self.tokens.get(self.current)
    }

    fn peek_token(&self) -> Option<&Token> {
        self.tokens.get(self.current + 1)
    }

    fn advance(&mut self) -> Option<&Token> {
        if self.current < self.tokens.len() {
            let tok = &self.tokens[self.current];
            self.current += 1;
            Some(tok)
        } else {
            None
        }
    }

    fn syntax_error(&self, token: &Token, msg: &str) -> WqError {
        let line = token.line;
        let column = token.column;
        let src_line = self
            .source
            .lines()
            .nth(line.saturating_sub(1))
            .unwrap_or("");
        let width = if token.byte_end > token.byte_start
            && token.byte_end <= self.source.len()
            && token.byte_start <= self.source.len()
        {
            self.source[token.byte_start..token.byte_end - 1]
                .chars()
                .count()
                .max(1)
        } else {
            1
        };
        let pointer = " ".repeat(column.saturating_sub(1)) + &"^".repeat(width);

        #[cfg(not(target_arch = "wasm32"))]
        {
            WqError::SyntaxError(format!(
                "{msg} \n{}\n{src_line}\n{pointer}",
                format!("At {line}:{column}").underline()
            ))
        }

        #[cfg(target_arch = "wasm32")]
        {
            WqError::SyntaxError(format!("{msg} \nAt {line}:{column}\n{src_line}\n{pointer}",))
        }
    }

    fn eof_error(&self, msg: &str) -> WqError {
        WqError::EofError(msg.to_string())
    }

    fn consume(&mut self, expected: TokenType) -> WqResult<()> {
        if let Some(tok) = self.current_token() {
            if std::mem::discriminant(&tok.token_type) == std::mem::discriminant(&expected) {
                self.advance();
                Ok(())
            } else {
                Err(self.syntax_error(
                    tok,
                    &format!("expected {:?}, found {:?}", expected, tok.token_type),
                ))
            }
        } else {
            Err(self.eof_error("unexpected end of input"))
        }
    }

    #[inline]
    fn is_token(&self, tt: &TokenType) -> bool {
        self.current_token()
            .map(|t| std::mem::discriminant(&t.token_type) == std::mem::discriminant(tt))
            .unwrap_or(false)
    }

    /// Skip trivia tokens.
    /// - allow_nl: if true, skip newline tokens as trivia
    /// - allow_com: if true, skip comments as trivia
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

    /// Require a literal semicolon. Comments/newlines may appear around it,
    /// but only `;` satisfies the requirement.
    #[inline]
    fn require_semicolon(&mut self, ctx: &str) -> WqResult<()> {
        self.eat_trivia(true, true);
        match self.current_token().map(|t| &t.token_type) {
            Some(TokenType::Semicolon) => {
                self.advance();
                Ok(())
            }
            Some(TokenType::Eof) => {
                Err(self.eof_error(&format!("Unexpected end of input in {ctx}")))
            }
            Some(tt) => Err(self.syntax_error(
                self.current_token().unwrap(),
                &format!("expected ';' in {ctx}, found {tt:?}"),
            )),
            None => Err(self.eof_error(&format!("Unexpected end of input in {ctx}"))),
        }
    }

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
        while matches!(
            self.current_token().map(|t| &t.token_type),
            Some(TokenType::Comment(_))
        ) {
            self.advance();
        }

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
                Err(self.eof_error(&format!("unexpected end of input in {ctx}")))
            }
            Some(tt) => Err(self.syntax_error(
                self.current_token().unwrap(),
                &format!("expected ';' or newline in {ctx}, found {tt:?}"),
            )),
            None => Err(self.eof_error(&format!("unexpected end of input in {ctx}"))),
        }
    }

    #[inline]
    fn err_missing_rhs(&self, op_tok: &Token, ctx: &str) -> WqError {
        self.syntax_error(op_tok, &format!("expected expression after {ctx}"))
    }

    #[inline]
    fn ensure_rhs_after_op(&self, op_tok: &Token, ctx: &str) -> WqResult<()> {
        match self.current_token().map(|t| &t.token_type) {
            Some(TokenType::Eof) | None => Err(self.err_missing_rhs(op_tok, ctx)),
            _ => Ok(()),
        }
    }

    // program
    // =======

    pub fn parse(&mut self) -> WqResult<AstNode> {
        let mut statements = Vec::new();

        loop {
            self.eat_stmt_separators();

            match self.current_token().map(|t| &t.token_type) {
                Some(TokenType::Eof) | None => break,
                _ => {
                    let start_idx = self.current;
                    let stmt = self.parse_statement()?;
                    let end_idx = self.current.saturating_sub(1);
                    if start_idx < self.tokens.len() && end_idx < self.tokens.len() {
                        let start = self.tokens[start_idx].byte_start;
                        let end = self.tokens[end_idx].byte_end;
                        self.stmt_spans.push((start, end));
                    }
                    statements.push(stmt);
                }
            }

            self.eat_stmt_separators();
        }

        Ok(if statements.len() == 1 {
            statements.remove(0)
        } else {
            AstNode::Block(statements)
        })
    }

    pub fn stmt_spans_top(&self) -> &[(usize, usize)] {
        &self.stmt_spans
    }

    pub fn fn_body_spans_all(&self) -> &Vec<Vec<(usize, usize)>> {
        &self.fn_spans
    }

    #[inline]
    fn record_stmt_span_idx(&mut self, start_idx: usize, end_idx: usize) {
        if start_idx < self.tokens.len() && end_idx < self.tokens.len() {
            let start = self.tokens[start_idx].byte_start;
            let end = self.tokens[end_idx].byte_end;
            if let Some(cur) = self.fn_span_stack.last_mut() {
                cur.push((start, end));
            }
        }
    }

    // fn parse_block(&mut self) -> WqResult<AstNode> {
    //     let mut statements = Vec::new();

    //     loop {
    //         self.eat_stmt_separators();

    //         match self.current_token().map(|t| &t.token_type) {
    //             Some(TokenType::RightBrace) => break,
    //             Some(TokenType::Eof) | None => {
    //                 return Err(self.eof_error("unexpected end of input in block"));
    //             }
    //             _ => {
    //                 let stmt = self.parse_statement()?;
    //                 statements.push(stmt);
    //             }
    //         }

    //         self.eat_stmt_separators();
    //     }

    //     Ok(if statements.len() == 1 {
    //         statements.remove(0)
    //     } else {
    //         AstNode::Block(statements)
    //     })
    // }

    // Parse a block and also record per-statement spans (start,end) for debug mapping.
    fn parse_block_with_spans(&mut self) -> WqResult<(AstNode, Vec<(usize, usize)>)> {
        let mut statements = Vec::new();
        let mut spans: Vec<(usize, usize)> = Vec::new();

        loop {
            self.eat_stmt_separators();

            match self.current_token().map(|t| &t.token_type) {
                Some(TokenType::RightBrace) => break,
                Some(TokenType::Eof) | None => {
                    return Err(self.eof_error("unexpected end of input in block"));
                }
                _ => {
                    let start_idx = self.current;
                    let stmt = self.parse_statement()?;
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
            }

            self.eat_stmt_separators();
        }

        // Do not push into fn_spans here; function-level span collection is finalized
        // in parse_function to ensure nested branch statements are included.

        let block = if statements.len() == 1 {
            statements.remove(0)
        } else {
            AstNode::Block(statements)
        };
        Ok((block, spans))
    }

    // expr
    // ====

    fn parse_statement(&mut self) -> WqResult<AstNode> {
        self.parse_expression()
    }

    fn parse_expression(&mut self) -> WqResult<AstNode> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> WqResult<AstNode> {
        let mut expr = self.parse_pipe()?;

        while let Some(token) = self.current_token() {
            if token.token_type == TokenType::Colon {
                match expr {
                    AstNode::Variable(name) => {
                        if self.builtins.has_function(&name) {
                            return Err(self.syntax_error(
                                token,
                                &format!("cannot assign to builtin `{name}`"),
                            ));
                        }
                        let colon_tok = token.clone();
                        self.advance();
                        self.ensure_rhs_after_op(&colon_tok, "':'")?;
                        let value = self.parse_assignment()?;
                        expr = AstNode::Assignment {
                            name,
                            value: Box::new(value),
                        };
                    }
                    AstNode::Index { object, index } => {
                        let colon_tok = token.clone();
                        self.advance();
                        self.ensure_rhs_after_op(&colon_tok, "':'")?;
                        let value = self.parse_assignment()?;
                        expr = AstNode::IndexAssign {
                            object,
                            index,
                            value: Box::new(value),
                        };
                    }
                    AstNode::Postfix {
                        object,
                        items,
                        explicit_call: false,
                    } => {
                        let colon_tok = token.clone();
                        self.advance();
                        self.ensure_rhs_after_op(&colon_tok, "':'")?;
                        let value = self.parse_assignment()?;
                        let index = if items.len() == 1 {
                            Box::new(items.into_iter().next().unwrap())
                        } else {
                            Box::new(AstNode::List(items))
                        };
                        expr = AstNode::IndexAssign {
                            object,
                            index,
                            value: Box::new(value),
                        };
                    }
                    _ => break,
                }
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_pipe(&mut self) -> WqResult<AstNode> {
        let mut left = self.parse_comma()?;

        while let Some(token) = self.current_token().cloned() {
            if token.token_type == TokenType::Pipe {
                self.advance();
                self.ensure_rhs_after_op(&token, "'|' operator")?;
                let right = self.parse_postfix()?;
                left = match right {
                    // AstNode::Variable(name) => AstNode::Call {
                    //     name,
                    //     args: vec![left],
                    // },
                    AstNode::Variable(name) => AstNode::Postfix {
                        object: Box::new(AstNode::Variable(name)),
                        items: vec![left],
                        explicit_call: false,
                    },
                    AstNode::Function { params, body } => AstNode::CallAnonymous {
                        object: Box::new(AstNode::Function { params, body }),
                        args: vec![left],
                    },
                    AstNode::Call { name, mut args } => {
                        args.push(left);
                        AstNode::Call { name, args }
                    }
                    AstNode::CallAnonymous { object, mut args } => {
                        args.push(left);
                        AstNode::CallAnonymous { object, args }
                    }
                    AstNode::Postfix {
                        object, mut items, ..
                    } => {
                        items.push(left);
                        AstNode::Postfix {
                            object,
                            items,
                            explicit_call: false,
                        }
                    }
                    _ => {
                        return Err(self.syntax_error(&token, "invalid right-hand side for '|'"));
                    }
                };
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_comma(&mut self) -> WqResult<AstNode> {
        let mut items = Vec::new();

        if let Some(token) = self.current_token()
            && token.token_type == TokenType::Comma
        {
            // Leading comma list: ,a,b,c
            while let Some(t) = self.current_token() {
                if t.token_type != TokenType::Comma {
                    break;
                }
                self.advance();
                let expr = self.parse_comparison()?;
                items.push(expr);
            }
            return Ok(AstNode::List(items));
        }

        let mut expr = self.parse_comparison()?;

        while let Some(t) = self.current_token() {
            if t.token_type != TokenType::Comma {
                break;
            }
            self.advance(); // eat ','
            let right = self.parse_comparison()?;
            // eprintln!("cat left={expr:?} right={right:?}");
            expr = match (expr, right) {
                // cat
                (AstNode::List(mut a), AstNode::List(mut b)) => {
                    a.append(&mut b);
                    AstNode::List(a)
                }
                (x, y) => AstNode::Call {
                    name: "cat".into(),
                    args: vec![x, y],
                },
            };
        }

        Ok(expr)
    }

    fn parse_comparison(&mut self) -> WqResult<AstNode> {
        let mut left = self.parse_additive()?;

        while let Some(token) = self.current_token().cloned() {
            let (op, op_tok) = match token.token_type {
                TokenType::Equal => (BinaryOperator::Equal, token),
                TokenType::NotEqual => (BinaryOperator::NotEqual, token),
                TokenType::LessThan => (BinaryOperator::LessThan, token),
                TokenType::LessThanOrEqual => (BinaryOperator::LessThanOrEqual, token),
                TokenType::GreaterThan => (BinaryOperator::GreaterThan, token),
                TokenType::GreaterThanOrEqual => (BinaryOperator::GreaterThanOrEqual, token),
                _ => break,
            };
            self.advance();
            self.ensure_rhs_after_op(&op_tok, "comparison operator")?;
            let right = self.parse_additive()?;
            left = AstNode::BinaryOp {
                left: Box::new(left),
                operator: op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_additive(&mut self) -> WqResult<AstNode> {
        let mut left = self.parse_multiplicative()?;

        while let Some(token) = self.current_token().cloned() {
            let (op, op_tok) = match token.token_type {
                TokenType::Plus => (BinaryOperator::Add, token),
                TokenType::Minus => (BinaryOperator::Subtract, token),
                _ => break,
            };
            self.advance();
            self.ensure_rhs_after_op(&op_tok, "binary operator")?;
            let right = self.parse_multiplicative()?;
            left = AstNode::BinaryOp {
                left: Box::new(left),
                operator: op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> WqResult<AstNode> {
        let mut left = self.parse_unary()?;

        while let Some(token) = self.current_token().cloned() {
            let (op, op_tok) = match token.token_type {
                TokenType::Multiply => (BinaryOperator::Multiply, token),
                TokenType::Divide => (BinaryOperator::Divide, token),
                TokenType::DivideDot => (BinaryOperator::DivideDot, token),
                TokenType::Modulo => (BinaryOperator::Modulo, token),
                TokenType::ModuloDot => (BinaryOperator::ModuloDot, token),
                _ => break,
            };
            self.advance();
            self.ensure_rhs_after_op(&op_tok, "binary operator")?;
            let right = self.parse_unary()?;
            left = AstNode::BinaryOp {
                left: Box::new(left),
                operator: op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_unary(&mut self) -> WqResult<AstNode> {
        let mut ops: Vec<UnaryOperator> = Vec::new();
        let mut negate_parity = 0u8;

        while let Some(token) = self.current_token().cloned() {
            match token.token_type {
                TokenType::Minus => {
                    self.advance();
                    negate_parity ^= 1;
                }
                TokenType::Sharp => {
                    if negate_parity == 1 {
                        ops.push(UnaryOperator::Negate);
                        negate_parity = 0;
                    }
                    self.advance();
                    ops.push(UnaryOperator::Count);
                }
                _ => break,
            }
        }

        let mut node = self.parse_power()?;

        if negate_parity == 1 {
            node = match node {
                AstNode::Literal(Value::Int(n)) => AstNode::Literal(Value::Int(-n)),
                AstNode::Literal(Value::Float(f)) => AstNode::Literal(Value::Float(-f)),
                _ => AstNode::UnaryOp {
                    operator: UnaryOperator::Negate,
                    operand: Box::new(node),
                },
            };
        }

        while let Some(op) = ops.pop() {
            node = AstNode::UnaryOp {
                operator: op,
                operand: Box::new(node),
            };
        }

        Ok(node)
    }

    fn parse_power(&mut self) -> WqResult<AstNode> {
        let mut operands = Vec::new();
        operands.push(self.parse_postfix()?);

        // slurp all "^ unary" pairs first
        while let Some(tok) = self.current_token() {
            if tok.token_type == TokenType::Power {
                let caret_tok = tok.clone();
                self.advance();
                self.ensure_rhs_after_op(&caret_tok, "power operator")?;
                operands.push(self.parse_postfix()?);
            } else {
                break;
            }
        }

        // fold right: a ^ b ^ c => a ^ (b ^ c)
        let mut it = operands.into_iter().rev();
        let mut acc = it.next().unwrap();
        for left in it {
            acc = AstNode::BinaryOp {
                left: Box::new(left),
                operator: BinaryOperator::Power,
                right: Box::new(acc),
            };
        }
        Ok(acc)
    }

    // postfix
    // =======

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
            return Err(self.eof_error("unexpected end of input in bracket"));
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
            self.eat_trivia(true, true);
            if self.is_token(&TokenType::RightBracket) {
                self.advance();
                break;
            }

            self.require_semicolon("bracket items")?;

            // allow trailing '; ]'
            self.eat_trivia(true, true);
            if self.is_token(&TokenType::RightBracket) {
                trailing = true;
                self.advance();
                break;
            }
            if self.is_token(&TokenType::Eof) {
                return Err(self.eof_error("unexpected end of input in bracket"));
            }
        }

        Ok((items, trailing))
    }

    fn parse_postfix(&mut self) -> WqResult<AstNode> {
        let mut expr = self.parse_primary()?;

        loop {
            // ignore comments but do not skip newlines.
            while matches!(
                self.current_token().map(|t| &t.token_type),
                Some(TokenType::Comment(_))
            ) {
                self.advance();
            }

            match self.current_token().map(|t| &t.token_type) {
                Some(TokenType::LeftBracket) => {
                    self.advance();
                    let (items, call_flag) = self.parse_bracket_items()?;
                    expr = AstNode::Postfix {
                        object: Box::new(expr),
                        items,
                        explicit_call: call_flag,
                    };
                }
                // call candidates (newline not allowed)
                Some(TokenType::Integer(_))
                | Some(TokenType::Symbol(_))
                | Some(TokenType::Identifier(_))
                // NO MINUS
                // | Some(TokenType::Minus)
                // EXPERIMENTAL!!
                | Some(TokenType::Sharp)
                | Some(TokenType::LeftParen) => {
                    let arg = self.parse_unary()?;
                    expr = AstNode::Postfix {
                        object: Box::new(expr),
                        items: vec![arg],
                        explicit_call: false,
                    };
                }
                // definitely fn calls
                //Some(TokenType::Integer(_))
                Some(TokenType::Float(_))
                | Some(TokenType::Character(_))
                | Some(TokenType::String(_))
                | Some(TokenType::Inf)
                | Some(TokenType::Nan)
                | Some(TokenType::LeftBrace)
                //| Some(TokenType::Symbol(_))
                //| Some(TokenType::Identifier(_))
                | Some(TokenType::True)
                | Some(TokenType::False) => {
                //| Some(TokenType::LeftParen) => {
                    let arg = self.parse_unary()?;
                    expr = AstNode::Postfix {
                        object: Box::new(expr),
                        items: vec![arg],
                        explicit_call: true,
                    };
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    // list/dict
    // =========

    fn parse_paren_list(&mut self) -> WqResult<AstNode> {
        let mut elements = Vec::new();

        loop {
            self.eat_trivia(true, true);
            if self.is_token(&TokenType::RightParen) {
                self.advance();
                break;
            }
            if self.is_token(&TokenType::Eof) {
                return Err(self.eof_error("unexpected end of input in list"));
            }

            let expr = self.parse_expression()?;
            elements.push(expr);

            self.eat_trivia(true, true);
            if self.is_token(&TokenType::RightParen) {
                self.advance();
                break;
            }

            self.require_semicolon("list")?;
            self.eat_trivia(true, true);
            if self.is_token(&TokenType::RightParen) {
                self.advance();
                break;
            }
            if self.is_token(&TokenType::Eof) {
                return Err(self.eof_error("unexpected end of input in list"));
            }
        }

        Ok(if elements.len() == 1 {
            elements.remove(0)
        } else {
            AstNode::List(elements)
        })
    }

    fn parse_paren_dict(&mut self) -> WqResult<AstNode> {
        let mut pairs = Vec::new();

        loop {
            self.eat_trivia(true, true);
            if self.is_token(&TokenType::RightParen) {
                self.advance();
                break;
            }
            if self.is_token(&TokenType::Eof) {
                return Err(self.eof_error("unexpected end of input in dict"));
            }

            let key_tok = self
                .current_token()
                .ok_or_else(|| self.eof_error("unexpected end of input in dict"))?;
            let key = match &key_tok.token_type {
                TokenType::Symbol(s) => {
                    let s = s.clone();
                    self.advance();
                    s
                }
                TokenType::Eof => return Err(self.eof_error("unexpected end of input in dict")),
                _ => return Err(self.syntax_error(key_tok, "expected symbol key in dict")),
            };

            self.consume(TokenType::Colon)?;
            let value = self.parse_expression()?;
            pairs.push((key, value));

            self.eat_trivia(true, true);
            if self.is_token(&TokenType::RightParen) {
                self.advance();
                break;
            }
            if self.is_token(&TokenType::Eof) {
                return Err(self.eof_error("unexpected end of input in dict"));
            }

            self.require_semicolon("dict")?;
            self.eat_trivia(true, true);
            if self.is_token(&TokenType::RightParen) {
                self.advance();
                break;
            }
        }

        Ok(AstNode::Dict(pairs))
    }

    fn parse_primary(&mut self) -> WqResult<AstNode> {
        if let Some(token) = self.current_token() {
            match &token.token_type {
                TokenType::Integer(n) => {
                    let v = *n;
                    self.advance();
                    Ok(AstNode::Literal(Value::Int(v)))
                }
                TokenType::Float(f) => {
                    let v = *f;
                    self.advance();
                    Ok(AstNode::Literal(Value::Float(v)))
                }
                TokenType::Character(c) => {
                    let v = *c;
                    self.advance();
                    Ok(AstNode::Literal(Value::Char(v)))
                }
                TokenType::String(s) => {
                    let v = s.clone();
                    self.advance();
                    let chars = v.chars().map(Value::Char).collect();
                    Ok(AstNode::Literal(Value::List(chars)))
                }
                TokenType::Symbol(s) => {
                    let v = s.clone();
                    self.advance();
                    Ok(AstNode::Literal(Value::Symbol(v)))
                }
                TokenType::True => {
                    self.advance();
                    Ok(AstNode::Literal(Value::Bool(true)))
                }
                TokenType::False => {
                    self.advance();
                    Ok(AstNode::Literal(Value::Bool(false)))
                }
                TokenType::Inf => {
                    self.advance();
                    Ok(AstNode::Literal(Value::Float(f64::INFINITY)))
                }
                TokenType::Nan => {
                    self.advance();
                    Ok(AstNode::Literal(Value::Float(f64::NAN)))
                }

                TokenType::Dollar => self.parse_conditional(),
                TokenType::DollarDot => self.parse_conditional_dot(),

                TokenType::AtBreak => {
                    self.advance();
                    Ok(AstNode::Break)
                }
                TokenType::AtContinue => {
                    self.advance();
                    Ok(AstNode::Continue)
                }
                TokenType::AtReturn => {
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
                            Ok(AstNode::Return(None))
                        } else {
                            let expr = self.parse_expression()?;
                            Ok(AstNode::Return(Some(Box::new(expr))))
                        }
                    } else {
                        Ok(AstNode::Return(None))
                    }
                }
                TokenType::AtAssert => {
                    self.advance();
                    let e = self.parse_expression()?;
                    Ok(AstNode::Assert(Box::new(e)))
                }
                TokenType::AtTry => {
                    self.advance();
                    let e = self.parse_expression()?;
                    Ok(AstNode::Try(Box::new(e)))
                }

                TokenType::Identifier(name) => {
                    let val = name.clone();
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
                            return self.parse_while();
                        } else if val == "N" {
                            self.advance();
                            return self.parse_for();
                        }
                    }
                    Ok(AstNode::Variable(val))
                }

                TokenType::LeftBrace => self.parse_function(),

                TokenType::LeftParen => {
                    self.advance(); // '('
                    self.eat_trivia(true, true);
                    if self.is_token(&TokenType::RightParen) {
                        self.advance();
                        return Ok(AstNode::List(Vec::new()));
                    }

                    // Decide dict vs list: Symbol ':' lookahead (no deep skipping)
                    let mut is_dict = false;
                    if let Some(Token {
                        token_type: TokenType::Symbol(_),
                        ..
                    }) = self.current_token()
                        && let Some(next) = self.peek_token()
                        && next.token_type == TokenType::Colon
                    {
                        is_dict = true;
                    }

                    if is_dict {
                        self.parse_paren_dict()
                    } else {
                        self.parse_paren_list()
                    }
                }

                TokenType::Eof => Err(self.eof_error("unexpected end of input")),
                _ => {
                    Err(self
                        .syntax_error(token, &format!("unexpected token: {:?}", token.token_type)))
                }
            }
        } else {
            Err(self.eof_error("unexpected end of input"))
        }
    }

    // func
    // ====

    fn parse_function(&mut self) -> WqResult<AstNode> {
        self.advance(); // '{'

        let mut params = None;

        // Optional parameter list: {[a;b]}
        if let Some(tok) = self.current_token()
            && tok.token_type == TokenType::LeftBracket
        {
            self.advance(); // '['
            let mut names = Vec::new();

            loop {
                // allow trivia inside params
                self.eat_trivia(true, true);
                match self.current_token().map(|t| (&t.token_type, t)) {
                    Some((TokenType::Identifier(name), tok)) => {
                        if self.builtins.has_function(name) {
                            return Err(self.syntax_error(
                                    tok,
                                    &format!("cannot use `{name}` as a param because a builtin with the same name exists"),
                                ));
                        }
                        names.push(name.clone());
                        self.advance();
                    }
                    Some((TokenType::Semicolon, _)) => {
                        self.advance();
                    }
                    Some((TokenType::RightBracket, _)) => {
                        self.advance();
                        break;
                    }
                    Some((TokenType::Eof, _)) => {
                        return Err(self.eof_error("unexpected end of input in parameter list"));
                    }
                    Some((_, bad)) => {
                        return Err(self.syntax_error(bad, "expected identifier `;` or `]`"));
                    }
                    None => {
                        return Err(self.eof_error("unexpected end of input in parameter list"));
                    }
                }
            }

            params = Some(names);
        }

        // Start collecting spans for this function (including nested branches)
        self.fn_span_stack.push(Vec::new());

        let (body, spans) = self.parse_block_with_spans()?;
        let _ = spans; // kept for potential local usage; full collection stored below
        self.consume(TokenType::RightBrace)?;

        // Finalize collection for this function body
        if let Some(collected) = self.fn_span_stack.pop() {
            self.fn_spans.push(collected);
        }

        Ok(AstNode::Function {
            params,
            body: Box::new(body),
        })
    }

    // control forms
    // =============

    fn parse_branch_sequence(&mut self, ends: &[TokenType]) -> WqResult<AstNode> {
        let mut stmts = Vec::new();
        let is_end =
            |tt: &TokenType, x: &TokenType| std::mem::discriminant(tt) == std::mem::discriminant(x);

        loop {
            // allow trivia before items
            self.eat_trivia(true, true);

            // stop if at an end token (do not consume); if Eof token, propagate EOF
            match self.current_token() {
                Some(tok) => {
                    if matches!(tok.token_type, TokenType::Eof) {
                        return Err(self.eof_error("unexpected end of input in branch"));
                    }
                    if ends.iter().any(|e| is_end(&tok.token_type, e)) {
                        break;
                    }
                }
                None => return Err(self.eof_error("unexpected end of input in branch")),
            }

            // Record span for this branch statement relative to the current position
            let start_idx = self.current;
            let expr = self.parse_expression()?;
            let end_idx = self.current.saturating_sub(1);
            self.record_stmt_span_idx(start_idx, end_idx);

            stmts.push(expr);

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
                        self.advance();
                        continue;
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
            AstNode::Block(stmts)
        })
    }

    fn parse_conditional(&mut self) -> WqResult<AstNode> {
        self.advance(); // '$'
        self.consume(TokenType::LeftBracket)?;
        // Record a span for the whole `$[cond;...]` construct as a statement boundary
        let header_start_idx = self.current.saturating_sub(2); // '$' then '[' consumed
        // condition
        self.eat_trivia(true, true);
        let condition = self.parse_expression()?;

        // cond -> true boundary: ';' or >=1 newline
        self.require_control_separator("$[condition;true;false]")?;

        // true-branch ends at ';' (boundary) or ']' (if no false for `$` -> error)
        let true_branch =
            self.parse_branch_sequence(&[TokenType::Semicolon, TokenType::RightBracket])?;

        if self.is_token(&TokenType::RightBracket) {
            return Err(self.syntax_error(
                self.current_token().unwrap(),
                "`$`: expected false branch; use '$.[...]' for single-branch conditional",
            ));
        }

        // consume the separating ';'
        self.consume(TokenType::Semicolon)?;

        // false-branch ends at ']'
        let false_branch = self.parse_branch_sequence(&[TokenType::RightBracket])?;
        self.consume(TokenType::RightBracket)?;
        // Record header span covering from '$' to ']'
        let header_end_idx = self.current.saturating_sub(1);
        self.record_stmt_span_idx(header_start_idx, header_end_idx);

        Ok(AstNode::Conditional {
            condition: Box::new(condition),
            true_branch: Box::new(true_branch),
            false_branch: Some(Box::new(false_branch)),
        })
    }

    fn parse_conditional_dot(&mut self) -> WqResult<AstNode> {
        self.advance(); // '$.'
        self.consume(TokenType::LeftBracket)?;
        // Record a span for the whole '$.[cond;true]' construct
        let header_start_idx = self.current.saturating_sub(2); // '$.' and '[' consumed
        self.eat_trivia(true, true);
        let condition = self.parse_expression()?;

        self.require_control_separator("$.[condition;true]")?;

        let true_branch = self.parse_branch_sequence(&[TokenType::RightBracket])?;
        self.consume(TokenType::RightBracket)?;
        let header_end_idx = self.current.saturating_sub(1);
        self.record_stmt_span_idx(header_start_idx, header_end_idx);

        Ok(AstNode::Conditional {
            condition: Box::new(condition),
            true_branch: Box::new(true_branch),
            false_branch: None,
        })
    }

    fn parse_while(&mut self) -> WqResult<AstNode> {
        // called after Identifier("W") and '[' consumed in parse_primary()
        // Record a span for the whole 'W[cond;body]' starting at 'W'
        let header_start_idx = self.current.saturating_sub(2); // 'W' and '[' consumed
        self.eat_trivia(true, true);
        let condition = self.parse_expression()?;

        self.require_control_separator("W[condition;body]")?;

        let body = self.parse_branch_sequence(&[TokenType::RightBracket])?;
        self.consume(TokenType::RightBracket)?;
        let header_end_idx = self.current.saturating_sub(1);
        self.record_stmt_span_idx(header_start_idx, header_end_idx);

        Ok(AstNode::WhileLoop {
            condition: Box::new(condition),
            body: Box::new(body),
        })
    }

    fn parse_for(&mut self) -> WqResult<AstNode> {
        // called after Identifier("N") and '[' consumed
        let header_start_idx = self.current.saturating_sub(2); // 'N' and '[' consumed
        self.eat_trivia(true, true);
        let count_expr = self.parse_expression()?;

        self.require_control_separator("N[count;body]")?;

        let body = self.parse_branch_sequence(&[TokenType::RightBracket])?;
        self.consume(TokenType::RightBracket)?;
        let header_end_idx = self.current.saturating_sub(1);
        self.record_stmt_span_idx(header_start_idx, header_end_idx);

        Ok(AstNode::ForLoop {
            count: Box::new(count_expr),
            body: Box::new(body),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse_string(input: &str) -> WqResult<AstNode> {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize()?;
        use crate::post_parser::resolver::Resolver;
        let mut parser = Parser::new(tokens, input.to_string());
        let ast = parser.parse()?;
        let mut resolver = Resolver::new();
        Ok(resolver.resolve(ast))
    }

    #[test]
    fn test_parse_literal() {
        let ast = parse_string("42").unwrap();
        assert_eq!(ast, AstNode::Literal(Value::Int(42)));
    }

    #[test]
    fn test_parse_binary_op() {
        let ast = parse_string("1+2").unwrap();
        assert_eq!(
            ast,
            AstNode::BinaryOp {
                left: Box::new(AstNode::Literal(Value::Int(1))),
                operator: BinaryOperator::Add,
                right: Box::new(AstNode::Literal(Value::Int(2))),
            }
        );
    }

    #[test]
    fn test_parse_assignment() {
        let ast = parse_string("x:42").unwrap();
        assert_eq!(
            ast,
            AstNode::Assignment {
                name: "x".to_string(),
                value: Box::new(AstNode::Literal(Value::Int(42))),
            }
        );
    }

    #[test]
    fn test_parse_index_assignment() {
        let ast = parse_string("a[1]:3").unwrap();
        assert_eq!(
            ast,
            AstNode::IndexAssign {
                object: Box::new(AstNode::Variable("a".into())),
                index: Box::new(AstNode::Literal(Value::Int(1))),
                value: Box::new(AstNode::Literal(Value::Int(3))),
            }
        );
    }

    #[test]
    fn test_parse_list() {
        let ast = parse_string("(1;2;3)").unwrap();
        assert_eq!(
            ast,
            AstNode::List(vec![
                AstNode::Literal(Value::Int(1)),
                AstNode::Literal(Value::Int(2)),
                AstNode::Literal(Value::Int(3)),
            ])
        );
    }

    #[test]
    fn test_parse_string_literal() {
        let ast = parse_string("\"ab\"").unwrap();
        assert_eq!(
            ast,
            AstNode::Literal(Value::List(vec![Value::Char('a'), Value::Char('b')]))
        );
    }

    #[test]
    fn test_operator_precedence() {
        let ast = parse_string("1+2*3").unwrap();
        assert_eq!(
            ast,
            AstNode::BinaryOp {
                left: Box::new(AstNode::Literal(Value::Int(1))),
                operator: BinaryOperator::Add,
                right: Box::new(AstNode::BinaryOp {
                    left: Box::new(AstNode::Literal(Value::Int(2))),
                    operator: BinaryOperator::Multiply,
                    right: Box::new(AstNode::Literal(Value::Int(3))),
                }),
            }
        );
    }

    #[test]
    fn test_addition_before_comparison() {
        let ast = parse_string("3+2<10").unwrap();
        assert_eq!(
            ast,
            AstNode::BinaryOp {
                left: Box::new(AstNode::BinaryOp {
                    left: Box::new(AstNode::Literal(Value::Int(3))),
                    operator: BinaryOperator::Add,
                    right: Box::new(AstNode::Literal(Value::Int(2))),
                }),
                operator: BinaryOperator::LessThan,
                right: Box::new(AstNode::Literal(Value::Int(10))),
            }
        );
    }

    #[test]
    fn test_function_with_assignment_block() {
        let ast = parse_string("f:{x:1;x+1}").unwrap();
        assert_eq!(
            ast,
            AstNode::Assignment {
                name: "f".into(),
                value: Box::new(AstNode::Function {
                    params: None,
                    body: Box::new(AstNode::Block(vec![
                        AstNode::Assignment {
                            name: "x".into(),
                            value: Box::new(AstNode::Literal(Value::Int(1))),
                        },
                        AstNode::BinaryOp {
                            left: Box::new(AstNode::Variable("x".into())),
                            operator: BinaryOperator::Add,
                            right: Box::new(AstNode::Literal(Value::Int(1))),
                        },
                    ])),
                }),
            }
        );
    }

    #[test]
    fn test_recursive_function_def() {
        let ast = parse_string("a:{a[]}").unwrap();
        assert_eq!(
            ast,
            AstNode::Assignment {
                name: "a".into(),
                value: Box::new(AstNode::Function {
                    params: None,
                    body: Box::new(AstNode::Call {
                        name: "a".into(),
                        args: vec![],
                    }),
                }),
            }
        );
    }

    #[test]
    fn test_parse_while() {
        let ast = parse_string("W[x<3;x:1]").unwrap();
        assert!(matches!(ast, AstNode::WhileLoop { .. }));
    }

    #[test]
    fn test_parse_for() {
        let ast = parse_string("N[3;x:1]").unwrap();
        assert!(matches!(ast, AstNode::ForLoop { .. }));
    }

    #[test]
    fn test_multiline_list() {
        let ast = parse_string("(1;2;\n3;4;\n5)").unwrap();
        assert_eq!(
            ast,
            AstNode::List(vec![
                AstNode::Literal(Value::Int(1)),
                AstNode::Literal(Value::Int(2)),
                AstNode::Literal(Value::Int(3)),
                AstNode::Literal(Value::Int(4)),
                AstNode::Literal(Value::Int(5)),
            ])
        );
    }

    #[test]
    fn test_multiline_dict() {
        let ast = parse_string("(`a:1;\n`b:9;\n)").unwrap();
        assert_eq!(
            ast,
            AstNode::Dict(vec![
                ("a".into(), AstNode::Literal(Value::Int(1))),
                ("b".into(), AstNode::Literal(Value::Int(9))),
            ])
        );
    }

    #[test]
    fn test_multiline_if() {
        let ast = parse_string("$[true;\n1;\n2]").unwrap();
        assert!(matches!(ast, AstNode::Conditional { .. }));
    }

    #[test]
    fn test_multiline_while() {
        let ast = parse_string("W[true;\necho n;\n]").unwrap();
        assert!(matches!(ast, AstNode::WhileLoop { .. }));
    }

    #[test]
    fn test_multiline_for() {
        let ast = parse_string("N[3;\necho n\n]").unwrap();
        assert!(matches!(ast, AstNode::ForLoop { .. }));
    }

    #[test]
    fn test_comments_in_bodies() {
        // comment inside function
        assert!(parse_string("{1; //c\n2}").is_ok());
        // comment inside loop and conditional
        assert!(parse_string("N[2;1;//c\n]").is_ok());
        assert!(parse_string("$[true;1;//c\n2]").is_ok());
    }

    #[test]
    fn test_pipe_simple() {
        let ast = parse_string("a | iota").unwrap();
        assert_eq!(
            ast,
            AstNode::Call {
                name: "iota".into(),
                args: vec![AstNode::Variable("a".into())],
            }
        );
    }

    #[test]
    fn test_pipe_with_arg() {
        let ast = parse_string("a | f[x]").unwrap();
        assert_eq!(
            ast,
            AstNode::Postfix {
                object: Box::new(AstNode::Variable("f".into())),
                items: vec![AstNode::Variable("x".into()), AstNode::Variable("a".into())],
                explicit_call: false
            }
        );
    }

    #[test]
    fn test_pipe_chain() {
        let ast = parse_string("a | f | g[y] | h").unwrap();
        assert_eq!(
            ast,
            AstNode::Postfix {
                object: Box::new(AstNode::Variable("h".into())),
                items: vec![AstNode::Postfix {
                    object: Box::new(AstNode::Variable("g".into())),
                    items: vec![
                        AstNode::Variable("y".into()),
                        AstNode::Postfix {
                            object: Box::new(AstNode::Variable("f".into())),
                            items: vec![AstNode::Variable("a".into())],
                            explicit_call: false
                        }
                    ],
                    explicit_call: false
                }],
                explicit_call: false
            }
        );
    }

    #[test]
    fn test_pipe_sequence() {
        let ast = parse_string("a|iota; b|iota").unwrap();
        assert_eq!(
            ast,
            AstNode::Block(vec![
                AstNode::Call {
                    name: "iota".into(),
                    args: vec![AstNode::Variable("a".into())],
                },
                AstNode::Call {
                    name: "iota".into(),
                    args: vec![AstNode::Variable("b".into())],
                },
            ]),
        );
    }
}
