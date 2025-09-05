// use std::fmt;
use crate::value::WqResult;
use crate::wqerror::WqError;
use std::iter::Peekable;
use std::str::Chars;

#[cfg(not(target_arch = "wasm32"))]
use colored::Colorize;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenType {
    // Literals
    Integer(i64),
    Float(f64),
    Character(char),
    String(String),
    Symbol(String),

    // Operators
    Plus,
    Minus,
    Multiply,
    Power,
    Divide,
    DivideDot,
    Modulo,
    ModuloDot,

    // Assignment
    Colon,

    // Comparison operators
    Equal,
    NotEqual,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,

    // Conditional
    Dollar,
    DollarDot,

    Sharp,
    Pipe,

    // Delimiters
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,

    // Separators
    Semicolon,
    Comma,

    // Special
    Backtick,
    Quote,
    Whitespace,
    Newline,

    Eof,

    Identifier(String),

    Inf,
    Nan,

    True,
    False,

    Comment(String),
    AtBreak,
    AtContinue,
    AtReturn,
    AtAssert,
    AtTry,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub token_type: TokenType,
    pub position: usize,
    pub line: usize,
    pub column: usize,
    // Byte offsets into the original source string (half-open [start, end))
    pub byte_start: usize,
    pub byte_end: usize,
}

impl Token {
    pub fn new(
        token_type: TokenType,
        position: usize,
        line: usize,
        column: usize,
        byte_start: usize,
        byte_end: usize,
    ) -> Self {
        Token {
            token_type,
            position,
            line,
            column,
            byte_start,
            byte_end,
        }
    }
}

// impl fmt::Display for Token {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         write!(f, "{:?}@{}:{}", self.token_type, self.line, self.column)
//     }
// }

pub struct Lexer<'a> {
    input: Peekable<Chars<'a>>,
    source: &'a str,
    position: usize,
    line: usize,
    column: usize,
    current_char: Option<char>,
    // Current byte position (immediately after `current_char`)
    byte_pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        let mut lexer = Lexer {
            input: input.chars().peekable(),
            source: input,
            position: 0,
            line: 1,
            column: 0,
            current_char: None,
            byte_pos: 0,
        };
        lexer.advance();
        lexer
    }

    fn syntax_error_span(
        &self,
        line: usize,
        column: usize,
        byte_start: usize,
        byte_end: usize,
        msg: &str,
    ) -> WqError {
        let src_line = self
            .source
            .lines()
            .nth(line.saturating_sub(1))
            .unwrap_or("");
        let width = if byte_end > byte_start
            && byte_end <= self.source.len()
            && byte_start <= self.source.len()
        {
            self.source[byte_start..byte_end].chars().count().max(1)
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

    fn advance(&mut self) {
        self.current_char = self.input.next();
        if let Some(ch) = self.current_char {
            self.position += 1;
            self.byte_pos += ch.len_utf8();
            if ch == '\n' {
                self.line += 1;
                self.column = 0;
            } else {
                self.column += 1;
            }
        }
    }

    fn peek(&mut self) -> Option<&char> {
        self.input.peek()
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

        if self.current_char == Some('0') {
            if let Some(next_ch) = self.peek() {
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
                            if prev_was_digit {
                                if let Some(nc) = self.peek() {
                                    if is_digit_for_base(*nc) {
                                        self.advance(); // consume '_'
                                        prev_was_digit = false;
                                        continue;
                                    }
                                }
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
                    match i64::from_str_radix(&lit, base) {
                        Ok(n) => return Ok(TokenType::Integer(n)),
                        Err(_) => {
                            return Err(self.syntax_error_span(
                                start_line,
                                start_column,
                                start_byte,
                                self.byte_pos,
                                "integer literal overflow",
                            ));
                        }
                    }
                }
            }
        }

        // --- decimal (no prefix) path: keep your original float/int logic ---
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
                {
                    if let Some(next_ch) = self.peek() {
                        if next_ch.is_ascii_digit() {
                            self.advance(); // consume '_'
                            continue;
                        }
                    }
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
                if let Some(sign_ch) = self.current_char {
                    if sign_ch == '+' || sign_ch == '-' {
                        raw_lit.push(sign_ch);
                        self.advance();
                    }
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
                Err(_) => Err(self.syntax_error_span(
                    start_line,
                    start_column,
                    start_byte,
                    self.byte_pos,
                    "integer literal overflow",
                )),
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
        self.advance(); // Skip the backtick
        let symbol_name = self.read_identifier();
        TokenType::Symbol(symbol_name)
    }

    fn read_string_or_char(
        &mut self,
        start_line: usize,
        start_column: usize,
        start_byte: usize,
    ) -> WqResult<TokenType> {
        self.advance(); // skip opening quote
        let mut content = String::new();
        let mut closed = false;

        while let Some(ch) = self.current_char {
            match ch {
                '\\' => {
                    // have an escape; peek at the next char
                    if self.peek().is_some() {
                        // Consume the backslash
                        self.advance();
                        // next is the escaped char
                        match self.current_char.unwrap() {
                            '"' => {
                                content.push('"');
                                self.advance();
                            }
                            '\\' => {
                                content.push('\\');
                                self.advance();
                            }
                            'n' => {
                                content.push('\n');
                                self.advance();
                            }
                            'u' => {
                                self.advance(); // move to the next char after 'u'
                                if self.current_char == Some('{') {
                                    self.advance(); // consume '{'

                                    let mut val: u32 = 0;
                                    let mut digits = 0;

                                    while let Some(c) = self.current_char {
                                        if c == '}' {
                                            break;
                                        }
                                        if let Some(d) = c.to_digit(16) {
                                            val = (val << 4) | d;
                                            digits += 1;
                                            self.advance();
                                        } else {
                                            // handle invalid hex digit
                                            return Err(self.syntax_error_span(
                                                start_line,
                                                start_column,
                                                start_byte,
                                                self.byte_pos,
                                                "invalid unicode escape",
                                            ));
                                        }
                                    }

                                    // we must be on the closing '}'
                                    if self.current_char != Some('}') || digits == 0 {
                                        return Err(self.syntax_error_span(
                                            start_line,
                                            start_column,
                                            start_byte,
                                            self.byte_pos,
                                            "invalid unicode escape",
                                        ));
                                    }
                                    self.advance(); // consume '}'

                                    if let Some(ch) = char::from_u32(val) {
                                        content.push(ch);
                                    } else {
                                        return Err(self.syntax_error_span(
                                            start_line,
                                            start_column,
                                            start_byte,
                                            self.byte_pos,
                                            "invalid unicode escape",
                                        ));
                                    }
                                } else {
                                    // not a \u{...} escape, keep it literally
                                    content.push('\\');
                                    content.push('u');
                                }
                            }
                            other => {
                                // unrecognized: keep the backslash + char
                                content.push('\\');
                                content.push(other);
                                self.advance();
                            }
                        }
                    } else {
                        // Trailing backslash before EOF; push it
                        content.push('\\');
                        self.advance();
                        break;
                    }
                }

                '"' => {
                    // Only a terminator if not escaped
                    self.advance(); // consume closing quote
                    closed = true;
                    break;
                }

                _ => {
                    content.push(ch);
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
                "unterminated string",
            ));
        }

        // Single‐char content -> Character, otherwise String
        if content.chars().count() == 1 {
            Ok(TokenType::Character(content.chars().next().unwrap()))
        } else {
            Ok(TokenType::String(content))
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

    pub fn next_token(&mut self) -> WqResult<Token> {
        loop {
            let token_line = self.line;
            let token_column = self.column;
            let token_position = self.position;
            let token_byte_start = match self.current_char {
                Some(ch) => self.byte_pos.saturating_sub(ch.len_utf8()),
                None => self.byte_pos,
            };

            match self.current_char {
                None => {
                    return Ok(Token::new(
                        TokenType::Eof,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some(' ') | Some('\t') | Some('\r') => {
                    self.skip_whitespace();
                    continue;
                }

                Some('\n') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::Newline,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some('/') => {
                    if self.peek() == Some(&'/') {
                        self.advance(); // consume first /
                        let comment = self.read_comment();
                        return Ok(Token::new(
                            comment,
                            token_position,
                            token_line,
                            token_column,
                            token_byte_start,
                            self.byte_pos,
                        ));
                    } else if self.peek() == Some(&'.') {
                        self.advance(); // consume '/'
                        self.advance(); // consume '.'
                        return Ok(Token::new(
                            TokenType::DivideDot,
                            token_position,
                            token_line,
                            token_column,
                            token_byte_start,
                            self.byte_pos,
                        ));
                    } else {
                        self.advance();
                        return Ok(Token::new(
                            TokenType::Divide,
                            token_position,
                            token_line,
                            token_column,
                            token_byte_start,
                            self.byte_pos,
                        ));
                    }
                }

                Some('+') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::Plus,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some('-') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::Minus,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some('*') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::Multiply,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some('^') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::Power,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some('%') => {
                    if self.peek() == Some(&'.') {
                        self.advance();
                        self.advance();
                        return Ok(Token::new(
                            TokenType::ModuloDot,
                            token_position,
                            token_line,
                            token_column,
                            token_byte_start,
                            self.byte_pos,
                        ));
                    } else {
                        self.advance();
                        return Ok(Token::new(
                            TokenType::Modulo,
                            token_position,
                            token_line,
                            token_column,
                            token_byte_start,
                            self.byte_pos,
                        ));
                    }
                }

                Some(':') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::Colon,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some('=') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::Equal,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some('~') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::NotEqual,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some('<') => {
                    if self.peek() == Some(&'=') {
                        self.advance(); // consume '<'
                        self.advance(); // consume '='
                        return Ok(Token::new(
                            TokenType::LessThanOrEqual,
                            token_position,
                            token_line,
                            token_column,
                            token_byte_start,
                            self.byte_pos,
                        ));
                    } else {
                        self.advance();
                        return Ok(Token::new(
                            TokenType::LessThan,
                            token_position,
                            token_line,
                            token_column,
                            token_byte_start,
                            self.byte_pos,
                        ));
                    }
                }

                Some('>') => {
                    if self.peek() == Some(&'=') {
                        self.advance(); // consume '>'
                        self.advance(); // consume '='
                        return Ok(Token::new(
                            TokenType::GreaterThanOrEqual,
                            token_position,
                            token_line,
                            token_column,
                            token_byte_start,
                            self.byte_pos,
                        ));
                    } else {
                        self.advance();
                        return Ok(Token::new(
                            TokenType::GreaterThan,
                            token_position,
                            token_line,
                            token_column,
                            token_byte_start,
                            self.byte_pos,
                        ));
                    }
                }

                Some('$') => {
                    if self.peek() == Some(&'.') {
                        self.advance(); // consume '$'
                        self.advance(); // consume '.'
                        return Ok(Token::new(
                            TokenType::DollarDot,
                            token_position,
                            token_line,
                            token_column,
                            token_byte_start,
                            self.byte_pos,
                        ));
                    } else {
                        self.advance();
                        return Ok(Token::new(
                            TokenType::Dollar,
                            token_position,
                            token_line,
                            token_column,
                            token_byte_start,
                            self.byte_pos,
                        ));
                    }
                }

                Some('@') => {
                    self.advance();
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
                        Some('a') => {
                            self.advance();
                            TokenType::AtAssert
                        }
                        Some('t') => {
                            self.advance();
                            TokenType::AtTry
                        }
                        _ => {
                            // unknown @ sequence - skip
                            continue;
                        }
                    };
                    return Ok(Token::new(
                        tok,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some('#') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::Sharp,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some('|') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::Pipe,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some('(') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::LeftParen,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some(')') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::RightParen,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some('[') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::LeftBracket,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some(']') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::RightBracket,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some('{') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::LeftBrace,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some('}') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::RightBrace,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some(';') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::Semicolon,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some(',') => {
                    self.advance();
                    return Ok(Token::new(
                        TokenType::Comma,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some('`') => {
                    let symbol = self.read_symbol();
                    return Ok(Token::new(
                        symbol,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some('"') => {
                    let string_or_char =
                        self.read_string_or_char(token_line, token_column, token_byte_start)?;
                    return Ok(Token::new(
                        string_or_char,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some(ch) if ch.is_ascii_digit() => {
                    let number = self.read_number(token_line, token_column, token_byte_start)?;
                    return Ok(Token::new(
                        number,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some(ch) if ch.is_alphabetic() || ch == '_' => {
                    let identifier = self.read_identifier();

                    // Check for literals and identifiers
                    let token_type = match identifier.as_str() {
                        "true" => TokenType::True,
                        "false" => TokenType::False,
                        "inf" => TokenType::Inf,
                        "nan" => TokenType::Nan,
                        _ => TokenType::Identifier(identifier),
                    };

                    return Ok(Token::new(
                        token_type,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        self.byte_pos,
                    ));
                }

                Some(_ch) => {
                    // Unknown char. skip
                    self.advance();
                    continue;
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
        assert!(matches!(res, Err(WqError::SyntaxError(_))));
    }

    #[test]
    fn integer_overflow_errors() {
        let mut lexer = Lexer::new("9223372036854775808");
        let res = lexer.tokenize();
        assert!(matches!(res, Err(WqError::SyntaxError(_))));
    }

    #[test]
    fn float_overflow_errors() {
        let big = "1".repeat(400) + ".0";
        let mut lexer = Lexer::new(&big);
        let res = lexer.tokenize();
        assert!(matches!(res, Err(WqError::SyntaxError(_))));
    }

    #[test]
    fn at_try_token() {
        let mut lexer = Lexer::new("@t 1");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::AtTry);
    }
}
