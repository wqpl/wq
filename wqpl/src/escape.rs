#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnescapeErrorKind {
    InvalidHexEscape,
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

/// Return a fully quoted string literal using the given quote character.
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
        let esc_start = i.saturating_sub(1);
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
                // Hex escapes require exactly two hexadecimal digits.
                let (d1, d2);
                match chars.peek().copied() {
                    Some(c) if c.is_ascii_hexdigit() => d1 = c,
                    _ => {
                        return Err(UnescapeError {
                            kind: UnescapeErrorKind::InvalidHexEscape,
                            index: esc_start,
                        });
                    }
                }
                // consume d1
                let _ = chars.next();
                i += d1.len_utf8();
                match chars.peek().copied() {
                    Some(c) if c.is_ascii_hexdigit() => d2 = c,
                    _ => {
                        return Err(UnescapeError {
                            kind: UnescapeErrorKind::InvalidHexEscape,
                            index: esc_start,
                        });
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
                                    val = (val << 4) | u32::from(hex_val(c));
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

/// Return the byte length of a supported escape sequence at the start of `s`.
/// Unknown and malformed escapes are intentionally excluded, matching the
/// compatibility behavior of `unescape_string_inner`.
pub(crate) fn valid_escape_sequence_len(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'\\') {
        return None;
    }

    match bytes.get(1).copied()? {
        b'"' | b'\'' | b'\\' | b'n' | b'r' | b't' | b'0' => Some(2),
        b'x' => {
            let digits = bytes.get(2..4)?;
            digits.iter().all(u8::is_ascii_hexdigit).then_some(4)
        }
        b'u' if bytes.get(2) == Some(&b'{') => {
            let close = bytes.get(3..)?.iter().position(|byte| *byte == b'}')? + 3;
            let digits = &bytes[3..close];
            if !(1..=6).contains(&digits.len()) || !digits.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }

            let digits = std::str::from_utf8(digits).ok()?;
            let value = u32::from_str_radix(digits, 16).ok()?;
            char::from_u32(value).map(|_| close + 1)
        }
        _ => None,
    }
}

fn hex_val(c: char) -> u8 {
    c.to_digit(16)
        .and_then(|digit| u8::try_from(digit).ok())
        .unwrap_or(0)
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
        for source in ["\\x", "\\x4", "\\xG0"] {
            assert_eq!(
                unescape_string_inner(source)
                    .expect_err("malformed hex escape should fail")
                    .kind,
                UnescapeErrorKind::InvalidHexEscape
            );
        }
        assert!(unescape_string_inner("\\u{}").is_err());
        // Trailing backslash kept literally
        assert_eq!(unescape_string_inner("\\").unwrap(), "\\");
        // Unknown escapes kept literally
        assert_eq!(unescape_string_inner("\\q").unwrap(), "\\q");
    }

    #[test]
    fn identifies_only_supported_escape_sequences() {
        for escape in [
            r#"\""#,
            r"\'",
            r"\\",
            r"\n",
            r"\r",
            r"\t",
            r"\0",
            r"\x41",
            r"\u{0}",
            r"\u{10ffff}",
        ] {
            assert_eq!(valid_escape_sequence_len(escape), Some(escape.len()));
        }

        for escape in [
            r"\q",
            r"\x",
            r"\x4",
            r"\xGG",
            r"\u",
            r"\u{}",
            r"\u{d800}",
            r"\u{110000}",
            r"\u{0000000}",
        ] {
            assert_eq!(valid_escape_sequence_len(escape), None);
        }
    }
}
