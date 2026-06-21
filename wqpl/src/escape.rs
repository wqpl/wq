#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnescapeErrorKind {
    InvalidUnicodeEscape,
    InvalidUnicodeScalar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnescapeError {
    pub(crate) kind: UnescapeErrorKind,
    /// Byte offset within the input string where the error occurred.
    pub(crate) index: usize,
}

/// Escape inner content suitable for inclusion inside a quoted string literal.
pub(crate) fn escape_string_inner(s: &str, quote: char) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            c if c == quote => {
                out.push('\\');
                out.push(quote);
            }
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            // Visible ASCII and non-control Unicode remain as-is
            c if !c.is_control() => out.push(c),
            c => out.extend(c.escape_unicode()),
        }
    }
    out
}

/// Convenience: returns a fully quoted string literal using the given `quote`.
///
/// Not currently called (the old AST-based formatter used it; the CST
/// formatter pulls string text verbatim from the source). Kept as a public
/// primitive for callers that need to synthesize quoted string literals
/// without duplicating escape rules.
#[allow(dead_code)]
pub(crate) fn quote_string(s: &str, quote: char) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    out.push_str(&escape_string_inner(s, quote));
    out.push(quote);
    out
}

/// Unescape the inner content of a quoted string literal.
/// Returns an error with the byte index of the offending sequence on failure.
pub(crate) fn unescape_string_inner(s: &str) -> Result<String, UnescapeError> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let mut i = 0usize; // byte index

    while let Some(ch) = chars.next() {
        i += ch.len_utf8();
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        // start of an escape
        let _esc_start = i.saturating_sub(1);
        let Some(next) = chars.next() else {
            // Compatibility: treat trailing backslash as literal
            out.push('\\');
            break;
        };
        i += next.len_utf8();
        match next {
            '"' => out.push('"'),
            '\'' => out.push('\''),
            '\\' => out.push('\\'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            '0' => out.push('\0'),
            'x' => {
                // two hex digits; if malformed, keep literally as "\\x"
                let (d1, d2);
                match chars.peek().copied() {
                    Some(c) if c.is_ascii_hexdigit() => d1 = c,
                    _ => {
                        out.push('\\');
                        out.push('x');
                        continue;
                    }
                }
                // consume d1
                let _ = chars.next();
                i += d1.len_utf8();
                match chars.peek().copied() {
                    Some(c) if c.is_ascii_hexdigit() => d2 = c,
                    _ => {
                        // backtrack impossible; emit literally
                        out.push('\\');
                        out.push('x');
                        out.push(d1);
                        continue;
                    }
                }
                let _ = chars.next();
                i += d2.len_utf8();
                let byte = (hex_val(d1) << 4) | hex_val(d2);
                out.push(byte as char);
            }
            'u' => {
                // Expect {HEX+}
                match chars.next() {
                    Some('{') => {
                        i += 1; // '{' is ASCII
                        let mut val: u32 = 0;
                        let mut digits = 0usize;
                        loop {
                            match chars.peek().copied() {
                                Some('}') => {
                                    // consume '}'
                                    let _ = chars.next();
                                    i += 1; // '}'
                                    break;
                                }
                                Some(c) if c.is_ascii_hexdigit() => {
                                    val = (val << 4) | (hex_val(c) as u32);
                                    digits += 1;
                                    let _ = chars.next();
                                    i += c.len_utf8();
                                    if digits > 6 {
                                        // match Rust-like upper bound
                                        return Err(UnescapeError {
                                            kind: UnescapeErrorKind::InvalidUnicodeEscape,
                                            index: i,
                                        });
                                    }
                                }
                                Some(_) | None => {
                                    return Err(UnescapeError {
                                        kind: UnescapeErrorKind::InvalidUnicodeEscape,
                                        index: i,
                                    });
                                }
                            }
                        }
                        if digits == 0 {
                            return Err(UnescapeError {
                                kind: UnescapeErrorKind::InvalidUnicodeEscape,
                                index: i,
                            });
                        }
                        if let Some(ch) = char::from_u32(val) {
                            out.push(ch);
                        } else {
                            return Err(UnescapeError {
                                kind: UnescapeErrorKind::InvalidUnicodeScalar,
                                index: i,
                            });
                        }
                    }
                    _ => {
                        return Err(UnescapeError {
                            kind: UnescapeErrorKind::InvalidUnicodeEscape,
                            index: i,
                        });
                    }
                }
            }
            other => {
                // Compatibility with previous lexer: keep unknown escapes literally
                out.push('\\');
                out.push(other);
            }
        }
    }

    Ok(out)
}

fn hex_val(c: char) -> u8 {
    match c {
        '0'..='9' => (c as u8) - b'0',
        'a'..='f' => (c as u8) - b'a' + 10,
        'A'..='F' => (c as u8) - b'A' + 10,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_basic() {
        let cases = vec![
            "abc",
            "a\n\tb",
            "\\\\\"",
            "\u{1f600}",
            "\x00\x1b\x7f",
            "nul:\u{0}",
        ];
        for raw in cases {
            let esc = escape_string_inner(raw, '"');
            let un = unescape_string_inner(&esc).unwrap();
            assert_eq!(un, raw);
        }
    }

    #[test]
    fn unescape_works() {
        assert_eq!(unescape_string_inner("a\\nb").unwrap(), "a\nb");
        assert_eq!(unescape_string_inner("\\x41").unwrap(), "A");
        assert_eq!(
            unescape_string_inner("\\u{1f4a9}").unwrap(),
            "\u{1f4a9}".chars().collect::<String>()
        );
    }

    #[test]
    fn rejects_invalid() {
        // Malformed \x should be kept literally (compat mode)
        assert_eq!(unescape_string_inner("\\xG0").unwrap(), "\\xG0");
        assert!(unescape_string_inner("\\u{}").is_err());
        // Trailing backslash kept literally
        assert_eq!(unescape_string_inner("\\").unwrap(), "\\");
        // Unknown escapes kept literally
        assert_eq!(unescape_string_inner("\\q").unwrap(), "\\q");
    }
}
