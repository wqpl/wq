use std::iter::Peekable;
use std::str::Chars;

use num_bigint::BigInt;
use num_traits::{Num, ToPrimitive};

use crate::token::{Keyword, Token, TokenType};
use crate::value::WqResult;
use crate::wqerror::{WqError, WqErrorType};

pub(crate) struct Lexer<'a> {
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
    // Byte position of the start of the current line.
    line_start_byte_pos: usize,
    // Optional global source context for better error spans
    global_source: Option<&'a str>,
    base_offset: usize,
    // Optional source file path / label for error rendering
    source_path: Option<String>,
    // When true, unterminated strings/comments emit a partial token instead
    // of an error so that syntax highlighting can continue to the end.
    recovery_mode: bool,
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(input: &'a str) -> Self {
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
            line_start_byte_pos: 0,
            global_source: None,
            base_offset: 0,
            source_path: None,
            recovery_mode: false,
        };
        lexer.advance();
        lexer
    }

    /// Provide a global source context and base byte offset for more accurate
    /// error spans - When lexing a snippet within a larger file
    pub(crate) fn with_ctx(mut self, global_source: &'a str, base_offset: usize) -> Self {
        self.global_source = Some(global_source);
        self.base_offset = base_offset;
        self
    }

    pub(crate) fn set_source_path(&mut self, path: String) {
        self.source_path = Some(path);
    }

    fn build_error(
        &self,
        err_type: WqErrorType,
        _line: usize,
        _column: usize,
        byte_start: usize,
        byte_end: usize,
        msg: impl std::fmt::Display,
    ) -> WqError {
        let src = self.source;
        let bs = byte_start.min(src.len());
        let be = Self::non_empty_span_end(src, bs, byte_end);
        let (text, abs_start, abs_end) = if let Some(gs) = self.global_source {
            let start = bs + self.base_offset;
            let end = be + self.base_offset;
            (gs.to_string(), start, end)
        } else {
            (src.to_string(), bs, be)
        };
        let path = self.source_path.as_deref().unwrap_or("?");
        WqError::new(err_type)
            .src("lexer")
            .msg(msg)
            .span(Some((abs_start, abs_end)))
            .source_ctx(text, path)
    }

    fn non_empty_span_end(src: &str, byte_start: usize, byte_end: usize) -> usize {
        let end = byte_end.min(src.len());
        if end > byte_start {
            return end;
        }
        src[byte_start..]
            .chars()
            .next()
            .map_or(end, |ch| byte_start + ch.len_utf8())
    }

    fn syntax_error_span(
        &self,
        line: usize,
        column: usize,
        byte_start: usize,
        byte_end: usize,
        msg: impl std::fmt::Display,
    ) -> WqError {
        self.build_error(WqErrorType::Syntax, line, column, byte_start, byte_end, msg)
    }

    fn eof_error_span(
        &self,
        line: usize,
        column: usize,
        byte_start: usize,
        byte_end: usize,
        msg: impl std::fmt::Display,
    ) -> WqError {
        self.build_error(WqErrorType::Eof, line, column, byte_start, byte_end, msg)
    }

    fn advance(&mut self) {
        // Consume the previous current_char, if any
        if let Some(prev) = self.current_char {
            self.byte_pos += prev.len_utf8();
            if prev == '\n' {
                self.line += 1;
                self.column = 0;
                self.line_start_byte_pos = self.byte_pos;
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
        let finish_imaginary = |lexer: &mut Self, value: f64| {
            if lexer.current_char == Some('i') {
                lexer.advance();
                TokenType::Imaginary(value)
            } else {
                TokenType::Float(value)
            }
        };
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
                    if self.current_char == Some('i') {
                        self.advance();
                        return Ok(TokenType::Imaginary(n as f64));
                    }
                    return Ok(TokenType::Integer(n));
                }
                match BigInt::from_str_radix(&lit, base) {
                    Ok(big) => {
                        if self.current_char == Some('i') {
                            let value =
                                big.to_f64().filter(|n| n.is_finite()).ok_or_else(|| {
                                    self.syntax_error_span(
                                        start_line,
                                        start_column,
                                        start_byte,
                                        self.byte_pos,
                                        "imaginary literal overflow",
                                    )
                                })?;
                            self.advance();
                            return Ok(TokenType::Imaginary(value));
                        }
                        return Ok(TokenType::BigInteger(big));
                    }
                    Err(_) => {
                        return Err(self.syntax_error_span(
                            start_line,
                            start_column,
                            start_byte,
                            self.byte_pos,
                            "invalid int literal",
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
                if let Some(next_ch) = self.peek()
                    && next_ch.is_ascii_digit()
                {
                    is_float = true;
                    raw_lit.push(ch);
                    self.advance();
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
                Ok(n) => Ok(finish_imaginary(self, n)),
                _ => Err(self.syntax_error_span(
                    start_line,
                    start_column,
                    start_byte,
                    self.byte_pos,
                    "invalid float literal",
                )),
            }
        } else {
            match lit.parse::<i64>() {
                Ok(n) => {
                    if self.current_char == Some('i') {
                        self.advance();
                        Ok(TokenType::Imaginary(n as f64))
                    } else {
                        Ok(TokenType::Integer(n))
                    }
                }
                Err(_) => match lit.parse::<BigInt>() {
                    Ok(big) => {
                        if self.current_char == Some('i') {
                            let value =
                                big.to_f64().filter(|n| n.is_finite()).ok_or_else(|| {
                                    self.syntax_error_span(
                                        start_line,
                                        start_column,
                                        start_byte,
                                        self.byte_pos,
                                        "imaginary literal overflow",
                                    )
                                })?;
                            self.advance();
                            Ok(TokenType::Imaginary(value))
                        } else {
                            Ok(TokenType::BigInteger(big))
                        }
                    }
                    Err(_) => Err(self.syntax_error_span(
                        start_line,
                        start_column,
                        start_byte,
                        self.byte_pos,
                        "invalid int literal",
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

    fn read_tag(&mut self) -> TokenType {
        self.advance(); // consume the backtick
        let tag_name = self.read_identifier();
        if tag_name.is_empty() {
            TokenType::Backtick
        } else {
            TokenType::Tag(tag_name)
        }
    }

    fn read_string_literal(
        &mut self,
        start_line: usize,
        start_column: usize,
        start_byte: usize,
    ) -> WqResult<TokenType> {
        // Count consecutive opening quotes
        let mut quote_count = 0;
        while self.current_char == Some('"') {
            quote_count += 1;
            self.advance();
        }

        match quote_count {
            2 => Ok(TokenType::String("".to_string())),
            1 => {
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
                    if self.recovery_mode {
                        return Ok(TokenType::String(raw));
                    }
                    return Err(self.eof_error_span(
                        start_line,
                        start_column,
                        start_byte,
                        self.byte_pos,
                        "string is not properly terminated",
                    ));
                }

                self.decode_string_content(&raw, start_line, start_column, content_start_byte)
                    .map(TokenType::String)
            }
            n if n >= 3 => {
                let content_start_byte = self.byte_pos;
                let mut raw = String::new();
                let mut closed = false;
                let mut consecutive_quotes = 0;

                while let Some(ch) = self.current_char {
                    if ch == '"' {
                        consecutive_quotes += 1;
                        self.advance();
                        if consecutive_quotes == quote_count {
                            closed = true;
                            break;
                        }
                    } else {
                        for _ in 0..consecutive_quotes {
                            raw.push('"');
                        }
                        consecutive_quotes = 0;

                        if ch == '\\' {
                            raw.push('\\');
                            self.advance();
                            if let Some(next) = self.current_char {
                                raw.push(next);
                                self.advance();
                            } else {
                                break;
                            }
                        } else {
                            raw.push(ch);
                            self.advance();
                        }
                    }
                }

                if !closed {
                    if self.recovery_mode {
                        for _ in 0..consecutive_quotes {
                            raw.push('"');
                        }
                        return Ok(TokenType::String(raw));
                    }
                    return Err(self.eof_error_span(
                        start_line,
                        start_column,
                        start_byte,
                        self.byte_pos,
                        "string is not properly terminated",
                    ));
                }

                self.decode_string_content(&raw, start_line, start_column, content_start_byte)
                    .map(TokenType::String)
            }
            _ => unreachable!(),
        }
    }

    fn decode_string_content(
        &self,
        raw: &str,
        start_line: usize,
        start_column: usize,
        content_start_byte: usize,
    ) -> WqResult<String> {
        crate::escape::unescape_string_inner(raw).map_err(|err| {
            use crate::escape::UnescapeErrorKind::*;
            let msg = match err.kind {
                InvalidHexEscape => "invalid hex escape",
                InvalidUnicodeEscape | InvalidUnicodeScalar => "invalid unicode escape",
            };
            let err_byte_start = content_start_byte.saturating_add(err.index);
            self.syntax_error_span(
                start_line,
                start_column,
                err_byte_start,
                err_byte_start + 1,
                msg,
            )
        })
    }

    fn advance_string_escape(&mut self) {
        let start = self.byte_pos;
        let remaining = &self.source[start..];
        let len = crate::escape::valid_escape_sequence_len(remaining)
            .unwrap_or_else(|| remaining.chars().take(2).map(char::len_utf8).sum::<usize>());
        let end = start.saturating_add(len);
        while self.byte_pos < end && self.current_char.is_some() {
            self.advance();
        }
    }

    fn read_unicode_scalar_literal(
        &mut self,
        start_line: usize,
        start_column: usize,
        start_byte: usize,
    ) -> WqResult<TokenType> {
        self.skip_whitespace();
        if self.current_char == Some('{') {
            return self.read_unicode_scalar_shorthand(start_line, start_column, start_byte);
        }
        if self.current_char != Some('"') {
            return Err(self.syntax_error_span(
                start_line,
                start_column,
                start_byte,
                self.byte_pos,
                "expected quoted Unicode scalar or hexadecimal shorthand after @u",
            ));
        }

        let TokenType::String(content) =
            self.read_string_literal(start_line, start_column, start_byte)?
        else {
            unreachable!("quoted literal reader always returns a string")
        };
        let mut chars = content.chars();
        let Some(value) = chars.next() else {
            return Err(self.syntax_error_span(
                start_line,
                start_column,
                start_byte,
                self.byte_pos,
                "Unicode scalar literal must contain exactly one scalar",
            ));
        };
        if chars.next().is_some() {
            return Err(self.syntax_error_span(
                start_line,
                start_column,
                start_byte,
                self.byte_pos,
                "Unicode scalar literal must contain exactly one scalar",
            ));
        }
        Ok(TokenType::Character(value))
    }

    fn read_unicode_scalar_shorthand(
        &mut self,
        start_line: usize,
        start_column: usize,
        start_byte: usize,
    ) -> WqResult<TokenType> {
        debug_assert_eq!(self.current_char, Some('{'));
        self.advance();

        let mut digits = String::new();
        let mut closed = false;
        while let Some(ch) = self.current_char {
            if ch == '}' {
                self.advance();
                closed = true;
                break;
            }
            digits.push(ch);
            self.advance();
        }

        if !closed && !self.recovery_mode {
            return Err(self.eof_error_span(
                start_line,
                start_column,
                start_byte,
                self.byte_pos,
                "Unicode scalar shorthand is not properly terminated",
            ));
        }

        if !(1..=6).contains(&digits.len()) || !digits.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(self.syntax_error_span(
                start_line,
                start_column,
                start_byte,
                self.byte_pos,
                "Unicode scalar shorthand must contain 1 to 6 hexadecimal digits",
            ));
        }

        let value = u32::from_str_radix(&digits, 16)
            .expect("validated hexadecimal Unicode scalar shorthand");
        let Some(value) = char::from_u32(value) else {
            return Err(self.syntax_error_span(
                start_line,
                start_column,
                start_byte,
                self.byte_pos,
                "invalid Unicode scalar shorthand value",
            ));
        };

        Ok(TokenType::Character(value))
    }

    fn read_inline_comment(
        &mut self,
        start_line: usize,
        start_column: usize,
        start_byte: usize,
    ) -> WqResult<TokenType> {
        let mut comment = String::new();
        let mut depth = 1;

        while depth > 0 {
            match self.current_char {
                Some('/') if self.peek() == Some('*') => {
                    comment.push('/');
                    comment.push('*');
                    self.advance();
                    self.advance();
                    depth += 1;
                }
                Some('*') if self.peek() == Some('/') => {
                    self.advance();
                    self.advance();
                    depth -= 1;
                    if depth > 0 {
                        comment.push('*');
                        comment.push('/');
                    }
                }
                Some(ch) => {
                    comment.push(ch);
                    self.advance();
                }
                None => break,
            }
        }
        if depth != 0 {
            if self.recovery_mode {
                return Ok(TokenType::Comment(comment));
            }
            return Err(self.eof_error_span(
                start_line,
                start_column,
                start_byte,
                self.byte_pos,
                "expected closing '*/'",
            ));
        }
        Ok(TokenType::Comment(comment))
    }

    fn read_line_comment(&mut self) -> TokenType {
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
            if self.recovery_mode {
                return Ok(TokenType::String(raw));
            }
            return Err(self.eof_error_span(
                start_line,
                start_column,
                start_byte,
                self.byte_pos,
                "string is not properly terminated",
            ));
        }
        Ok(TokenType::String(raw))
    }

    /// Read a format string (`@f"..."`). The current_char must be '"'.
    /// Produces `TokenType::FormatString` with segmented parts so the
    /// highlighter can recursively highlight expressions inside braces.
    fn read_format_string(
        &mut self,
        start_line: usize,
        start_column: usize,
        start_byte: usize,
    ) -> WqResult<TokenType> {
        use crate::token::FmtPart;

        let mut parts = Vec::new();
        let mut current_text_start: Option<usize> = None;

        let open_quote = self.byte_pos; // position of the opening quote
        self.advance(); // consume opening quote

        while let Some(ch) = self.current_char {
            match ch {
                '"' => {
                    self.advance();
                    let close_quote = self.byte_pos.saturating_sub(1); // position of closing quote
                    if let Some(text_start) = current_text_start.take() {
                        let text_end = close_quote; // exclude closing quote
                        let content = self.source[text_start..text_end].to_string();
                        self.decode_string_content(&content, start_line, start_column, text_start)?;
                        parts.push(FmtPart::Text {
                            content,
                            start: text_start,
                            end: text_end,
                        });
                    }
                    return Ok(TokenType::FormatString(parts, open_quote, close_quote));
                }
                '\\' => {
                    if current_text_start.is_none() {
                        current_text_start = Some(self.byte_pos);
                    }
                    self.advance_string_escape();
                }
                '{' => {
                    if self.peek() == Some('{') {
                        // literal '{{'
                        if current_text_start.is_none() {
                            current_text_start = Some(self.byte_pos);
                        }
                        self.advance();
                        self.advance();
                    } else {
                        // start of expression
                        if let Some(text_start) = current_text_start.take() {
                            let text_end = self.byte_pos;
                            let content = self.source[text_start..text_end].to_string();
                            self.decode_string_content(
                                &content,
                                start_line,
                                start_column,
                                text_start,
                            )?;
                            parts.push(FmtPart::Text {
                                content,
                                start: text_start,
                                end: text_end,
                            });
                        }

                        let expr_start = self.byte_pos;
                        self.advance(); // skip '{'
                        let mut depth = 1usize;
                        let mut in_str = false;

                        while let Some(c) = self.current_char {
                            if in_str {
                                if c == '\\' {
                                    self.advance();
                                    if self.current_char.is_some() {
                                        self.advance();
                                    }
                                } else if c == '"' {
                                    in_str = false;
                                    self.advance();
                                } else {
                                    self.advance();
                                }
                            } else {
                                match c {
                                    '"' => {
                                        in_str = true;
                                        self.advance();
                                    }
                                    '{' => {
                                        depth += 1;
                                        self.advance();
                                    }
                                    '}' => {
                                        depth -= 1;
                                        if depth == 0 {
                                            self.advance();
                                            break;
                                        } else {
                                            self.advance();
                                        }
                                    }
                                    _ => self.advance(),
                                }
                            }
                        }

                        if depth != 0 {
                            return Err(self.eof_error_span(
                                start_line,
                                start_column,
                                expr_start,
                                self.byte_pos,
                                "unterminated expression in format string",
                            ));
                        }

                        let expr_end = self.byte_pos;
                        let source = self.source[expr_start..expr_end].to_string();
                        parts.push(FmtPart::Expr {
                            source,
                            start: expr_start,
                            end: expr_end,
                        });
                    }
                }
                '}' => {
                    if self.peek() == Some('}') {
                        // literal '}}'
                        if current_text_start.is_none() {
                            current_text_start = Some(self.byte_pos);
                        }
                        self.advance();
                        self.advance();
                    } else {
                        return Err(self.syntax_error_span(
                            start_line,
                            start_column,
                            self.byte_pos,
                            self.byte_pos + ch.len_utf8(),
                            "unmatched '}' in format string",
                        ));
                    }
                }
                _ => {
                    if current_text_start.is_none() {
                        current_text_start = Some(self.byte_pos);
                    }
                    self.advance();
                }
            }
        }

        // Unterminated format string
        if self.recovery_mode {
            if let Some(text_start) = current_text_start.take() {
                let text_end = self.byte_pos;
                let content = self.source[text_start..text_end].to_string();
                self.decode_string_content(&content, start_line, start_column, text_start)?;
                parts.push(FmtPart::Text {
                    content,
                    start: text_start,
                    end: text_end,
                });
            }
            return Ok(TokenType::FormatString(parts, open_quote, self.byte_pos));
        }
        Err(self.eof_error_span(
            start_line,
            start_column,
            start_byte,
            self.byte_pos,
            "format string is not properly terminated",
        ))
    }

    pub(crate) fn next_token(&mut self) -> WqResult<Token> {
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
                    Some(':') => {
                        self.advance();
                        self.advance();
                        return emit(TokenType::MultiplyColon, self.byte_pos);
                    }
                    _ => {
                        self.advance();
                        return emit(TokenType::Multiply, self.byte_pos);
                    }
                },

                Some('/') => {
                    match nxt {
                        Some('/') => {
                            self.advance(); // first '/'
                            self.advance(); // second '/'
                            let comment = self.read_line_comment();
                            return emit(comment, self.byte_pos);
                        }
                        Some('*') => {
                            self.advance(); // '/'
                            self.advance(); // '*'
                            let comment = self.read_inline_comment(
                                token_line,
                                token_column,
                                token_byte_start,
                            )?;
                            return emit(comment, self.byte_pos);
                        }
                        Some('%') => {
                            let n2 = self.peek2();
                            if n2 == Some(':') {
                                self.advance();
                                self.advance();
                                self.advance();
                                return emit(TokenType::FloorDivColon, self.byte_pos);
                            } else {
                                self.advance();
                                self.advance();
                                return emit(TokenType::FloorDiv, self.byte_pos);
                            }
                        }
                        Some('.') => {
                            let n2 = self.peek2();
                            if n2 == Some(':') {
                                self.advance();
                                self.advance();
                                self.advance();
                                return emit(TokenType::DivideDotColon, self.byte_pos);
                            } else {
                                self.advance();
                                self.advance(); // '/.'
                                return emit(TokenType::DivideDot, self.byte_pos);
                            }
                        }
                        Some(':') => {
                            self.advance();
                            self.advance();
                            return emit(TokenType::DivideColon, self.byte_pos);
                        }
                        _ => {
                            self.advance();
                            return emit(TokenType::Divide, self.byte_pos);
                        }
                    }
                }

                Some('%') => match nxt {
                    Some(':') => {
                        self.advance();
                        self.advance();
                        return emit(TokenType::ModuloColon, self.byte_pos);
                    }
                    _ => {
                        self.advance();
                        return emit(TokenType::Modulo, self.byte_pos);
                    }
                },

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
                            let err = self.syntax_error_span(
                                token_line,
                                token_column,
                                token_byte_start,
                                self.byte_pos,
                                "unrecognized character",
                            );
                            self.advance();
                            return Err(err);
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
                        Some('d') => {
                            self.advance();
                            TokenType::AtDebug
                        }
                        Some('p') => {
                            self.advance();
                            TokenType::AtPause
                        }
                        Some(ch) if ch.is_ascii_digit() => {
                            let mut raw_lit = String::new();
                            while let Some(ch) = self.current_char {
                                if ch.is_ascii_digit() {
                                    raw_lit.push(ch);
                                    self.advance();
                                } else if ch == '_' {
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                            let depth = raw_lit.parse::<i64>().map_err(|_| {
                                self.syntax_error_span(
                                    token_line,
                                    token_column,
                                    token_byte_start,
                                    self.byte_pos,
                                    "depth modifier is too large",
                                )
                            })?;
                            TokenType::AtDepth(depth)
                        }
                        Some('-') if self.peek().is_some_and(|ch| ch.is_ascii_digit()) => {
                            let mut raw_lit = String::from("-");
                            self.advance();
                            while let Some(ch) = self.current_char {
                                if ch.is_ascii_digit() {
                                    raw_lit.push(ch);
                                    self.advance();
                                } else if ch == '_' {
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                            let depth = raw_lit.parse::<i64>().map_err(|_| {
                                self.syntax_error_span(
                                    token_line,
                                    token_column,
                                    token_byte_start,
                                    self.byte_pos,
                                    "depth modifier is too large",
                                )
                            })?;
                            TokenType::AtDepth(depth)
                        }
                        Some('t') => {
                            self.advance();
                            TokenType::AtTry
                        }
                        Some('i') => {
                            self.advance();
                            TokenType::AtImport
                        }
                        Some('f') => {
                            self.advance();
                            self.skip_whitespace();
                            if self.current_char == Some('"') {
                                let t = self.read_format_string(
                                    token_line,
                                    token_column,
                                    token_byte_start,
                                )?;
                                return emit(t, self.byte_pos);
                            }
                            return Err(self.syntax_error_span(
                                token_line,
                                token_column,
                                token_byte_start,
                                self.byte_pos,
                                "expected string after @f",
                            ));
                        }
                        Some('s') => {
                            self.advance();
                            TokenType::AtSymbolic
                        }
                        // @l followed by a quoted string is a raw string; lex it now
                        Some('l') => {
                            self.advance();
                            // Expect a '"' and then consume as raw string
                            let t =
                                self.read_raw_string(token_line, token_column, token_byte_start)?;
                            return emit(t, self.byte_pos);
                        }
                        Some('u') => {
                            self.advance();
                            let t = self.read_unicode_scalar_literal(
                                token_line,
                                token_column,
                                token_byte_start,
                            )?;
                            return emit(t, self.byte_pos);
                        }
                        unknown => {
                            if unknown.is_some() {
                                self.advance();
                            }
                            let spelling = self
                                .source
                                .get(token_byte_start..self.byte_pos)
                                .unwrap_or("@");
                            return Err(self.syntax_error_span(
                                token_line,
                                token_column,
                                token_byte_start,
                                self.byte_pos,
                                format!(
                                    "unknown '@' form {}",
                                    crate::escape::quote_string(spelling, '\'')
                                ),
                            ));
                        }
                    };
                    return emit(tok, self.byte_pos);
                }

                // Symbols that are always a single token
                Some('+') => {
                    if nxt == Some(':') {
                        self.advance();
                        self.advance();
                        return emit(TokenType::PlusColon, self.byte_pos);
                    } else {
                        self.advance();
                        return emit(TokenType::Plus, self.byte_pos);
                    }
                }
                Some('-') => {
                    if nxt == Some(':') {
                        self.advance();
                        self.advance();
                        return emit(TokenType::MinusColon, self.byte_pos);
                    } else {
                        self.advance();
                        return emit(TokenType::Minus, self.byte_pos);
                    }
                }

                Some('^') => {
                    if nxt == Some('.') {
                        let n2 = self.peek2();
                        if n2 == Some(':') {
                            self.advance();
                            self.advance();
                            self.advance();
                            return emit(TokenType::PowerDotColon, self.byte_pos);
                        } else {
                            self.advance();
                            self.advance();
                            return emit(TokenType::PowerDot, self.byte_pos);
                        }
                    } else if nxt == Some(':') {
                        self.advance();
                        self.advance();
                        return emit(TokenType::PowerColon, self.byte_pos);
                    } else {
                        self.advance();
                        return emit(TokenType::Power, self.byte_pos);
                    }
                }
                Some(':') => {
                    self.advance();
                    return emit(TokenType::Colon, self.byte_pos);
                }
                Some('=') => {
                    if nxt == Some('.') {
                        self.advance();
                        self.advance();
                        return emit(TokenType::EqualDot, self.byte_pos);
                    } else {
                        self.advance();
                        return emit(TokenType::Equal, self.byte_pos);
                    }
                }
                Some('~') => {
                    if nxt == Some('.') {
                        self.advance();
                        self.advance();
                        return emit(TokenType::NotEqualDot, self.byte_pos);
                    } else {
                        self.advance();
                        return emit(TokenType::NotEqual, self.byte_pos);
                    }
                }
                Some('#') => {
                    self.advance();
                    return emit(TokenType::Sharp, self.byte_pos);
                }
                Some('!') => {
                    self.advance();
                    return emit(TokenType::Bang, self.byte_pos);
                }
                Some('|') => {
                    let n2 = self.peek2();
                    match (nxt, n2) {
                        (Some('|'), Some('.')) => {
                            self.advance();
                            self.advance();
                            self.advance();
                            return emit(TokenType::PipePipeDot, self.byte_pos);
                        }
                        (Some('|'), _) => {
                            self.advance();
                            self.advance();
                            return emit(TokenType::PipePipe, self.byte_pos);
                        }
                        (Some('.'), _) => {
                            self.advance();
                            self.advance();
                            return emit(TokenType::PipeDot, self.byte_pos);
                        }
                        _ => {
                            self.advance();
                            return emit(TokenType::Pipe, self.byte_pos);
                        }
                    }
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
                    if nxt == Some(':') {
                        self.advance();
                        self.advance();
                        return emit(TokenType::CommaColon, self.byte_pos);
                    } else {
                        self.advance();
                        return emit(TokenType::Comma, self.byte_pos);
                    }
                }
                Some('\'') => {
                    self.advance();
                    return emit(TokenType::Apostrophe, self.byte_pos);
                }

                // Backtick-quoted symbol
                Some('`') => {
                    let symbol = self.read_tag();
                    return emit(symbol, self.byte_pos);
                }

                // Strings
                Some('"') => {
                    let t = self.read_string_literal(token_line, token_column, token_byte_start)?;
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
                        "true" | "T" => TokenType::True,
                        "false" | "F" => TokenType::False,
                        "inf" => TokenType::Inf,
                        _ => Keyword::from_identifier(&ident)
                            .map_or(TokenType::Identifier(ident), TokenType::Keyword),
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

    pub(crate) fn tokenize(&mut self) -> WqResult<Vec<Token>> {
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

    /// Tokenize the entire input, emitting `TokenType::Error` tokens instead of
    /// stopping on the first lexing error. This is useful for syntax
    /// highlighting where we want to keep going to the end of the file.
    pub(crate) fn tokenize_recovery(&mut self) -> Vec<Token> {
        let prev = self.recovery_mode;
        self.recovery_mode = true;
        let mut tokens = Vec::new();
        loop {
            let token_line = self.line;
            let token_column = self.column + 1;
            let token_position = self.position;
            let token_byte_start = self.byte_pos;

            match self.next_token() {
                Ok(token) => {
                    let is_eof = token.token_type == TokenType::Eof;
                    tokens.push(token);
                    if is_eof {
                        break;
                    }
                }
                Err(_) => {
                    let mut byte_end = self.byte_pos;
                    // Guard against infinite loops if next_token failed without
                    // consuming anything.
                    if byte_end == token_byte_start {
                        if let Some(ch) = self.current_char {
                            byte_end += ch.len_utf8();
                        }
                        self.advance();
                    }
                    tokens.push(Token::new(
                        TokenType::Error,
                        token_position,
                        token_line,
                        token_column,
                        token_byte_start,
                        byte_end,
                    ));
                }
            }
        }
        self.recovery_mode = prev;
        tokens
    }
}

#[cfg(test)]
mod tests {
    use num_bigint::BigInt;

    use super::*;
    use crate::style::ColorMode;

    fn lexer_err(src: &str) -> WqError {
        Lexer::new(src)
            .tokenize()
            .expect_err("expected lexer error")
    }

    #[test]
    fn invalid_int_literal_uses_public_type_term() {
        let mut lexer = Lexer::new("!");
        let (line, column, byte) = (lexer.line, lexer.column, lexer.byte_pos);
        let err = lexer
            .read_number(line, column, byte)
            .expect_err("non-numeric input should reach the numeric fallback error");

        assert_eq!(err.msg.as_deref(), Some("invalid int literal"));
    }

    #[test]
    fn unknown_at_form_quotes_the_exact_syntax() {
        let err = lexer_err("@a");

        assert_eq!(err.msg.as_deref(), Some("unknown '@' form '@a'"));
    }

    #[test]
    fn standalone_dots_are_rejected() {
        for (source, span) in [
            (".", (0, 1)),
            (".1", (0, 1)),
            ("1.", (1, 2)),
            ("B.[1]", (1, 2)),
        ] {
            let err = lexer_err(source);

            assert_eq!(err.msg.as_deref(), Some("unrecognized character"));
            assert_eq!(err.span, Some(span));
        }
    }

    #[test]
    fn unknown_at_forms_are_rejected() {
        for (source, message, span) in [
            ("@", "unknown '@' form '@'", (0, 1)),
            ("@q", "unknown '@' form '@q'", (0, 2)),
            ("@-", "unknown '@' form '@-'", (0, 2)),
            ("@\0", "unknown '@' form '@\\0'", (0, 2)),
        ] {
            let err = lexer_err(source);

            assert_eq!(err.msg.as_deref(), Some(message));
            assert_eq!(err.span, Some(span));
        }
    }

    #[test]
    fn test_tokenize_numbers() {
        let mut lexer = Lexer::new("42 3.1 -5 1e3 2E-2 2i 3.5i 0x10i");
        let tokens = lexer.tokenize().unwrap();

        assert_eq!(tokens[0].token_type, TokenType::Integer(42));
        assert_eq!(tokens[1].token_type, TokenType::Float(3.1));
        assert_eq!(tokens[2].token_type, TokenType::Minus);
        assert_eq!(tokens[3].token_type, TokenType::Integer(5));
        assert_eq!(tokens[4].token_type, TokenType::Float(1000.0));
        assert_eq!(tokens[5].token_type, TokenType::Float(0.02));
        assert_eq!(tokens[6].token_type, TokenType::Imaginary(2.0));
        assert_eq!(tokens[7].token_type, TokenType::Imaginary(3.5));
        assert_eq!(tokens[8].token_type, TokenType::Imaginary(16.0));
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
        let mut lexer = Lexer::new("+ - * / % ^ = =. ~ ~. | |. || ||.");
        let tokens = lexer.tokenize().unwrap();

        assert_eq!(tokens[0].token_type, TokenType::Plus);
        assert_eq!(tokens[1].token_type, TokenType::Minus);
        assert_eq!(tokens[2].token_type, TokenType::Multiply);
        assert_eq!(tokens[3].token_type, TokenType::Divide);
        assert_eq!(tokens[4].token_type, TokenType::Modulo);
        assert_eq!(tokens[5].token_type, TokenType::Power);
        assert_eq!(tokens[6].token_type, TokenType::Equal);
        assert_eq!(tokens[7].token_type, TokenType::EqualDot);
        assert_eq!(tokens[8].token_type, TokenType::NotEqual);
        assert_eq!(tokens[9].token_type, TokenType::NotEqualDot);
        assert_eq!(tokens[10].token_type, TokenType::Pipe);
        assert_eq!(tokens[11].token_type, TokenType::PipeDot);
        assert_eq!(tokens[12].token_type, TokenType::PipePipe);
        assert_eq!(tokens[13].token_type, TokenType::PipePipeDot);
    }

    #[test]
    fn test_tokenize_dot_operators() {
        let mut lexer = Lexer::new("/.");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::DivideDot);
    }

    #[test]
    fn test_tokenize_power_dot() {
        let mut lexer = Lexer::new("^.");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::PowerDot);
    }

    #[test]
    fn test_tokenize_inf() {
        let mut lexer = Lexer::new("inf");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::Inf);
    }

    #[test]
    fn test_tokenize_symbols() {
        let mut lexer = Lexer::new("`hello `world");
        let tokens = lexer.tokenize().unwrap();

        assert_eq!(tokens[0].token_type, TokenType::Tag("hello".to_string()));
        assert_eq!(tokens[1].token_type, TokenType::Tag("world".to_string()));
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
    fn test_single_unicode_scalar_quoted_literal_is_string() {
        let mut lexer = Lexer::new("\"a\"");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::String("a".to_string()));
    }

    #[test]
    fn test_tokenize_unicode_scalar_literal() {
        let mut lexer = Lexer::new("@u\"a\" @u\"\\n\" @u\"🦀\"");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::Character('a'));
        assert_eq!(tokens[1].token_type, TokenType::Character('\n'));
        assert_eq!(tokens[2].token_type, TokenType::Character('🦀'));
    }

    #[test]
    fn test_tokenize_unicode_scalar_shorthand() {
        let mut lexer = Lexer::new("@u{0} @u{41} @u {a} @u{1f980} @u{10FFFF}+1");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::Character('\0'));
        assert_eq!(tokens[1].token_type, TokenType::Character('A'));
        assert_eq!(tokens[2].token_type, TokenType::Character('\n'));
        assert_eq!(tokens[3].token_type, TokenType::Character('🦀'));
        assert_eq!(tokens[4].token_type, TokenType::Character('\u{10ffff}'));
        assert_eq!(tokens[5].token_type, TokenType::Plus);
        assert_eq!(tokens[6].token_type, TokenType::Integer(1));
    }

    #[test]
    fn test_unicode_scalar_shorthand_rejects_invalid_code_points() {
        for source in [
            "@u{}",
            "@u{xyz}",
            "@u{41_}",
            "@u{1234567}",
            "@u{d800}",
            "@u{110000}",
        ] {
            let mut lexer = Lexer::new(source);
            assert!(lexer.tokenize().is_err(), "{source} should be rejected");
        }
    }

    #[test]
    fn test_unterminated_unicode_scalar_shorthand_is_incomplete() {
        let error = lexer_err("@u{1f980");

        assert_eq!(error.err_type, WqErrorType::Eof);
    }

    #[test]
    fn test_unicode_scalar_literal_requires_exactly_one_scalar() {
        for source in ["@u\"\"", "@u\"ab\"", "@u\"é\""] {
            let mut lexer = Lexer::new(source);
            assert!(lexer.tokenize().is_err(), "{source} should be rejected");
        }
    }

    #[test]
    fn test_unicode_scalar_literal_requires_quoted_content() {
        for source in ["@u", "@u  ", "@u a"] {
            let mut lexer = Lexer::new(source);
            assert!(lexer.tokenize().is_err(), "{source} should be rejected");
        }
    }

    #[test]
    fn test_strings_reject_malformed_hex_escapes() {
        for source in [
            r#""\x""#,
            r#""\x4""#,
            r#""\xGG""#,
            r#"@f"\x""#,
            r#"@f"\x4""#,
            r#"@f"\xGG""#,
        ] {
            let err = lexer_err(source);
            assert_eq!(err.msg.as_deref(), Some("invalid hex escape"));
        }
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
    fn control_keywords_are_not_identifiers() {
        let tokens = Lexer::new("W N B A and O or Word android or_else")
            .tokenize()
            .expect("tokenize keywords");
        assert_eq!(tokens[0].token_type, TokenType::Keyword(Keyword::WLoop));
        assert_eq!(tokens[1].token_type, TokenType::Keyword(Keyword::NLoop));
        assert_eq!(tokens[2].token_type, TokenType::Keyword(Keyword::Block));
        assert_eq!(tokens[3].token_type, TokenType::Keyword(Keyword::And));
        assert_eq!(tokens[4].token_type, TokenType::Keyword(Keyword::And));
        assert_eq!(tokens[5].token_type, TokenType::Keyword(Keyword::Or));
        assert_eq!(tokens[6].token_type, TokenType::Keyword(Keyword::Or));
        assert_eq!(
            tokens[7].token_type,
            TokenType::Identifier("Word".to_string())
        );
        assert_eq!(
            tokens[8].token_type,
            TokenType::Identifier("android".to_string())
        );
        assert_eq!(
            tokens[9].token_type,
            TokenType::Identifier("or_else".to_string())
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
    fn float_overflow_inf() {
        let big = "1".repeat(400) + ".0";
        let mut lexer = Lexer::new(&big);
        let res = lexer.tokenize().unwrap();
        assert!(matches!(res[0].token_type, TokenType::Float(f64::INFINITY)));
    }

    #[test]
    fn at_try_token() {
        let mut lexer = Lexer::new("@t 1");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::AtTry);
    }

    #[test]
    fn at_import_token() {
        let mut lexer = Lexer::new("@i\"module.wq\"");
        let tokens = lexer.tokenize().expect("tokenize import");
        assert_eq!(tokens[0].token_type, TokenType::AtImport);
        assert_eq!(
            tokens[1].token_type,
            TokenType::String("module.wq".to_string())
        );
    }

    #[test]
    fn at_debug_token() {
        let mut lexer = Lexer::new("@d 1");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::AtDebug);
    }

    #[test]
    fn at_depth_token() {
        let mut lexer = Lexer::new("@12 has?[x; y]");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::AtDepth(12));
    }

    #[test]
    fn at_negative_depth_token() {
        let mut lexer = Lexer::new("@-1 has?[x; y]");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::AtDepth(-1));
        assert_eq!((tokens[0].byte_start, tokens[0].byte_end), (0, 3));
    }

    #[test]
    fn unrecognized_character_error_has_colored_and_plain_underlines() {
        let err = lexer_err("?");
        assert_eq!(err.span, Some((0, 1)));

        let plain = err.render_with_color_mode(ColorMode::Never);
        assert!(
            plain.lines().any(|line| line.trim() == "~"),
            "expected plain underline in {plain:?}",
        );

        let colored = err.render_with_color_mode(ColorMode::Always);
        assert!(
            colored.contains("\x1b[1;4;32m?\x1b[0m"),
            "expected colored underline in {colored:?}",
        );
    }

    #[test]
    fn at_symbolic_token() {
        let mut lexer = Lexer::new("@s x+1");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::AtSymbolic);
        assert_eq!(tokens[1].token_type, TokenType::Identifier("x".into()));
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

    #[test]
    fn test_inline_comment_basic() {
        let mut lexer = Lexer::new("/* hello */");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(
            tokens[0].token_type,
            TokenType::Comment(" hello ".to_string())
        );
        assert_eq!(tokens[1].token_type, TokenType::Eof);
    }

    #[test]
    fn test_inline_comment_nested() {
        let mut lexer = Lexer::new("/* /* inner */ */");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(
            tokens[0].token_type,
            TokenType::Comment(" /* inner */ ".to_string())
        );
        assert_eq!(tokens[1].token_type, TokenType::Eof);
    }

    #[test]
    fn test_inline_comment_nested_empty() {
        let mut lexer = Lexer::new("/*/**/ */");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(
            tokens[0].token_type,
            TokenType::Comment("/**/ ".to_string())
        );
        assert_eq!(tokens[1].token_type, TokenType::Eof);
    }

    #[test]
    fn test_inline_comment_unclosed_returns_eof() {
        let mut lexer = Lexer::new("/* hello");
        let res = lexer.tokenize();
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.err_type, WqErrorType::Eof);
    }

    #[test]
    fn test_inline_comment_unclosed_nested_returns_eof() {
        let mut lexer = Lexer::new("/* /* hello */");
        let res = lexer.tokenize();
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.err_type, WqErrorType::Eof);
    }

    #[test]
    fn test_multiline_string() {
        let mut lexer = Lexer::new("\"a\nb\"");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::String("a\nb".to_string()));
    }

    #[test]
    fn test_unterminated_string_returns_eof() {
        let mut lexer = Lexer::new("\"abc");
        let res = lexer.tokenize();
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.err_type, WqErrorType::Eof);
    }

    #[test]
    fn test_unterminated_string_recovery_emits_token() {
        let mut lexer = Lexer::new("\"abc");
        let tokens = lexer.tokenize_recovery();
        assert_eq!(tokens[0].token_type, TokenType::String("abc".to_string()));
        assert_eq!(tokens[1].token_type, TokenType::Eof);
    }

    #[test]
    fn test_unterminated_comment_recovery_emits_token() {
        let mut lexer = Lexer::new("/* abc");
        let tokens = lexer.tokenize_recovery();
        assert_eq!(tokens[0].token_type, TokenType::Comment(" abc".to_string()));
        assert_eq!(tokens[1].token_type, TokenType::Eof);
    }

    #[test]
    fn test_triple_quote_string() {
        let mut lexer = Lexer::new("\"\"\"hello\"\"\"");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::String("hello".to_string()));
    }

    #[test]
    fn test_quad_quote_string() {
        let mut lexer = Lexer::new("\"\"\"\"hello\"\"\"\"");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token_type, TokenType::String("hello".to_string()));
    }

    #[test]
    fn test_triple_quote_with_embedded_quotes() {
        let input: String = [
            '"', '"', '"', 'i', ' ', 's', 'a', 'i', 'd', ' ', '"', 'h', 'e', 'l', 'l', 'o', '"',
            ' ', '"', '"', '"',
        ]
        .into_iter()
        .collect();
        let mut lexer = Lexer::new(&input);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(
            tokens[0].token_type,
            TokenType::String("i said \"hello\" ".to_string())
        );
    }

    #[test]
    fn test_triple_quote_with_escaped_quotes() {
        // lexer sees: """hello\""""
        let input: String = [
            '"', '"', '"', 'h', 'e', 'l', 'l', 'o', '\\', '"', '"', '"', '"',
        ]
        .into_iter()
        .collect();
        let mut lexer = Lexer::new(&input);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(
            tokens[0].token_type,
            TokenType::String("hello\"".to_string())
        );
    }

    #[test]
    fn test_six_quotes_is_unterminated() {
        // lexer sees: """""" (6 opening quotes, no closing)
        let input: String = ['"', '"', '"', '"', '"', '"'].into_iter().collect();
        let mut lexer = Lexer::new(&input);
        let res = lexer.tokenize();
        assert!(res.is_err());
    }

    #[test]
    fn test_triple_quote_unterminated_errors() {
        let mut lexer = Lexer::new("\"\"\"abc");
        let res = lexer.tokenize();
        assert!(res.is_err());
    }

    #[test]
    fn test_triple_quote_recovery_emits_token() {
        let mut lexer = Lexer::new("\"\"\"abc");
        let tokens = lexer.tokenize_recovery();
        assert_eq!(tokens[0].token_type, TokenType::String("abc".to_string()));
        assert_eq!(tokens[1].token_type, TokenType::Eof);
    }
}
