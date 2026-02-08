use std::{iter::Peekable, str::Chars};

use crate::{
    token::{Token, TokenType},
    value::WqResult,
    wqerror::{WqError, WqErrorType},
};

use num_bigint::BigInt;
use num_traits::Num;

pub struct Lexer<'a> {
    input: Peekable<Chars<'a>>,
    source: &'a str,
    position: usize,
    line: usize,
    column: usize,
    current_char: Option<char>,
    // 2-character lookahead window
    la1: Option<char>,
    la2: Option<char>,
    // Current byte position (immediately after `current_char`)
    byte_pos: usize,
    // Optional global source context for better error spans
    global_source: Option<&'a str>,
    line_base: usize,
    col_base: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        let mut input_iter = input.chars().peekable();
        // Initialize the lookahead window
        let la1 = input_iter.next();
        let la2 = input_iter.next();

        let mut lexer = Lexer {
            input: input_iter,
            source: input,
            position: 0,
            line: 1,
            column: 0,
            current_char: None,
            la1,
            la2,
            byte_pos: 0,
            global_source: None,
            line_base: 0,
            col_base: 0,
        };
        lexer.advance();
        lexer
    }

    /// Provide a global source context and base byte offset for more accurate error spans -
    /// When lexing a snippet within a larger file
    pub fn with_ctx(mut self, global_source: &'a str, base_offset: usize) -> Self {
        let base = base_offset.min(global_source.len());
        let line_base = global_source[..base]
            .bytes()
            .filter(|b| *b == b'\n')
            .count();
        let col_base = if base == 0 {
            0
        } else {
            match global_source[..base].rfind('\n') {
                Some(i) => global_source[i + 1..base].chars().count(),
                None => global_source[..base].chars().count(),
            }
        };
        self.global_source = Some(global_source);
        self.line_base = line_base;
        self.col_base = col_base;
        self
    }

    fn syntax_error_span(
        &self,
        _line: usize,
        _column: usize,
        byte_start: usize,
        byte_end: usize,
        msg: &'static str,
    ) -> WqError {
        let src = self.source;
        let bs = byte_start.min(src.len());
        let be = byte_end.min(src.len());
        // find local line index & column from bytes
        let pre = &src[..bs];
        let local_line_idx = pre.bytes().filter(|b| *b == b'\n').count(); // 0-based
        let line_start_byte = pre.rfind('\n').map(|i| i + 1).unwrap_or(0);
        // 1-based column within the local line
        let mut disp_col = src[line_start_byte..bs].chars().count() + 1;
        let width = if be > bs {
            src[bs..be]
                .chars()
                .take_while(|&c| c != '\n')
                .count()
                .max(1)
        } else {
            1
        };
        let (disp_line, src_line) = if let Some(gs) = self.global_source {
            let line_no = local_line_idx + 1 + self.line_base;
            if local_line_idx == 0 {
                disp_col += self.col_base;
            }
            (line_no, gs.lines().nth(line_no - 1).unwrap_or(""))
        } else {
            (
                local_line_idx + 1,
                src.lines().nth(local_line_idx).unwrap_or(""),
            )
        };
        let pointer = " ".repeat(disp_col.saturating_sub(1)) + &"~".repeat(width);
        WqError::new(WqErrorType::Syntax)
            .src("lexer")
            .msg(msg)
            .attach_note(format!("at {disp_line}:{disp_col}\n{src_line}\n{pointer}",))
    }

    fn advance(&mut self) {
        // Consume the previous current_char, if any
        if let Some(prev) = self.current_char {
            self.byte_pos += prev.len_utf8();
            if prev == '\n' {
                self.line += 1;
                self.column = 0;
            } else {
                self.column += 1;
            }
        }
        // Shift the lookahead window: current_char <- la1 <- la2 <- input.next()
        self.current_char = self.la1;
        self.la1 = self.la2;
        self.la2 = self.input.next();

        if self.current_char.is_some() {
            self.position += 1;
        }
    }

    #[inline]
    fn peek(&self) -> Option<char> {
        self.la1
    }

    #[inline]
    fn peek2(&self) -> Option<char> {
        self.la2
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.current_char {
            if ch.is_whitespace() && ch != '\n' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn read_number(
        &mut self,
        start_line: usize,
        start_column: usize,
        start_byte: usize,
    ) -> WqResult<TokenType> {
        let mut raw_lit = String::new(); // digits and optional `_`
        let mut is_float = false;
        let mut has_exp = false;
        // --- detect 0b / 0o / 0x prefix ---
        let mut base: u32 = 10;
        let mut had_prefix = false;
        if self.current_char == Some('0')
            && let Some(next_ch) = self.peek()
        {
            match next_ch {
                'b' | 'B' => {
                    base = 2;
                    had_prefix = true;
                }
                'o' | 'O' => {
                    base = 8;
                    had_prefix = true;
                }
                'x' | 'X' => {
                    base = 16;
                    had_prefix = true;
                }
                _ => {}
            }
            if had_prefix {
                // consume '0' and the base letter
                self.advance();
                self.advance();
                let is_digit_for_base = |c: char| -> bool {
                    match base {
                        2 => c == '0' || c == '1',
                        8 => c.is_ascii_digit() && c <= '7',
                        10 => c.is_ascii_digit(),
                        16 => c.is_ascii_hexdigit(),
                        _ => false,
                    }
                };
                let mut prev_was_digit = false;
                let mut saw_digit = false;
                while let Some(ch) = self.current_char {
                    if is_digit_for_base(ch) {
                        raw_lit.push(ch);
                        prev_was_digit = true;
                        saw_digit = true;
                        self.advance();
                    } else if ch == '_' {
                        // allow underscore only between two valid digits
                        if prev_was_digit
                            && let Some(nc) = self.peek()
                            && is_digit_for_base(nc)
                        {
                            self.advance(); // consume '_'
                            prev_was_digit = false;
                            continue;
                        }
                        break;
                    } else {
                        // For non-decimal prefixed literals, stop on '.'/'e' as well.
                        break;
                    }
                }
                if !saw_digit {
                    return Err(self.syntax_error_span(
                        start_line,
                        start_column,
                        start_byte,
                        self.byte_pos,
                        "expected digits after base prefix",
                    ));
                }
                let lit = raw_lit.replace('_', "");
                if let Ok(n) = i64::from_str_radix(&lit, base) {
                    return Ok(TokenType::Integer(n));
                }
                match BigInt::from_str_radix(&lit, base) {
                    Ok(big) => return Ok(TokenType::BigInteger(big)),
                    Err(_) => {
                        return Err(self.syntax_error_span(
                            start_line,
                            start_column,
                            start_byte,
                            self.byte_pos,
                            "invalid integer literal",
                        ));
                    }
                }
            }
        }

        // no prefix ===============================================================
        while let Some(ch) = self.current_char {
            if ch.is_ascii_digit() {
                raw_lit.push(ch);
                self.advance();
            } else if ch == '_' {
                // only allow underscore between digits
                if raw_lit
                    .chars()
                    .last()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
                    && let Some(next_ch) = self.peek()
                    && next_ch.is_ascii_digit()
                {
                    self.advance(); // consume '_'
                    continue;
                }
                break;
            } else if ch == '.' && !is_float && !has_exp {
                // fractional part
                if let Some(next_ch) = self.peek() {
                    if next_ch.is_ascii_digit() {
                        is_float = true;
                        raw_lit.push(ch);
                        self.advance();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else if (ch == 'e' || ch == 'E') && !has_exp {
                // exponent part (decimal only)
                has_exp = true;
                is_float = true;
                raw_lit.push(ch);
                self.advance();
                // optional +/-
                if let Some(sign_ch @ ('+' | '-')) = self.current_char {
                    raw_lit.push(sign_ch);
                    self.advance();
                }
            } else {
                break;
            }
        }
        let lit = raw_lit.replace('_', "");
        if is_float {
            match lit.parse::<f64>() {
                Ok(n) if n.is_finite() => Ok(TokenType::Float(n)),
                _ => Err(self.syntax_error_span(
                    start_line,
                    start_column,
                    start_byte,
                    self.byte_pos,
                    "float literal overflow",
                )),
            }
        } else {
            match lit.parse::<i64>() {
                Ok(n) => Ok(TokenType::Integer(n)),
                Err(_) => match lit.parse::<BigInt>() {
                    Ok(big) => Ok(TokenType::BigInteger(big)),
                    Err(_) => Err(self.syntax_error_span(
                        start_line,
                        start_column,
                        start_byte,
                        self.byte_pos,
                        "invalid integer literal",
                    )),
                },
            }
        }
    }

    fn read_identifier(&mut self) -> String {
        let mut identifier = String::new();
        while let Some(ch) = self.current_char {
            if ch.is_alphanumeric() || ch == '_' || ch == '?' {
                identifier.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        identifier
    }

    fn read_symbol(&mut self) -> TokenType {
        self.advance(); // consume the backtick
        let symbol_name = self.read_identifier();
        if symbol_name.is_empty() {
            TokenType::Backtick
        } else {
            TokenType::Symbol(symbol_name)
        }
    }

    fn read_string_or_char(
        &mut self,
        start_line: usize,
        start_column: usize,
        start_byte: usize,
    ) -> WqResult<TokenType> {
        // consume opening quote
        self.advance();
        let content_start_byte = self.byte_pos;
        let mut raw = String::new();
        let mut closed = false;
        while let Some(ch) = self.current_char {
            match ch {
                '\\' => {
                    // include backslash and the next char literally in raw
                    raw.push('\\');
                    self.advance();
                    if let Some(next) = self.current_char {
                        raw.push(next);
                        self.advance();
                    } else {
                        // EOF right after backslash -> unterminated string
                        break;
                    }
                }
                '"' => {
                    // terminator not preceded by a raw backslash in this step
                    self.advance();
                    closed = true;
                    break;
                }
                other => {
                    raw.push(other);
                    self.advance();
                }
            }
        }
        if !closed {
            return Err(self.syntax_error_span(
                start_line,
                start_column,
                start_byte,
                self.byte_pos,
                "string is not properly terminated",
            ));
        }

        // Now unescape the raw inner content using the shared helper
        match crate::escape::unescape_string_inner(&raw) {
            Ok(content) => {
                if content.chars().count() == 1 {
                    Ok(TokenType::Character(content.chars().next().unwrap()))
                } else {
                    Ok(TokenType::String(content))
                }
            }
            Err(err) => {
                // Map to a syntax error with a reasonable message
                use crate::escape::UnescapeErrorKind::*;
                let msg = match err.kind {
                    InvalidUnicodeEscape => "invalid unicode escape",
                    InvalidUnicodeScalar => "invalid unicode escape",
                };
                let err_byte_start = content_start_byte.saturating_add(err.index);
                Err(self.syntax_error_span(
                    start_line,
                    start_column,
                    err_byte_start,
                    err_byte_start + 1,
                    msg,
                ))
            }
        }
    }

    fn read_comment(&mut self) -> TokenType {
        let mut comment = String::new();
        while let Some(ch) = self.current_char {
            if ch == '\n' {
                break;
            }
            comment.push(ch);
            self.advance();
        }
        TokenType::Comment(comment)
    }

    // Read a raw string starting at the '"'
    // * No escape processing
    // * Always produces TokenType::String
    fn read_raw_string(
        &mut self,
        start_line: usize,
        start_column: usize,
        start_byte: usize,
    ) -> WqResult<TokenType> {
        self.skip_whitespace();
        if self.current_char != Some('"') {
            return Err(self.syntax_error_span(
                start_line,
                start_column,
                start_byte,
                self.byte_pos,
                "expected '\"' after raw string prefix",
            ));
        }
        self.advance(); // consume opening quote
        let mut raw = String::new();
        let mut closed = false;
        while let Some(ch) = self.current_char {
            match ch {
                '"' => {
                    self.advance();
                    closed = true;
                    break;
                }
                other => {
                    raw.push(other);
                    self.advance();
                }
            }
        }
        if !closed {
            return Err(self.syntax_error_span(
                start_line,
                start_column,
                start_byte,
                self.byte_pos,
                "string is not properly terminated",
            ));
        }
        Ok(TokenType::String(raw))
    }

    pub fn next_token(&mut self) -> WqResult<Token> {
        loop {
            let token_line = self.line;
            let token_column = self.column + 1;
            let token_position = self.position;
            let token_byte_start = self.byte_pos;

            let emit = |t: TokenType, byte_end: usize| -> WqResult<Token> {
                Ok(Token::new(
                    t,
                    token_position,
                    token_line,
                    token_column,
                    token_byte_start,
                    byte_end, // current end
                ))
            };

            let ch = self.current_char;
            let nxt = self.peek();

            match ch {
                None => return emit(TokenType::Eof, self.byte_pos),

                Some(' ') | Some('\t') | Some('\r') => {
                    self.skip_whitespace();
                    continue;
                }

                Some('\n') => {
                    self.advance();
                    return emit(TokenType::Newline, self.byte_pos);
                }

                Some('*') => match nxt {
                    Some('*') => {
                        self.advance();
                        self.advance();
                        return emit(TokenType::Matmul, self.byte_pos);
                    }
                    _ => {
                        self.advance();
                        return emit(TokenType::Multiply, self.byte_pos);
                    }
                },

                Some('/') => {
                    match nxt {
                        Some('/') => {
                            self.advance(); // consume first '/'
                            let comment = self.read_comment();
                            return emit(comment, self.byte_pos);
                        }
                        Some('.') => {
                            self.advance();
                            self.advance(); // '/.'
                            return emit(TokenType::DivideDot, self.byte_pos);
                        }
                        _ => {
                            self.advance();
                            return emit(TokenType::Divide, self.byte_pos);
                        }
                    }
                }

                Some('%') => {
                    if nxt == Some('.') {
                        self.advance();
                        self.advance();
                        return emit(TokenType::ModuloDot, self.byte_pos);
                    } else {
                        self.advance();
                        return emit(TokenType::Modulo, self.byte_pos);
                    }
                }

                Some('$') => {
                    if nxt == Some('$') {
                        self.advance();
                        self.advance();
                        return emit(TokenType::DollarDollar, self.byte_pos);
                    } else if nxt == Some('.') {
                        self.advance();
                        self.advance();
                        return emit(TokenType::DollarDot, self.byte_pos);
                    } else {
                        self.advance();
                        return emit(TokenType::Dollar, self.byte_pos);
                    }
                }

                Some('<') => {
                    if nxt == Some('=') {
                        self.advance();
                        self.advance();
                        return emit(TokenType::LessThanOrEqual, self.byte_pos);
                    } else {
                        self.advance();
                        return emit(TokenType::LessThan, self.byte_pos);
                    }
                }

                Some('>') => {
                    if nxt == Some('=') {
                        self.advance();
                        self.advance();
                        return emit(TokenType::GreaterThanOrEqual, self.byte_pos);
                    } else {
                        self.advance();
                        return emit(TokenType::GreaterThan, self.byte_pos);
                    }
                }

                Some('.') => {
                    let n1 = nxt;
                    let n2 = self.peek2();
                    match (n1, n2) {
                        (Some('.'), Some('.')) => {
                            self.advance();
                            self.advance();
                            self.advance();
                            return emit(TokenType::Ellipsis, self.byte_pos);
                        }
                        (Some('.'), Some('=')) => {
                            self.advance();
                            self.advance();
                            self.advance();
                            return emit(TokenType::RangeInclusive, self.byte_pos);
                        }
                        (Some('.'), _) => {
                            self.advance();
                            self.advance();
                            return emit(TokenType::Range, self.byte_pos);
                        }
                        _ => {
                            // Unknown single '.': consume and skip
                            self.advance();
                            continue;
                        }
                    }
                }

                Some('@') => {
                    self.advance(); // consume '@'
                    let tok = match self.current_char {
                        Some('b') => {
                            self.advance();
                            TokenType::AtBreak
                        }
                        Some('c') => {
                            self.advance();
                            TokenType::AtContinue
                        }
                        Some('r') => {
                            self.advance();
                            TokenType::AtReturn
                        }
                        Some('t') => {
                            self.advance();
                            TokenType::AtTry
                        }
                        Some('f') => {
                            self.advance();
                            TokenType::AtFormat
                        }
                        // @l followed by a quoted string is a raw string; lex it now
                        Some('l') => {
                            self.advance();
                            // Expect a '"' and then consume as raw string
                            let t =
                                self.read_raw_string(token_line, token_column, token_byte_start)?;
                            return emit(t, self.byte_pos);
                        }
                        _ => continue, // unknown @ sequence, skip
                    };
                    return emit(tok, self.byte_pos);
                }

                // Symbols that are always a single token
                Some('+') => {
                    self.advance();
                    return emit(TokenType::Plus, self.byte_pos);
                }
                Some('-') => {
                    self.advance();
                    return emit(TokenType::Minus, self.byte_pos);
                }

                Some('^') => {
                    self.advance();
                    return emit(TokenType::Power, self.byte_pos);
                }
                Some(':') => {
                    self.advance();
                    return emit(TokenType::Colon, self.byte_pos);
                }
                Some('=') => {
                    self.advance();
                    return emit(TokenType::Equal, self.byte_pos);
                }
                Some('~') => {
                    self.advance();
                    return emit(TokenType::NotEqual, self.byte_pos);
                }
                Some('#') => {
                    self.advance();
                    return emit(TokenType::Sharp, self.byte_pos);
                }
                Some('|') => {
                    self.advance();
                    return emit(TokenType::Pipe, self.byte_pos);
                }
                Some('(') => {
                    self.advance();
                    return emit(TokenType::LeftParen, self.byte_pos);
                }
                Some(')') => {
                    self.advance();
                    return emit(TokenType::RightParen, self.byte_pos);
                }
                Some('[') => {
                    self.advance();
                    return emit(TokenType::LeftBracket, self.byte_pos);
                }
                Some(']') => {
                    self.advance();
                    return emit(TokenType::RightBracket, self.byte_pos);
                }
                Some('{') => {
                    self.advance();
                    return emit(TokenType::LeftBrace, self.byte_pos);
                }
                Some('}') => {
                    self.advance();
                    return emit(TokenType::RightBrace, self.byte_pos);
                }
                Some(';') => {
                    self.advance();
                    return emit(TokenType::Semicolon, self.byte_pos);
                }
                Some(',') => {
                    self.advance();
                    return emit(TokenType::Comma, self.byte_pos);
                }
                Some('\'') => {
                    self.advance();
                    return emit(TokenType::Apostrophe, self.byte_pos);
                }

                // Backtick-quoted symbol
                Some('`') => {
                    let symbol = self.read_symbol();
                    return emit(symbol, self.byte_pos);
                }

                // Strings / chars
                Some('"') => {
                    let t = self.read_string_or_char(token_line, token_column, token_byte_start)?;
                    return emit(t, self.byte_pos);
                }

                // Numbers
                Some(c) if c.is_ascii_digit() => {
                    let number = self.read_number(token_line, token_column, token_byte_start)?;
                    return emit(number, self.byte_pos);
                }

                // Identifiers and keywords
                Some(c) if c.is_alphabetic() || c == '_' => {
                    // Otherwise, read an identifier and map to keywords if any.
                    let ident = self.read_identifier();
                    let tt = match ident.as_str() {
                        "true" => TokenType::True,
                        "false" => TokenType::False,
                        "inf" => TokenType::Inf,
                        "nan" => TokenType::Nan,
                        _ => TokenType::Identifier(ident),
                    };
                    return emit(tt, self.byte_pos);
                }

                // Unknown byte
                Some(_) => {
                    // self.advance();
                    let err = self.syntax_error_span(
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                        "unrecognized character",
                    );
                    self.advance();
                    return Err(err);
                    // continue;
                }
            }
        }
    }

    pub fn tokenize(&mut self) -> WqResult<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token()?;
            let is_eof = token.token_type == TokenType::Eof;
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigInt;

    #[test]
    fn test_tokenize_numbers() {
        let mut lexer = Lexer::new("42 3.1 -5 1e3 2E-2");
        let tokens = lexer.tokenize().unwrap();

        assert_eq!(tokens[0].token_type, TokenType::Integer(42));
        assert_eq!(tokens[1].token_type, TokenType::Float(3.1));
        assert_eq!(tokens[2].token_type, TokenType::Minus);
        assert_eq!(tokens[3].token_type, TokenType::Integer(5));
        assert_eq!(tokens[4].token_type, TokenType::Float(1000.0));
        assert_eq!(tokens[5].token_type, TokenType::Float(0.02));
    }

    #[test]
    fn test_tokenize_bigint_literal() {
        let big = BigInt::from(i64::MAX) + BigInt::from(1);
        let literal = big.to_string();
        let mut lexer = Lexer::new(&literal);
        let tokens = lexer.tokenize().unwrap();
        match &tokens[0].token_type {
            TokenType::BigInteger(n) => assert_eq!(*n, big),
            other => panic!("expected BigInteger token, got {other:?}"),
        }
    }

    #[test]
    fn test_tokenize_operators() {
        let mut lexer = Lexer::new("+ - * / % ^ |");
        let tokens = lexer.tokenize().unwrap();

        assert_eq!(tokens[0].token_type, TokenType::Plus);
        assert_eq!(tokens[1].token_type, TokenType::Minus);
        assert_eq!(tokens[2].token_type, TokenType::Multiply);
        assert_eq!(tokens[3].token_type, TokenType::Divide);
        assert_eq!(tokens[4].token_type, TokenType::Modulo);
        assert_eq!(tokens[5].token_type, TokenType::Power);
        assert_eq!(tokens[6].token_type, TokenType::Pipe);
    }

    #[test]
    fn test_tokenize_dot_operators() {
        let mut lexer = Lexer::new("/. %.");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::DivideDot);
        assert_eq!(tokens[1].token_type, TokenType::ModuloDot);
    }

    #[test]
    fn test_tokenize_inf_nan() {
        let mut lexer = Lexer::new("inf nan");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::Inf);
        assert_eq!(tokens[1].token_type, TokenType::Nan);
    }

    #[test]
    fn test_tokenize_symbols() {
        let mut lexer = Lexer::new("`hello `world");
        let tokens = lexer.tokenize().unwrap();

        assert_eq!(tokens[0].token_type, TokenType::Symbol("hello".to_string()));
        assert_eq!(tokens[1].token_type, TokenType::Symbol("world".to_string()));
    }

    #[test]
    fn test_tokenize_string() {
        let mut lexer = Lexer::new("\"ab\"");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::String("ab".to_string()));
    }

    #[test]
    fn test_escape_quote() {
        let mut lexer = Lexer::new("\"a\\\"b\"");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::String("a\"b".to_string()));
    }

    #[test]
    fn test_tokenize_char() {
        let mut lexer = Lexer::new("\"a\"");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::Character('a'));
    }

    #[test]
    fn test_tokenize_expression() {
        let mut lexer = Lexer::new("x:1+2*3");
        let tokens = lexer.tokenize().unwrap();

        assert_eq!(tokens[0].token_type, TokenType::Identifier("x".to_string()));
        assert_eq!(tokens[1].token_type, TokenType::Colon);
        assert_eq!(tokens[2].token_type, TokenType::Integer(1));
        assert_eq!(tokens[3].token_type, TokenType::Plus);
        assert_eq!(tokens[4].token_type, TokenType::Integer(2));
        assert_eq!(tokens[5].token_type, TokenType::Multiply);
        assert_eq!(tokens[6].token_type, TokenType::Integer(3));
    }

    #[test]
    fn test_tokenize_range_builder() {
        let mut lexer = Lexer::new("1..=2..3");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::Integer(1));
        assert_eq!(tokens[1].token_type, TokenType::RangeInclusive);
        assert_eq!(tokens[2].token_type, TokenType::Integer(2));
        assert_eq!(tokens[3].token_type, TokenType::Range);
        assert_eq!(tokens[4].token_type, TokenType::Integer(3));
    }

    #[test]
    fn test_identifier_with_question_mark() {
        let mut lexer = Lexer::new("a?:1 a? a???");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(
            tokens[0].token_type,
            TokenType::Identifier("a?".to_string())
        );
        assert_eq!(tokens[1].token_type, TokenType::Colon);
        assert_eq!(tokens[2].token_type, TokenType::Integer(1));
        assert_eq!(
            tokens[3].token_type,
            TokenType::Identifier("a?".to_string())
        );
        assert_eq!(
            tokens[4].token_type,
            TokenType::Identifier("a???".to_string())
        );
    }

    #[test]
    fn unterminated_string_errors() {
        let mut lexer = Lexer::new("\"abc");
        let res = lexer.tokenize();
        assert!(res.is_err());
    }

    #[test]
    fn integer_overflow_errors() {
        let mut lexer = Lexer::new("9223372036854775808");
        let res = lexer.tokenize().unwrap();
        assert!(matches!(res[0].token_type, TokenType::BigInteger(_)));
    }

    #[test]
    fn float_overflow_errors() {
        let big = "1".repeat(400) + ".0";
        let mut lexer = Lexer::new(&big);
        let res = lexer.tokenize();
        assert!(res.is_err());
    }

    #[test]
    fn at_try_token() {
        let mut lexer = Lexer::new("@t 1");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::AtTry);
    }

    #[test]
    fn test_raw_string_basic() {
        let mut lexer = Lexer::new("@l\"\\n\"");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::String("\\n".to_string()));
    }

    #[test]
    fn test_raw_string_unterminated_errors() {
        let mut lexer = Lexer::new("@l\"abc");
        let res = lexer.tokenize();
        assert!(res.is_err());
    }
}
