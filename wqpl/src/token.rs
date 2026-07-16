use num_bigint::BigInt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Keyword {
    WLoop,
    NLoop,
    Block,
    And,
    Or,
}

impl Keyword {
    pub(crate) fn from_identifier(identifier: &str) -> Option<Self> {
        match identifier {
            "W" => Some(Self::WLoop),
            "N" => Some(Self::NLoop),
            "B" => Some(Self::Block),
            "A" | "and" => Some(Self::And),
            "O" | "or" => Some(Self::Or),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenType {
    Identifier(String),
    Keyword(Keyword),

    Integer(i64),
    BigInteger(BigInt),
    Float(f64),
    Imaginary(f64),
    Character(char),
    String(String),
    Tag(String),
    Backtick, // lone backtick (no identifier) used for special syntax like (`)
    Inf,
    True,
    False,

    Plus,
    Minus,
    Multiply,
    Power,
    PowerDot,
    Divide,
    DivideDot,
    Modulo,
    Matmul,
    FloorDiv,

    PlusColon,
    MinusColon,
    MultiplyColon,
    DivideColon,
    DivideDotColon,
    ModuloColon,
    PowerColon,
    PowerDotColon,
    CommaColon,
    FloorDivColon,

    Colon,

    Equal,
    EqualDot,
    NotEqual,
    NotEqualDot,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,

    Sharp,
    Pipe,
    PipeDot,
    PipePipe,
    PipePipeDot,
    Range,
    RangeInclusive,

    Dollar,
    DollarDot,
    DollarDollar,

    AtBreak,
    AtContinue,
    AtReturn,
    AtDebug,
    AtPause,
    AtDepth(i64),
    // @s ... symbolic quote
    AtSymbolic,
    AtTry,

    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    Semicolon,
    Comma,

    Ellipsis, // ...
    Apostrophe,

    Comment(String),
    Newline,
    Eof,
    Bang,
    /// Lexer error token emitted in recovery mode so highlighting can continue.
    Error,
    /// F-string: `@f"..."` lexed as a single token with segmented parts.
    /// The two `usize` values are the byte positions of the opening and closing
    /// quote characters, so the highlighter can colour `@f`, the quotes, and
    /// the contents separately.
    FormatString(Vec<FmtPart>, usize, usize),
}

/// A segment inside a format string (`@f"..."`).
#[derive(Debug, Clone, PartialEq)]
pub enum FmtPart {
    /// Plain text slice (raw, escapes intact).
    Text {
        content: String,
        start: usize,
        end: usize,
    },
    /// Braced expression, including the surrounding `{` and `}`.
    Expr {
        source: String,
        start: usize,
        end: usize,
    },
}

impl std::fmt::Display for TokenType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub token_type: TokenType,
    pub position: usize,
    pub line: usize,
    pub column: usize,
    // Byte offsets into the original source string [start, end)
    pub byte_start: usize,
    pub byte_end: usize,
}

impl Token {
    pub(crate) fn new(
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

/// Rebase a token produced from a source fragment into its containing source.
///
/// The lexer intentionally keeps parser-facing tokens local to the fragment.
/// Tooling and diagnostics can clone those tokens and use this helper when
/// they need stable file-wide character, line, column, and byte coordinates.
pub(crate) fn rebase_token(token: &mut Token, full_source: &str, base_offset: usize) {
    let prefix = &full_source[..base_offset];
    let local_line = token.line;
    token.position += prefix.chars().count();
    token.line += prefix.bytes().filter(|byte| *byte == b'\n').count();
    if local_line == 1 {
        token.column += prefix
            .rsplit('\n')
            .next()
            .expect("split always yields one segment")
            .chars()
            .count();
    }
    token.byte_start += base_offset;
    token.byte_end += base_offset;
    if let TokenType::FormatString(parts, open_quote, close_quote) = &mut token.token_type {
        *open_quote += base_offset;
        *close_quote += base_offset;
        for part in parts {
            match part {
                FmtPart::Text { start, end, .. } | FmtPart::Expr { start, end, .. } => {
                    *start += base_offset;
                    *end += base_offset;
                }
            }
        }
    }
}

// impl fmt::Display for Token {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         write!(f, "{:?}@{}:{}", self.token_type, self.line, self.column)
//     }
// }

pub fn fmt_tokens_table(tokens: &[Token]) -> String {
    // Column order matches your example
    let headers = ["type", "pos", "line", "col", "start", "end"];
    let mut rows: Vec<[String; 6]> = Vec::with_capacity(tokens.len());
    for t in tokens {
        rows.push([
            format!("{:?}", t.token_type), // or implement Display for nicer names
            t.position.to_string(),
            t.line.to_string(),
            t.column.to_string(),
            t.byte_start.to_string(),
            t.byte_end.to_string(),
        ]);
    }

    // Compute column widths (start with header widths)
    let mut w: [usize; 6] = [
        headers[0].len(),
        headers[1].len(),
        headers[2].len(),
        headers[3].len(),
        headers[4].len(),
        headers[5].len(),
    ];
    for r in &rows {
        for i in 0..6 {
            w[i] = w[i].max(r[i].len());
        }
    }

    // Build the table
    let mut out = String::new();

    // Header (token_type left, numbers right)
    out.push_str(&format!(
        "{:<w0$} {:>w1$} {:>w2$} {:>w3$} {:>w4$} {:>w5$}\n",
        headers[0],
        headers[1],
        headers[2],
        headers[3],
        headers[4],
        headers[5],
        w0 = w[0],
        w1 = w[1],
        w2 = w[2],
        w3 = w[3],
        w4 = w[4],
        w5 = w[5]
    ));

    // Rows
    for r in rows {
        out.push_str(&format!(
            "{:<w0$} {:>w1$} {:>w2$} {:>w3$} {:>w4$} {:>w5$}\n",
            r[0],
            r[1],
            r[2],
            r[3],
            r[4],
            r[5],
            w0 = w[0],
            w1 = w[1],
            w2 = w[2],
            w3 = w[3],
            w4 = w[4],
            w5 = w[5]
        ));
    }

    out.pop();
    out
}
