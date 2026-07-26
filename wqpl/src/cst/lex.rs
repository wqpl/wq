//! Lexer-to-CST helpers.
//!
//! This module maps lexer [`Token`]s to [`SyntaxKind`]s and can build a flat
//! `Root` whose children are source-order tokens. Since the lexer omits
//! horizontal whitespace, byte gaps are synthesized as
//! [`SyntaxKind::Whitespace`].
//!
//! The flat tree is mainly for lexer tests and tooling. The parser drives
//! [`GreenNodeBuilder`] for structured CSTs. Both paths preserve `source`
//! byte-for-byte.

use super::builder::GreenNodeBuilder;
use super::green::GreenNode;
use super::kind::SyntaxKind;
use crate::lex::Lexer;
use crate::token::{Keyword, Token, TokenType};
use crate::value::WqResult;

/// Map a lexer token type onto its CST kind.
///
/// `Eof` has no CST representation; every other token maps directly.
pub fn syntax_kind_of_token(tt: &TokenType) -> SyntaxKind {
    use SyntaxKind as K;
    use TokenType as T;
    match tt {
        // Identifiers and literals.
        T::Identifier(_) => K::Ident,
        T::Keyword(Keyword::WLoop) => K::WLoopKw,
        T::Keyword(Keyword::NLoop) => K::NLoopKw,
        T::Keyword(Keyword::Block) => K::BlockKw,
        T::Keyword(Keyword::And) => K::AndKw,
        T::Keyword(Keyword::Or) => K::OrKw,
        T::Integer(_) => K::IntLit,
        T::BigInteger(_) => K::BigIntLit,
        T::Float(_) => K::FloatLit,
        T::Imaginary(_) => K::ImagLit,
        T::Character(_) => K::CharLit,
        T::String(_) => K::StringLit,
        T::Tag(_) => K::TagLit,
        T::Backtick => K::Backtick,
        T::Inf => K::InfLit,
        T::True => K::TrueKw,
        T::False => K::FalseKw,
        T::FormatString(..) => K::FString,

        // Identifier-adjacent.
        T::Apostrophe => K::Apostrophe,
        T::Ellipsis => K::Ellipsis,

        // `@`-keywords.
        T::AtBreak => K::AtBreak,
        T::AtContinue => K::AtContinue,
        T::AtReturn => K::AtReturn,
        T::AtDebug => K::AtDebug,
        T::AtPause => K::AtPause,
        T::AtDepth(_) => K::AtDepth,
        T::AtSymbolic => K::AtSymbolic,
        T::AtTry => K::AtTry,
        T::AtImport => K::AtImport,

        // `$`-keywords.
        T::Dollar => K::Dollar,
        T::DollarDot => K::DollarDot,
        T::DollarDollar => K::DollarDollar,

        // Arithmetic operators.
        T::Plus => K::Plus,
        T::Minus => K::Minus,
        T::Multiply => K::Star,
        T::Power => K::Power,
        T::PowerDot => K::PowerDot,
        T::Divide => K::Slash,
        T::DivideDot => K::SlashDot,
        T::Modulo => K::Percent,
        T::Matmul => K::Matmul,
        T::FloorDiv => K::FloorDiv,

        // Augmented assignments.
        T::PlusColon => K::PlusColon,
        T::MinusColon => K::MinusColon,
        T::MultiplyColon => K::StarColon,
        T::DivideColon => K::SlashColon,
        T::DivideDotColon => K::SlashDotColon,
        T::ModuloColon => K::PercentColon,
        T::PowerColon => K::PowerColon,
        T::PowerDotColon => K::PowerDotColon,
        T::CommaColon => K::CommaColon,
        T::FloorDivColon => K::FloorDivColon,

        T::Colon => K::Colon,

        // Comparison.
        T::Equal => K::EqEq,
        T::EqualDot => K::EqDot,
        T::NotEqual => K::NotEq,
        T::NotEqualDot => K::NotEqDot,
        T::LessThan => K::Lt,
        T::LessThanOrEqual => K::Le,
        T::GreaterThan => K::Gt,
        T::GreaterThanOrEqual => K::Ge,

        // Misc tokens.
        T::Sharp => K::Hash,
        T::Pipe => K::Pipe,
        T::PipeDot => K::PipeDot,
        T::PipePipe => K::PipePipe,
        T::PipePipeDot => K::PipePipeDot,
        T::Range => K::RangeOp,
        T::RangeInclusive => K::RangeIncOp,

        // Brackets and punctuation.
        T::LeftParen => K::LParen,
        T::RightParen => K::RParen,
        T::LeftBracket => K::LBrack,
        T::RightBracket => K::RBrack,
        T::LeftBrace => K::LBrace,
        T::RightBrace => K::RBrace,
        T::Semicolon => K::Semicolon,
        T::Comma => K::Comma,
        T::Bang => K::Bang,

        // Trivia (Newline, Comment) and recovery.
        T::Newline => K::Newline,
        T::Comment(_) => K::Comment,
        T::Error => K::ErrorTok,

        // Callers filter Eof before mapping.
        T::Eof => K::ErrorTok,
    }
}

/// True for tokens excluded from the CST.
fn is_eof(tt: &TokenType) -> bool {
    matches!(tt, TokenType::Eof)
}

/// Build a flat green tree from a lexer token stream.
///
/// The root is [`SyntaxKind::Root`]. Its children are source-order tokens plus
/// synthesized [`SyntaxKind::Whitespace`] for byte gaps.
///
/// Round-trip invariant: `result.text() == source`.
///
/// `tokens` must include comments, newlines, and the trailing `Eof`.
pub fn build_flat_cst_from_tokens(source: &str, tokens: &[Token]) -> GreenNode {
    let mut b = GreenNodeBuilder::new();
    b.start_node(SyntaxKind::Root);
    push_with_trivia(&mut b, source, tokens);
    b.finish_node();
    b.finish()
}

/// Lex `source`, then build the flat CST.
///
/// Errors out on the first lexing failure. For an error-tolerant variant see
/// [`build_flat_cst_recovery`].
pub fn build_flat_cst(source: &str) -> WqResult<GreenNode> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;
    Ok(build_flat_cst_from_tokens(source, &tokens))
}

/// Recovery variant that keeps lexer errors covered as `ErrorTok`.
pub fn build_flat_cst_recovery(source: &str) -> GreenNode {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize_recovery();
    build_flat_cst_from_tokens(source, &tokens)
}

/// Push `tokens` onto `b`, synthesizing whitespace from byte gaps.
pub(super) fn push_with_trivia(b: &mut GreenNodeBuilder, source: &str, tokens: &[Token]) {
    let mut cursor: usize = 0;
    for tok in tokens {
        if is_eof(&tok.token_type) {
            // Eof marks the end; trailing whitespace is emitted after the loop.
            break;
        }
        debug_assert!(
            tok.byte_start >= cursor,
            "lexer tokens out of byte order: cursor={cursor}, next={}..{}",
            tok.byte_start,
            tok.byte_end,
        );
        if tok.byte_start > cursor {
            let gap = &source[cursor..tok.byte_start];
            b.token(SyntaxKind::Whitespace, gap);
        }
        let text = &source[tok.byte_start..tok.byte_end];
        b.token(syntax_kind_of_token(&tok.token_type), text);
        cursor = tok.byte_end;
    }
    // Trailing whitespace after the last real token.
    if cursor < source.len() {
        let trailing = &source[cursor..];
        b.token(SyntaxKind::Whitespace, trailing);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(src: &str) {
        let g = build_flat_cst(src).unwrap_or_else(|e| panic!("lex error on `{src}`: {e:?}"));
        assert_eq!(g.text(), src, "round-trip mismatch on `{src}`",);
        // Every source byte must be covered.
        let text_len = usize::try_from(g.text_len()).expect("text length fits in usize");
        assert_eq!(text_len, src.len(), "text_len mismatch on `{src}`");
    }

    fn round_trip_recovery(src: &str) {
        let g = build_flat_cst_recovery(src);
        assert_eq!(g.text(), src, "recovery round-trip mismatch on `{src}`");
        let text_len = usize::try_from(g.text_len()).expect("text length fits in usize");
        assert_eq!(text_len, src.len());
    }

    #[test]
    fn empty_source() {
        round_trip("");
    }

    #[test]
    fn whitespace_only() {
        round_trip("   ");
        round_trip("\t \t");
        round_trip("\n");
        round_trip("\n\n  \n");
    }

    #[test]
    fn simple_tokens_with_spaces() {
        round_trip("1 + 2");
        round_trip("  1+2  ");
        round_trip("a:b");
        round_trip("foo[1; 2; 3]");
    }

    #[test]
    fn comments_preserved() {
        round_trip("// a line comment\n1+2");
        round_trip("1 /* inline */ + 2");
        round_trip("// trailing\n");
    }

    #[test]
    fn newlines_preserved() {
        round_trip("a:1\nb:2\nc:3\n");
        round_trip("\n\nfoo\n\n");
    }

    #[test]
    fn unicode_in_strings_round_trips() {
        round_trip("\"héllo, 世界\"");
    }

    #[test]
    fn fstring_round_trips_as_one_token() {
        let src = r#"@f"x={1+2}""#;
        round_trip(src);
        // Flat CST keeps the format string as one token.
        let g = build_flat_cst(src).unwrap();
        let fstrings = g
            .children()
            .iter()
            .filter(|c| matches!(c.kind(), SyntaxKind::FString))
            .count();
        assert_eq!(fstrings, 1);
    }

    #[test]
    fn function_block() {
        round_trip("{[a;b]a+b}");
        round_trip("{[a; b]\n  a + b\n}");
    }

    #[test]
    fn control_flow() {
        round_trip("$[c;t;f]");
        round_trip("$.[c;t]");
        round_trip("$$[c1;t1;c2;t2;d]");
        round_trip("W[c;b]");
        round_trip("N[10;@b]");
    }

    #[test]
    fn assignments() {
        round_trip("x:1");
        round_trip("x+:1");
        round_trip("(a;b):1,2");
    }

    #[test]
    fn pipes() {
        round_trip("1|f");
        round_trip("1|.f");
        round_trip("xs|sum");
    }

    #[test]
    fn recovery_on_unterminated_string_round_trips() {
        // Lexer errors still round-trip as ErrorTok.
        round_trip_recovery(r#""open string"#);
        round_trip_recovery("1 + ");
    }

    #[test]
    fn token_kinds_match_lexer_output() {
        // Representative source with trivia interleaved.
        let src = "x : 1 + 2 // hi\n";
        let mut lexer = Lexer::new(src);
        let toks = lexer.tokenize().unwrap();
        let g = build_flat_cst_from_tokens(src, &toks);
        let kinds: Vec<_> = g
            .children()
            .iter()
            .filter_map(|c| c.as_token().map(|t| t.kind()))
            .collect();

        // Expected kinds: lexer tokens plus synthesized whitespace.
        let mut expected = Vec::new();
        let mut cursor = 0usize;
        for t in &toks {
            if is_eof(&t.token_type) {
                break;
            }
            if t.byte_start > cursor {
                expected.push(SyntaxKind::Whitespace);
            }
            expected.push(syntax_kind_of_token(&t.token_type));
            cursor = t.byte_end;
        }
        if cursor < src.len() {
            expected.push(SyntaxKind::Whitespace);
        }
        assert_eq!(kinds, expected);
    }

    #[test]
    fn each_byte_gap_becomes_one_whitespace_token() {
        // Leading, middle, and trailing gaps become separate Whitespace tokens.
        let src = "  a  b  ";
        let g = build_flat_cst(src).unwrap();
        let kinds: Vec<_> = g
            .children()
            .iter()
            .filter_map(|c| c.as_token().map(|t| (t.kind(), t.text().to_string())))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (SyntaxKind::Whitespace, "  ".to_string()),
                (SyntaxKind::Ident, "a".to_string()),
                (SyntaxKind::Whitespace, "  ".to_string()),
                (SyntaxKind::Ident, "b".to_string()),
                (SyntaxKind::Whitespace, "  ".to_string()),
            ],
        );
    }

    #[test]
    fn corpus_round_trip() {
        // Every `e/*.wq` file should round-trip cleanly.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("e");
        if !dir.exists() {
            // Some packaged builds omit examples.
            return;
        }
        let entries =
            std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot read {dir:?}: {e}"));
        let mut checked = 0;
        for entry in entries {
            let entry = entry.expect("read_dir entry");
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("wq") {
                continue;
            }
            let src = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {path:?}: {e}"));
            // Recovery keeps this a round-trip test, not a syntax test.
            let g = build_flat_cst_recovery(&src);
            assert_eq!(g.text(), src, "{} did not round-trip", path.display());
            checked += 1;
        }
        assert!(checked > 0, "no .wq examples were checked under {dir:?}");
    }
}
