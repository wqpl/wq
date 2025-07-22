use crate::lexer::{Token, TokenType};
use crate::value::{Value, WqError, WqResult};

/// ast nodes
#[derive(Debug, Clone, PartialEq)]
pub enum AstNode {
    Literal(Value),

    /// Variable reference
    Variable(String),

    BinaryOp {
        left: Box<AstNode>,
        operator: BinaryOperator,
        right: Box<AstNode>,
    },

    UnaryOp {
        operator: UnaryOperator,
        operand: Box<AstNode>,
    },

    Assignment {
        name: String,
        value: Box<AstNode>,
    },

    /// List construction
    List(Vec<AstNode>),

    /// Dictionary construction
    Dict(Vec<(String, AstNode)>),

    /// Function call
    Call {
        name: String,
        args: Vec<AstNode>,
    },

    CallAnonymous {
        object: Box<AstNode>,
        args: Vec<AstNode>,
    },

    /// Index access
    Index {
        object: Box<AstNode>,
        index: Box<AstNode>,
    },

    /// Index assignment like `a[1]:3`
    IndexAssign {
        object: Box<AstNode>,
        index: Box<AstNode>,
        value: Box<AstNode>,
    },

    /// Function def
    Function {
        params: Option<Vec<String>>, // None for implicit params (x, y)
        body: Box<AstNode>,
    },

    /// Conditional expression
    Conditional {
        condition: Box<AstNode>,
        true_branch: Box<AstNode>,
        false_branch: Option<Box<AstNode>>,
    },

    /// While loop
    WhileLoop {
        condition: Box<AstNode>,
        body: Box<AstNode>,
    },

    /// Numeric for loop, exposes counter `n`
    ForLoop {
        count: Box<AstNode>,
        body: Box<AstNode>,
    },

    /// Block/sequence of statements
    Block(Vec<AstNode>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Equal,
    NotEqual,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOperator {
    Negate,
    Count, // '#' operator
}

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
    source: String,
}

impl Parser {
    pub fn new(tokens: Vec<Token>, source: String) -> Self {
        Parser {
            tokens,
            current: 0,
            source,
        }
    }

    fn current_token(&self) -> Option<&Token> {
        self.tokens.get(self.current)
    }

    fn peek_token(&self) -> Option<&Token> {
        self.tokens.get(self.current + 1)
    }

    fn syntax_error(&self, token: &Token, msg: &str) -> WqError {
        let line = token.line;
        let column = token.column;
        let src_line = self
            .source
            .lines()
            .nth(line.saturating_sub(1))
            .unwrap_or("");
        let pointer = " ".repeat(column.saturating_sub(1)) + "^";
        WqError::SyntaxError(format!("{msg} at {line}:{column}\n{src_line}\n{pointer}"))
    }

    fn eof_error(&self, msg: &str) -> WqError {
        WqError::EofError(msg.to_string())
    }

    fn advance(&mut self) -> Option<&Token> {
        if self.current < self.tokens.len() {
            let token = &self.tokens[self.current];
            self.current += 1;
            Some(token)
        } else {
            None
        }
    }

    // fn match_token(&mut self, expected: &TokenType) -> bool {
    //     if let Some(token) = self.current_token() {
    //         if std::mem::discriminant(&token.token_type) == std::mem::discriminant(expected) {
    //             self.advance();
    //             return true;
    //         }
    //     }
    //     false
    // }

    fn consume(&mut self, expected: TokenType) -> WqResult<()> {
        if let Some(token) = self.current_token() {
            if std::mem::discriminant(&token.token_type) == std::mem::discriminant(&expected) {
                self.advance();
                Ok(())
            } else {
                Err(self.syntax_error(
                    token,
                    &format!("Expected {:?}, found {:?}", expected, token.token_type),
                ))
            }
        } else {
            Err(self.eof_error("Unexpected end of input"))
        }
    }

    fn skip_newlines(&mut self) {
        while let Some(Token {
            token_type: TokenType::Newline,
            ..
        }) = self.current_token()
        {
            self.advance();
        }
    }

    pub fn parse(&mut self) -> WqResult<AstNode> {
        let mut statements = Vec::new();

        while let Some(token) = self.current_token() {
            match token.token_type {
                TokenType::Eof => break,
                TokenType::Newline | TokenType::Semicolon => {
                    self.advance();
                    continue;
                }
                TokenType::Comment(_) => {
                    self.advance();
                    continue;
                }
                _ => {
                    let stmt = self.parse_statement()?;
                    statements.push(stmt);
                }
            }
        }

        if statements.len() == 1 {
            Ok(statements.into_iter().next().unwrap())
        } else {
            Ok(AstNode::Block(statements))
        }
    }

    fn parse_statement(&mut self) -> WqResult<AstNode> {
        self.parse_expression()
    }

    fn parse_expression(&mut self) -> WqResult<AstNode> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> WqResult<AstNode> {
        let mut expr = self.parse_comma()?;

        while let Some(token) = self.current_token() {
            if token.token_type == TokenType::Colon {
                match expr {
                    AstNode::Variable(name) => {
                        self.advance();
                        let value = self.parse_assignment()?;
                        expr = AstNode::Assignment {
                            name,
                            value: Box::new(value),
                        };
                    }
                    AstNode::Index { object, index } => {
                        self.advance();
                        let value = self.parse_assignment()?;
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

    fn parse_block(&mut self) -> WqResult<AstNode> {
        let mut statements = Vec::new();

        while let Some(token) = self.current_token() {
            match token.token_type {
                TokenType::RightBrace => break,
                TokenType::Semicolon | TokenType::Newline => {
                    self.advance();
                    continue;
                }
                TokenType::Eof => {
                    return Err(self.eof_error("Unexpected end of input in block"));
                }
                _ => {
                    let stmt = self.parse_statement()?;
                    statements.push(stmt);
                }
            }
        }

        if statements.len() == 1 {
            Ok(statements.into_iter().next().unwrap())
        } else {
            Ok(AstNode::Block(statements))
        }
    }

    fn parse_comma(&mut self) -> WqResult<AstNode> {
        let mut items = Vec::new();

        if let Some(token) = self.current_token() {
            if token.token_type == TokenType::Comma {
                // Leading comma: begin list
                while let Some(token) = self.current_token() {
                    if token.token_type == TokenType::Comma {
                        self.advance(); // consume comma
                        let expr = self.parse_additive()?; // or parse_expression() depending on desired precedence
                        items.push(expr);
                    } else {
                        break;
                    }
                }
                return Ok(AstNode::List(items));
            }
        }

        // No leading comma: parse normally, treating comma as concatenation
        let mut expr = self.parse_additive()?;

        while let Some(token) = self.current_token() {
            if token.token_type == TokenType::Comma {
                self.advance(); // consume comma
                let right = self.parse_additive()?;
                expr = AstNode::Call {
                    name: "cat".to_string(),
                    args: vec![expr, right],
                };
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_additive(&mut self) -> WqResult<AstNode> {
        let mut left = self.parse_comparison()?;

        while let Some(token) = self.current_token() {
            let op = match token.token_type {
                TokenType::Plus => BinaryOperator::Add,
                TokenType::Minus => BinaryOperator::Subtract,
                _ => break,
            };

            self.advance();
            let right = self.parse_comparison()?;
            left = AstNode::BinaryOp {
                left: Box::new(left),
                operator: op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_comparison(&mut self) -> WqResult<AstNode> {
        let mut left = self.parse_multiplicative()?;

        while let Some(token) = self.current_token() {
            let op = match token.token_type {
                TokenType::Equal => BinaryOperator::Equal,
                TokenType::NotEqual => BinaryOperator::NotEqual,
                TokenType::LessThan => BinaryOperator::LessThan,
                TokenType::LessThanOrEqual => BinaryOperator::LessThanOrEqual,
                TokenType::GreaterThan => BinaryOperator::GreaterThan,
                TokenType::GreaterThanOrEqual => BinaryOperator::GreaterThanOrEqual,
                _ => break,
            };

            self.advance();
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

        while let Some(token) = self.current_token() {
            let op = match token.token_type {
                TokenType::Multiply => BinaryOperator::Multiply,
                TokenType::Divide => BinaryOperator::Divide,
                TokenType::Modulo => BinaryOperator::Modulo,
                _ => break,
            };

            self.advance();
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
        if let Some(token) = self.current_token() {
            match token.token_type {
                TokenType::Minus => {
                    if let Some(next_token) = self.peek_token() {
                        match next_token.token_type {
                            TokenType::Integer(n) => {
                                self.advance(); // consume minus
                                self.advance(); // consume number
                                return Ok(AstNode::Literal(Value::Int(-n)));
                            }
                            TokenType::Float(f) => {
                                self.advance(); // consume minus
                                self.advance(); // consume number
                                return Ok(AstNode::Literal(Value::Float(-f)));
                            }
                            _ => {
                                // It's a unary minus operation
                                self.advance();
                                let operand = self.parse_unary()?;
                                return Ok(AstNode::UnaryOp {
                                    operator: UnaryOperator::Negate,
                                    operand: Box::new(operand),
                                });
                            }
                        }
                    } else {
                        // It's a unary minus operation
                        self.advance();
                        let operand = self.parse_unary()?;
                        return Ok(AstNode::UnaryOp {
                            operator: UnaryOperator::Negate,
                            operand: Box::new(operand),
                        });
                    }
                }
                TokenType::Sharp => {
                    self.advance();
                    let operand = self.parse_unary()?;
                    return Ok(AstNode::UnaryOp {
                        operator: UnaryOperator::Count,
                        operand: Box::new(operand),
                    });
                }
                _ => (),
            }
        }

        self.parse_postfix()
    }

    fn has_trailing_semicolon(&mut self) -> bool {
        let mut lookahead_pos = self.current + 1;
        let mut bracket_depth = 1;
        let mut has_trailing_semicolon = false;

        while lookahead_pos < self.tokens.len() && bracket_depth > 0 {
            match &self.tokens[lookahead_pos].token_type {
                TokenType::LeftBracket => bracket_depth += 1,
                TokenType::RightBracket => {
                    bracket_depth -= 1;
                    if bracket_depth == 0 {
                        // Check if there's a semicolon right before the closing bracket
                        if lookahead_pos > self.current + 1 {
                            if let Some(prev_token) = self.tokens.get(lookahead_pos - 1) {
                                if prev_token.token_type == TokenType::Semicolon {
                                    has_trailing_semicolon = true;
                                }
                            }
                        }
                        break;
                    }
                }
                _ => {}
            }
            lookahead_pos += 1;
        }

        has_trailing_semicolon
    }

    fn parse_fn_args(&mut self) -> WqResult<Vec<AstNode>> {
        let mut args = Vec::new();

        // Parse arguments until final semicolon
        while let Some(token) = self.current_token() {
            match token.token_type {
                TokenType::Semicolon => {
                    self.advance();
                    // Check if this is the final semicolon before ']'
                    if let Some(next_token) = self.current_token() {
                        if next_token.token_type == TokenType::RightBracket {
                            break;
                        }
                    }
                }
                TokenType::RightBracket => {
                    // todo: currently unreachable
                    return Err(self.syntax_error(token, "Function call must end with ';]'"));
                }
                _ => {
                    let arg = self.parse_expression()?;
                    args.push(arg);
                }
            }
        }
        self.consume(TokenType::RightBracket)?;
        Ok(args)
    }

    fn parse_index_args(&mut self) -> WqResult<AstNode> {
        let mut args = Vec::new();

        loop {
            let expr = self.parse_expression()?;
            args.push(expr);

            if let Some(token) = self.current_token() {
                match token.token_type {
                    TokenType::Semicolon => {
                        self.advance();
                        continue;
                    }
                    TokenType::RightBracket => {
                        self.advance();
                        break;
                    }
                    _ => {
                        return Err(self.syntax_error(token, "Expected ';' or ']' in index"));
                    }
                }
            } else {
                return Err(self.eof_error("Unexpected end of input in index"));
            }
        }

        if args.len() == 1 {
            Ok(args.remove(0))
        } else {
            Ok(AstNode::List(args))
        }
    }

    fn parse_postfix(&mut self) -> WqResult<AstNode> {
        let mut expr = self.parse_primary()?;

        loop {
            let next = match self.current_token() {
                Some(t) => t.clone(),
                None => break,
            };

            match next.token_type {
                TokenType::LeftBracket => {
                    if self.has_trailing_semicolon() {
                        self.advance();
                        let args = self.parse_fn_args()?;
                        expr = match expr {
                            AstNode::Variable(name) => AstNode::Call { name, args },
                            _ => AstNode::CallAnonymous {
                                object: Box::new(expr),
                                args,
                            },
                        };
                    } else {
                        self.advance();
                        let index = self.parse_index_args()?;
                        expr = AstNode::Index {
                            object: Box::new(expr),
                            index: Box::new(index),
                        };
                    }
                }
                TokenType::Integer(_)
                | TokenType::Float(_)
                | TokenType::Character(_)
                | TokenType::String(_)
                | TokenType::Symbol(_)
                | TokenType::Identifier(_)
                | TokenType::True
                | TokenType::False
                | TokenType::LeftParen => {
                    let arg = self.parse_unary()?;
                    let args = vec![arg];
                    expr = match expr {
                        AstNode::Variable(name) => AstNode::Call { name, args },
                        _ => AstNode::CallAnonymous {
                            object: Box::new(expr),
                            args,
                        },
                    };
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> WqResult<AstNode> {
        if let Some(token) = self.current_token() {
            match &token.token_type {
                TokenType::Integer(n) => {
                    let val = *n;
                    self.advance();
                    Ok(AstNode::Literal(Value::Int(val)))
                }
                TokenType::Float(f) => {
                    let val = *f;
                    self.advance();
                    Ok(AstNode::Literal(Value::Float(val)))
                }
                TokenType::Character(c) => {
                    let val = *c;
                    self.advance();
                    Ok(AstNode::Literal(Value::Char(val)))
                }
                TokenType::String(s) => {
                    let val = s.clone();
                    self.advance();
                    let chars = val.chars().map(Value::Char).collect();
                    Ok(AstNode::Literal(Value::List(chars)))
                }
                TokenType::Symbol(s) => {
                    let val = s.clone();
                    self.advance();
                    Ok(AstNode::Literal(Value::Symbol(val)))
                }
                TokenType::True => {
                    self.advance();
                    Ok(AstNode::Literal(Value::Bool(true)))
                }
                TokenType::False => {
                    self.advance();
                    Ok(AstNode::Literal(Value::Bool(false)))
                }
                TokenType::Dollar => {
                    // Parse conditional: $[condition;true_branch;false_branch]
                    self.parse_conditional()
                }
                TokenType::DollarDot => {
                    // Parse conditional: $[condition;true_branch;false_branch]
                    self.parse_conditional_dot()
                }
                TokenType::Identifier(name) => {
                    let val = name.clone();
                    self.advance();

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
                TokenType::LeftBrace => {
                    // Parse function definition
                    self.parse_function()
                }
                TokenType::LeftParen => {
                    self.advance(); // consume '('

                    // Check for empty list/dict
                    if let Some(token) = self.current_token() {
                        if token.token_type == TokenType::RightParen {
                            self.advance();
                            return Ok(AstNode::List(Vec::new()));
                        }
                    }

                    // Determine if this is a dictionary by looking for `symbol:` pattern
                    let mut is_dict = false;

                    while let Some(Token {
                        token_type: TokenType::Newline,
                        ..
                    }) = self.current_token()
                    {
                        self.advance();
                    }

                    if let Some(Token {
                        token_type: TokenType::Symbol(_),
                        ..
                    }) = self.current_token()
                    {
                        if let Some(next) = self.peek_token() {
                            if next.token_type == TokenType::Colon {
                                is_dict = true;
                            }
                        }
                    }

                    if is_dict {
                        let mut pairs = Vec::new();
                        loop {
                            while let Some(Token {
                                token_type: TokenType::Newline,
                                ..
                            }) = self.current_token()
                            {
                                self.advance();
                            }
                            if let Some(token) = self.current_token() {
                                if token.token_type == TokenType::RightParen {
                                    self.advance();
                                    break;
                                }
                            } else {
                                return Err(self.eof_error("Unexpected end of input in dict"));
                            }

                            let key_token = self
                                .current_token()
                                .ok_or_else(|| self.eof_error("Unexpected end of input in dict"))?;
                            let key = match &key_token.token_type {
                                TokenType::Symbol(s) => {
                                    let s = s.clone();
                                    self.advance();
                                    s
                                }
                                TokenType::Eof => {
                                    return Err(self.eof_error("Unexpected end of input in dict"));
                                }
                                _ => {
                                    return Err(
                                        self.syntax_error(key_token, "Expected symbol key in dict")
                                    );
                                }
                            };
                            self.consume(TokenType::Colon)?;
                            let value = self.parse_expression()?;
                            pairs.push((key, value));
                            if let Some(token) = self.current_token() {
                                match token.token_type {
                                    TokenType::Semicolon | TokenType::Newline => {
                                        self.advance();
                                        continue;
                                    }
                                    TokenType::Eof => {
                                        return Err(
                                            self.eof_error("Unexpected end of input in dict")
                                        );
                                    }
                                    TokenType::RightParen => {
                                        self.advance();
                                        break;
                                    }
                                    _ => {
                                        return Err(
                                            self.syntax_error(token, "Expected ';' or ')' in dict")
                                        );
                                    }
                                }
                            } else {
                                return Err(self.eof_error("Unexpected end of input in dict"));
                            }
                        }
                        Ok(AstNode::Dict(pairs))
                    } else {
                        let mut elements = Vec::new();
                        loop {
                            while let Some(Token {
                                token_type: TokenType::Newline,
                                ..
                            }) = self.current_token()
                            {
                                self.advance();
                            }
                            if let Some(token) = self.current_token() {
                                if token.token_type == TokenType::RightParen {
                                    self.advance();
                                    break;
                                }
                            } else {
                                return Err(self.eof_error("Unexpected end of input in list"));
                            }

                            let expr = self.parse_expression()?;
                            elements.push(expr);
                            if let Some(token) = self.current_token() {
                                match token.token_type {
                                    TokenType::Semicolon | TokenType::Newline => {
                                        self.advance();
                                        continue;
                                    }
                                    TokenType::Eof => {
                                        return Err(
                                            self.eof_error("Unexpected end of input in list")
                                        );
                                    }
                                    TokenType::RightParen => {
                                        self.advance();
                                        break;
                                    }
                                    _ => {
                                        return Err(
                                            self.syntax_error(token, "Expected ';' or ')' in list")
                                        );
                                    }
                                }
                            } else {
                                return Err(self.eof_error("Unexpected end of input in list"));
                            }
                        }
                        if elements.len() == 1 {
                            Ok(elements.into_iter().next().unwrap())
                        } else {
                            Ok(AstNode::List(elements))
                        }
                    }
                }
                TokenType::Eof => {
                    Err(self.eof_error("Unexpected end of input"))
                }
                _ => {
                    Err(self
                        .syntax_error(token, &format!("Unexpected token: {:?}", token.token_type)))
                }
            }
        } else {
            Err(self.eof_error("Unexpected end of input"))
        }
    }

    fn parse_function(&mut self) -> WqResult<AstNode> {
        self.advance(); // consume '{'

        let mut params = None;

        // Check for explicit parameter declaration [a;b]
        if let Some(token) = self.current_token() {
            if token.token_type == TokenType::LeftBracket {
                self.advance(); // consume '['

                let mut param_names = Vec::new();

                loop {
                    match self.current_token().map(|t| (&t.token_type, t)) {
                        Some((TokenType::Identifier(name), _)) => {
                            param_names.push(name.clone());
                            self.advance();
                            // eat ‘;’ or break on ‘]’ …
                        }
                        Some((TokenType::Semicolon, _)) => {
                            self.advance();
                            continue;
                        }
                        Some((TokenType::RightBracket, _)) => {
                            self.advance();
                            break;
                        }
                        Some((TokenType::Eof, _)) => {
                            return Err(self.eof_error("Unexpected end of input in parameter list"));
                        }
                        Some((TokenType::Newline, _)) => {
                            self.advance();
                        }
                        Some((_, tok)) => {
                            return Err(self.syntax_error(tok, "Expected identifier, ';' or ']'"));
                        }
                        None => {
                            return Err(self.eof_error("Unexpected end of input in parameter list"));
                        }
                    }
                }

                params = Some(param_names);
            }
        }

        // Parse function body allowing multiple statements
        let body = self.parse_block()?;

        // Expect closing brace
        self.consume(TokenType::RightBrace)?;

        Ok(AstNode::Function {
            params,
            body: Box::new(body),
        })
    }

    fn parse_conditional(&mut self) -> WqResult<AstNode> {
        self.advance(); // consume '$'
        self.consume(TokenType::LeftBracket)?;

        self.skip_newlines();

        let condition = self.parse_expression()?;
        self.consume(TokenType::Semicolon)?;

        let true_branch = self.parse_branch_sequence(vec![TokenType::Semicolon])?;
        self.consume(TokenType::Semicolon)?;

        let false_branch = self.parse_branch_sequence(vec![TokenType::RightBracket])?;
        self.consume(TokenType::RightBracket)?;

        Ok(AstNode::Conditional {
            condition: Box::new(condition),
            true_branch: Box::new(true_branch),
            false_branch: Some(Box::new(false_branch)),
        })
    }

    fn parse_conditional_dot(&mut self) -> WqResult<AstNode> {
        self.advance(); // consume '$.'
        self.consume(TokenType::LeftBracket)?;

        self.skip_newlines();

        let condition = self.parse_expression()?;
        self.consume(TokenType::Semicolon)?;

        let true_branch = self.parse_branch_sequence(vec![TokenType::RightBracket])?;
        self.consume(TokenType::RightBracket)?;

        Ok(AstNode::Conditional {
            condition: Box::new(condition),
            true_branch: Box::new(true_branch),
            false_branch: None,
        })
    }

    fn parse_while(&mut self) -> WqResult<AstNode> {
        self.skip_newlines();

        let condition = self.parse_expression()?;
        self.consume(TokenType::Semicolon)?;

        let body = self.parse_branch_sequence(vec![TokenType::RightBracket])?;
        self.consume(TokenType::RightBracket)?;

        Ok(AstNode::WhileLoop {
            condition: Box::new(condition),
            body: Box::new(body),
        })
    }

    fn parse_for(&mut self) -> WqResult<AstNode> {
        self.skip_newlines();

        let count_expr = self.parse_expression()?;
        self.consume(TokenType::Semicolon)?;

        let body = self.parse_branch_sequence(vec![TokenType::RightBracket])?;
        self.consume(TokenType::RightBracket)?;

        Ok(AstNode::ForLoop {
            count: Box::new(count_expr),
            body: Box::new(body),
        })
    }

    fn parse_branch_sequence(&mut self, ends: Vec<TokenType>) -> WqResult<AstNode> {
        let mut statements = Vec::new();

        loop {
            // skip leading newlines
            while let Some(Token {
                token_type: TokenType::Newline,
                ..
            }) = self.current_token()
            {
                self.advance();
            }

            // if already at one of the end‐tokens, stop
            if let Some(token) = self.current_token() {
                if ends
                    .iter()
                    .any(|e| std::mem::discriminant(&token.token_type) == std::mem::discriminant(e))
                {
                    break;
                }
            } else {
                return Err(self.eof_error("Unexpected end of input in branch"));
            }

            // parse one expression
            let expr = self.parse_expression()?;
            statements.push(expr);

            // now see what follows: semicolon/newline, an end‐token, or a syntax error
            if let Some(token) = self.current_token() {
                // treat semicolon or newline specially
                if token.token_type == TokenType::Semicolon
                    || token.token_type == TokenType::Newline
                {
                    // if semicolon is itself one of the ends, break; otherwise consume and continue
                    let sem_dis = std::mem::discriminant(&TokenType::Semicolon);
                    if ends.iter().any(|e| std::mem::discriminant(e) == sem_dis) {
                        break;
                    } else {
                        self.advance();
                        continue;
                    }
                }
                // if it’s one of end‐tokens, break
                else if ends
                    .iter()
                    .any(|e| std::mem::discriminant(&token.token_type) == std::mem::discriminant(e))
                {
                    break;
                } else if ends.iter().any(|_| {
                    std::mem::discriminant(&token.token_type)
                        == std::mem::discriminant(&TokenType::Eof)
                }) {
                    return Err(self.eof_error("Unexpected end of input in branch"));
                }
                // otherwise it’s an error
                else {
                    return Err(self.syntax_error(
                        token,
                        &format!(
                            "Expected one of {:?} or ';', found {:?}",
                            ends, token.token_type
                        ),
                    ));
                }
            } else {
                return Err(self.eof_error("Unexpected eof"));
            }
        }

        // collapse single‐stmt blocks
        if statements.len() == 1 {
            Ok(statements.remove(0))
        } else {
            Ok(AstNode::Block(statements))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse_string(input: &str) -> WqResult<AstNode> {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens, input.to_string());
        parser.parse()
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
    fn test_parse_multi_index() {
        let ast = parse_string("a[1;2]").unwrap();
        assert_eq!(
            ast,
            AstNode::Index {
                object: Box::new(AstNode::Variable("a".into())),
                index: Box::new(AstNode::List(vec![
                    AstNode::Literal(Value::Int(1)),
                    AstNode::Literal(Value::Int(2)),
                ])),
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
}
