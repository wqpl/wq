use std::fmt;
use std::iter::Peekable;
use std::str::Chars;

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
    NaN,

    True,
    False,

    Comment(String),
    AtBreak,
    AtContinue,
    AtReturn,
    AtAssert,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub token_type: TokenType,
    pub position: usize,
    pub line: usize,
    pub column: usize,
}

impl Token {
    pub fn new(token_type: TokenType, position: usize, line: usize, column: usize) -> Self {
        Token {
            token_type,
            position,
            line,
            column,
        }
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}@{}:{}", self.token_type, self.line, self.column)
    }
}

pub struct Lexer<'a> {
    input: Peekable<Chars<'a>>,
    position: usize,
    line: usize,
    column: usize,
    current_char: Option<char>,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        let mut lexer = Lexer {
            input: input.chars().peekable(),
            position: 0,
            line: 1,
            column: 0,
            current_char: None,
        };
        lexer.advance();
        lexer
    }

    fn advance(&mut self) {
        self.current_char = self.input.next();
        if let Some(ch) = self.current_char {
            self.position += 1;
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

    fn read_number(&mut self) -> TokenType {
        let _start_pos = self.position;
        let mut number_str = String::new();
        let mut is_float = false;

        while let Some(ch) = self.current_char {
            if ch.is_ascii_digit() {
                number_str.push(ch);
                self.advance();
            } else if ch == '.' && !is_float {
                // Check if next char is a digit
                if let Some(next_ch) = self.peek() {
                    if next_ch.is_ascii_digit() {
                        is_float = true;
                        number_str.push(ch);
                        self.advance();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        if is_float {
            TokenType::Float(number_str.parse().unwrap_or(0.0))
        } else {
            TokenType::Integer(number_str.parse().unwrap_or(0))
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

    fn read_string_or_char(&mut self) -> TokenType {
        self.advance(); // skip opening quote
        let mut content = String::new();

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
                    break;
                }

                _ => {
                    content.push(ch);
                    self.advance();
                }
            }
        }

        // Single‐char content -> Character, otherwise String
        if content.chars().count() == 1 {
            TokenType::Character(content.chars().next().unwrap())
        } else {
            TokenType::String(content)
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

    pub fn next_token(&mut self) -> Token {
        loop {
            let token_line = self.line;
            let token_column = self.column;
            let token_position = self.position;

            match self.current_char {
                None => {
                    return Token::new(TokenType::Eof, token_position, token_line, token_column);
                }

                Some(' ') | Some('\t') | Some('\r') => {
                    self.skip_whitespace();
                    continue;
                }

                Some('\n') => {
                    self.advance();
                    return Token::new(
                        TokenType::Newline,
                        token_position,
                        token_line,
                        token_column,
                    );
                }

                Some('/') => {
                    if self.peek() == Some(&'/') {
                        self.advance(); // consume first /
                        let comment = self.read_comment();
                        return Token::new(comment, token_position, token_line, token_column);
                    } else if self.peek() == Some(&'.') {
                        self.advance(); // consume '/'
                        self.advance(); // consume '.'
                        return Token::new(TokenType::DivideDot, token_position, token_line, token_column);
                    } else {
                        self.advance();
                        return Token::new(
                            TokenType::Divide,
                            token_position,
                            token_line,
                            token_column,
                        );
                    }
                }

                Some('+') => {
                    self.advance();
                    return Token::new(TokenType::Plus, token_position, token_line, token_column);
                }

                Some('-') => {
                    self.advance();
                    return Token::new(TokenType::Minus, token_position, token_line, token_column);
                }

                Some('*') => {
                    self.advance();
                    return Token::new(
                        TokenType::Multiply,
                        token_position,
                        token_line,
                        token_column,
                    );
                }

                Some('%') => {
                    if self.peek() == Some(&'.') {
                        self.advance();
                        self.advance();
                        return Token::new(TokenType::ModuloDot, token_position, token_line, token_column);
                    } else {
                        self.advance();
                        return Token::new(TokenType::Modulo, token_position, token_line, token_column);
                    }
                }

                Some(':') => {
                    self.advance();
                    return Token::new(TokenType::Colon, token_position, token_line, token_column);
                }

                Some('=') => {
                    self.advance();
                    return Token::new(TokenType::Equal, token_position, token_line, token_column);
                }

                Some('~') => {
                    self.advance();
                    return Token::new(
                        TokenType::NotEqual,
                        token_position,
                        token_line,
                        token_column,
                    );
                }

                Some('<') => {
                    if self.peek() == Some(&'=') {
                        self.advance(); // consume '<'
                        self.advance(); // consume '='
                        return Token::new(
                            TokenType::LessThanOrEqual,
                            token_position,
                            token_line,
                            token_column,
                        );
                    } else {
                        self.advance();
                        return Token::new(
                            TokenType::LessThan,
                            token_position,
                            token_line,
                            token_column,
                        );
                    }
                }

                Some('>') => {
                    if self.peek() == Some(&'=') {
                        self.advance(); // consume '>'
                        self.advance(); // consume '='
                        return Token::new(
                            TokenType::GreaterThanOrEqual,
                            token_position,
                            token_line,
                            token_column,
                        );
                    } else {
                        self.advance();
                        return Token::new(
                            TokenType::GreaterThan,
                            token_position,
                            token_line,
                            token_column,
                        );
                    }
                }

                Some('$') => {
                    if self.peek() == Some(&'.') {
                        self.advance(); // consume '$'
                        self.advance(); // consume '.'
                        return Token::new(
                            TokenType::DollarDot,
                            token_position,
                            token_line,
                            token_column,
                        );
                    } else {
                        self.advance();
                        return Token::new(
                            TokenType::Dollar,
                            token_position,
                            token_line,
                            token_column,
                        );
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
                        _ => {
                            // unknown @ sequence - skip
                            continue;
                        }
                    };
                    return Token::new(tok, token_position, token_line, token_column);
                }

                Some('#') => {
                    self.advance();
                    return Token::new(TokenType::Sharp, token_position, token_line, token_column);
                }

                Some('(') => {
                    self.advance();
                    return Token::new(
                        TokenType::LeftParen,
                        token_position,
                        token_line,
                        token_column,
                    );
                }

                Some(')') => {
                    self.advance();
                    return Token::new(
                        TokenType::RightParen,
                        token_position,
                        token_line,
                        token_column,
                    );
                }

                Some('[') => {
                    self.advance();
                    return Token::new(
                        TokenType::LeftBracket,
                        token_position,
                        token_line,
                        token_column,
                    );
                }

                Some(']') => {
                    self.advance();
                    return Token::new(
                        TokenType::RightBracket,
                        token_position,
                        token_line,
                        token_column,
                    );
                }

                Some('{') => {
                    self.advance();
                    return Token::new(
                        TokenType::LeftBrace,
                        token_position,
                        token_line,
                        token_column,
                    );
                }

                Some('}') => {
                    self.advance();
                    return Token::new(
                        TokenType::RightBrace,
                        token_position,
                        token_line,
                        token_column,
                    );
                }

                Some(';') => {
                    self.advance();
                    return Token::new(
                        TokenType::Semicolon,
                        token_position,
                        token_line,
                        token_column,
                    );
                }

                Some(',') => {
                    self.advance();
                    return Token::new(TokenType::Comma, token_position, token_line, token_column);
                }

                Some('`') => {
                    let symbol = self.read_symbol();
                    return Token::new(symbol, token_position, token_line, token_column);
                }

                Some('"') => {
                    let string_or_char = self.read_string_or_char();
                    return Token::new(string_or_char, token_position, token_line, token_column);
                }

                Some(ch) if ch.is_ascii_digit() => {
                    let number = self.read_number();
                    return Token::new(number, token_position, token_line, token_column);
                }

                Some(ch) if ch.is_alphabetic() || ch == '_' => {
                    let identifier = self.read_identifier();

                    // Check for literals and identifiers
                    let token_type = match identifier.as_str() {
                        "true" => TokenType::True,
                        "false" => TokenType::False,
                        "Inf" | "inf" => TokenType::Inf,
                        "NaN" | "nan" => TokenType::NaN,
                        _ => TokenType::Identifier(identifier),
                    };

                    return Token::new(token_type, token_position, token_line, token_column);
                }

                Some(_ch) => {
                    // Unknown char. skip
                    self.advance();
                    continue;
                }
            }
        }
    }

    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();

        loop {
            let token = self.next_token();
            let is_eof = token.token_type == TokenType::Eof;
            tokens.push(token);
            if is_eof {
                break;
            }
        }

        tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_numbers() {
        let mut lexer = Lexer::new("42 3.1 -5");
        let tokens = lexer.tokenize();

        assert_eq!(tokens[0].token_type, TokenType::Integer(42));
        assert_eq!(tokens[1].token_type, TokenType::Float(3.1));
        assert_eq!(tokens[2].token_type, TokenType::Minus);
        assert_eq!(tokens[3].token_type, TokenType::Integer(5));
    }

    #[test]
    fn test_tokenize_operators() {
        let mut lexer = Lexer::new("+ - * / %");
        let tokens = lexer.tokenize();

        assert_eq!(tokens[0].token_type, TokenType::Plus);
        assert_eq!(tokens[1].token_type, TokenType::Minus);
        assert_eq!(tokens[2].token_type, TokenType::Multiply);
        assert_eq!(tokens[3].token_type, TokenType::Divide);
        assert_eq!(tokens[4].token_type, TokenType::Modulo);
    }

    #[test]
    fn test_tokenize_dot_operators() {
        let mut lexer = Lexer::new("/. %.");
        let tokens = lexer.tokenize();
        assert_eq!(tokens[0].token_type, TokenType::DivideDot);
        assert_eq!(tokens[1].token_type, TokenType::ModuloDot);
    }

    #[test]
    fn test_tokenize_inf_nan() {
        let mut lexer = Lexer::new("Inf NaN");
        let tokens = lexer.tokenize();
        assert_eq!(tokens[0].token_type, TokenType::Inf);
        assert_eq!(tokens[1].token_type, TokenType::NaN);
    }

    #[test]
    fn test_tokenize_symbols() {
        let mut lexer = Lexer::new("`hello `world");
        let tokens = lexer.tokenize();

        assert_eq!(tokens[0].token_type, TokenType::Symbol("hello".to_string()));
        assert_eq!(tokens[1].token_type, TokenType::Symbol("world".to_string()));
    }

    #[test]
    fn test_tokenize_string() {
        let mut lexer = Lexer::new("\"ab\"");
        let tokens = lexer.tokenize();
        assert_eq!(tokens[0].token_type, TokenType::String("ab".to_string()));
    }

    #[test]
    fn test_escape_quote() {
        let mut lexer = Lexer::new("\"a\\\"b\"");
        let tokens = lexer.tokenize();
        assert_eq!(tokens[0].token_type, TokenType::String("a\"b".to_string()));
    }

    #[test]
    fn test_tokenize_char() {
        let mut lexer = Lexer::new("\"a\"");
        let tokens = lexer.tokenize();
        assert_eq!(tokens[0].token_type, TokenType::Character('a'));
    }

    #[test]
    fn test_tokenize_expression() {
        let mut lexer = Lexer::new("x:1+2*3");
        let tokens = lexer.tokenize();

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
        let tokens = lexer.tokenize();
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
}
