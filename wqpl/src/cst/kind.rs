//! Syntax kinds for green and red trees.
//!
//! Token kinds must stay before [`SyntaxKind::__LastToken`]; node kinds must
//! stay after it. This keeps [`SyntaxKind::is_token`] and
//! [`SyntaxKind::is_node`] cheap.
//!
//! Add trivia-like tokens to [`SyntaxKind::is_trivia`].

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum SyntaxKind {
    // Trivia tokens.
    Whitespace,
    Newline,
    Comment,

    // Literal tokens.
    IntLit,
    BigIntLit,
    FloatLit,
    ImagLit,
    CharLit,
    StringLit,
    TagLit,
    Backtick,
    InfLit,
    TrueKw,
    FalseKw,
    /// Entire `@f"..."` literal as emitted by the lexer.
    FString,

    // Identifiers.
    Ident,
    WLoopKw,
    NLoopKw,
    BlockKw,
    AndKw,
    OrKw,
    Apostrophe,
    Ellipsis,

    // `@`-keywords.
    AtBreak,
    AtContinue,
    AtReturn,
    AtDebug,
    AtPause,
    AtDepth,
    AtSymbolic,
    AtTry,

    // `$`-keywords.
    Dollar,
    DollarDot,
    DollarDollar,

    // Arithmetic operators.
    Plus,
    Minus,
    Star,
    Slash,
    SlashDot,
    Percent,
    Power,
    PowerDot,
    Matmul,
    FloorDiv,

    // Augmented assignments.
    PlusColon,
    MinusColon,
    StarColon,
    SlashColon,
    SlashDotColon,
    PercentColon,
    PowerColon,
    PowerDotColon,
    CommaColon,
    FloorDivColon,

    Colon,

    // Comparison.
    EqEq,
    EqDot,
    NotEq,
    NotEqDot,
    Lt,
    Le,
    Gt,
    Ge,

    // Pipes, range, and misc.
    Hash,
    Pipe,
    PipeDot,
    PipePipe,
    PipePipeDot,
    RangeOp,
    RangeIncOp,

    // Brackets and punctuation.
    LParen,
    RParen,
    LBrack,
    RBrack,
    LBrace,
    RBrace,
    Semicolon,
    Comma,
    Bang,
    ScriptLine,

    /// Lexer recovery token that keeps source coverage intact.
    ErrorTok,

    /// Sentinel between token and node kinds.
    #[doc(hidden)]
    __LastToken,

    // Nodes.
    /// Top of the tree. Every parse produces exactly one of these.
    Root,
    Block,
    Shebang,
    ScriptDirective,

    // Expression-shaped nodes.
    LiteralExpr,
    VarExpr,
    OuterVarExpr,
    BinaryExpr,
    UnaryExpr,
    ComparisonChainExpr,
    AssignExpr,
    OuterAssignExpr,
    UnpackAssignExpr,
    IndexAssignExpr,
    MutatingIndexExpr,
    MutatingIndexAssignExpr,
    RangeExpr,
    ListExpr,
    DictExpr,
    DictPair,

    ParenExpr,
    PostfixExpr,
    LazyBoolExpr,
    /// Named argument at a call site: `<backtick>name: value`
    NamedArgExpr,
    /// Argument list inside `[...]` for postfix calls/indexing. Holds the
    /// items separated by `;` (kept as tokens for round-trip).
    ArgList,
    FStringExpr,
    CondExpr,
    CondDotExpr,
    CondChainExpr,
    WLoopExpr,
    NLoopExpr,
    BlockExpr,
    FunctionExpr,
    ParamList,
    Param,
    ReturnExpr,
    DebugExpr,
    PauseExpr,
    TryExpr,
    SymbolicExpr,
    BreakExpr,
    ContinueExpr,
    EllipsisExpr,
    PipeExpr,
    PipeTapExpr,

    /// Tokens skipped during parser recovery.
    ErrorNode,
}

impl SyntaxKind {
    /// True when this kind is produced by the lexer (or by trivia synthesis).
    #[inline]
    pub fn is_token(self) -> bool {
        (self as u16) < (SyntaxKind::__LastToken as u16)
    }

    /// True when this kind is an internal tree node.
    #[inline]
    pub fn is_node(self) -> bool {
        (self as u16) > (SyntaxKind::__LastToken as u16)
    }

    /// True for trivia tokens with no semantic payload.
    #[inline]
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            SyntaxKind::Whitespace | SyntaxKind::Newline | SyntaxKind::Comment
        )
    }

    /// Stable name for debug output and snapshots.
    pub fn name(self) -> &'static str {
        use SyntaxKind::*;
        match self {
            Whitespace => "WHITESPACE",
            Newline => "NEWLINE",
            Comment => "COMMENT",
            IntLit => "INT",
            BigIntLit => "BIG_INT",
            FloatLit => "FLOAT",
            ImagLit => "IMAG",
            CharLit => "CHAR",
            StringLit => "STRING",
            TagLit => "TAG",
            Backtick => "BACKTICK",
            InfLit => "INF",
            TrueKw => "TRUE_KW",
            FalseKw => "FALSE_KW",
            FString => "FSTRING",
            Ident => "IDENT",
            WLoopKw => "W_KW",
            NLoopKw => "N_KW",
            BlockKw => "B_KW",
            AndKw => "AND_KW",
            OrKw => "OR_KW",
            Apostrophe => "APOS",
            Ellipsis => "ELLIPSIS",
            AtBreak => "AT_BREAK",
            AtContinue => "AT_CONTINUE",
            AtReturn => "AT_RETURN",
            AtDebug => "AT_DEBUG",
            AtPause => "AT_PAUSE",
            AtDepth => "AT_DEPTH",
            AtSymbolic => "AT_SYMBOLIC",
            AtTry => "AT_TRY",

            Dollar => "DOLLAR",
            DollarDot => "DOLLAR_DOT",
            DollarDollar => "DOLLAR_DOLLAR",
            Plus => "PLUS",
            Minus => "MINUS",
            Star => "STAR",
            Slash => "SLASH",
            SlashDot => "SLASH_DOT",
            Percent => "PERCENT",
            Power => "POWER",
            PowerDot => "POWER_DOT",
            Matmul => "MATMUL",
            FloorDiv => "FLOOR_DIV",

            PlusColon => "PLUS_COLON",
            MinusColon => "MINUS_COLON",
            StarColon => "STAR_COLON",
            SlashColon => "SLASH_COLON",
            SlashDotColon => "SLASH_DOT_COLON",
            PercentColon => "PERCENT_COLON",
            PowerColon => "POWER_COLON",
            PowerDotColon => "POWER_DOT_COLON",
            CommaColon => "COMMA_COLON",
            FloorDivColon => "FLOOR_DIV_COLON",

            Colon => "COLON",
            EqEq => "EQ_EQ",
            EqDot => "EQ_DOT",
            NotEq => "NOT_EQ",
            NotEqDot => "NOT_EQ_DOT",
            Lt => "LT",
            Le => "LE",
            Gt => "GT",
            Ge => "GE",

            Hash => "HASH",
            Pipe => "PIPE",
            PipeDot => "PIPE_DOT",
            PipePipe => "PIPE_PIPE",
            PipePipeDot => "PIPE_PIPE_DOT",
            RangeOp => "RANGE",
            RangeIncOp => "RANGE_INC",
            LParen => "L_PAREN",
            RParen => "R_PAREN",
            LBrack => "L_BRACK",
            RBrack => "R_BRACK",
            LBrace => "L_BRACE",
            RBrace => "R_BRACE",
            Semicolon => "SEMI",
            Comma => "COMMA_TOK",
            Bang => "BANG",
            ScriptLine => "SCRIPT_LINE",
            ErrorTok => "ERROR_TOK",
            __LastToken => "__LAST_TOKEN",
            Root => "ROOT",
            Block => "BLOCK",
            Shebang => "SHEBANG",
            ScriptDirective => "SCRIPT_DIRECTIVE",
            LiteralExpr => "LITERAL_EXPR",
            VarExpr => "VAR_EXPR",
            OuterVarExpr => "OUTER_VAR_EXPR",
            BinaryExpr => "BINARY_EXPR",
            UnaryExpr => "UNARY_EXPR",
            ComparisonChainExpr => "COMPARISON_CHAIN_EXPR",
            AssignExpr => "ASSIGN_EXPR",
            OuterAssignExpr => "OUTER_ASSIGN_EXPR",
            UnpackAssignExpr => "UNPACK_ASSIGN_EXPR",
            IndexAssignExpr => "INDEX_ASSIGN_EXPR",
            MutatingIndexExpr => "MUTATING_INDEX_EXPR",
            MutatingIndexAssignExpr => "MUTATING_INDEX_ASSIGN_EXPR",
            RangeExpr => "RANGE_EXPR",
            ListExpr => "LIST_EXPR",
            DictExpr => "DICT_EXPR",
            DictPair => "DICT_PAIR",

            ParenExpr => "PAREN_EXPR",
            PostfixExpr => "POSTFIX_EXPR",
            LazyBoolExpr => "LAZY_BOOL_EXPR",
            NamedArgExpr => "NAMED_ARG_EXPR",
            ArgList => "ARG_LIST",
            FStringExpr => "FSTRING_EXPR",
            CondExpr => "COND_EXPR",
            CondDotExpr => "COND_DOT_EXPR",
            CondChainExpr => "COND_CHAIN_EXPR",
            WLoopExpr => "W_LOOP_EXPR",
            NLoopExpr => "N_LOOP_EXPR",
            BlockExpr => "BLOCK_EXPR",
            FunctionExpr => "FUNCTION_EXPR",
            ParamList => "PARAM_LIST",
            Param => "PARAM",
            ReturnExpr => "RETURN_EXPR",
            DebugExpr => "DEBUG_EXPR",
            PauseExpr => "PAUSE_EXPR",
            TryExpr => "TRY_EXPR",
            SymbolicExpr => "SYMBOLIC_EXPR",
            BreakExpr => "BREAK_EXPR",
            ContinueExpr => "CONTINUE_EXPR",

            EllipsisExpr => "ELLIPSIS_EXPR",
            PipeExpr => "PIPE_EXPR",
            PipeTapExpr => "PIPE_TAP_EXPR",
            ErrorNode => "ERROR",
        }
    }
}

impl std::fmt::Display for SyntaxKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_node_partition_is_total() {
        // Each variant belongs to exactly one partition.
        let all: &[SyntaxKind] = &[
            SyntaxKind::Whitespace,
            SyntaxKind::Newline,
            SyntaxKind::Comment,
            SyntaxKind::IntLit,
            SyntaxKind::Ident,
            SyntaxKind::Plus,
            SyntaxKind::Colon,
            SyntaxKind::ErrorTok,
            SyntaxKind::Root,
            SyntaxKind::Block,
            SyntaxKind::BinaryExpr,
            SyntaxKind::ErrorNode,
        ];
        for k in all {
            let is_sentinel = matches!(k, SyntaxKind::__LastToken);
            let count = u8::from(k.is_token()) + u8::from(k.is_node()) + u8::from(is_sentinel);
            assert_eq!(
                count,
                1,
                "{k:?} is_token={} is_node={}",
                k.is_token(),
                k.is_node()
            );
        }
    }

    #[test]
    fn trivia_is_subset_of_tokens() {
        for k in [
            SyntaxKind::Whitespace,
            SyntaxKind::Newline,
            SyntaxKind::Comment,
        ] {
            assert!(k.is_trivia());
            assert!(k.is_token());
            assert!(!k.is_node());
        }
        for k in [SyntaxKind::Ident, SyntaxKind::Plus, SyntaxKind::Root] {
            assert!(!k.is_trivia());
        }
    }

    #[test]
    fn names_are_distinct() {
        // `name()` itself gives full enum coverage.
        let mut names = std::collections::HashSet::new();
        for k in [
            SyntaxKind::Whitespace,
            SyntaxKind::IntLit,
            SyntaxKind::Plus,
            SyntaxKind::Root,
            SyntaxKind::BinaryExpr,
            SyntaxKind::ErrorTok,
            SyntaxKind::ErrorNode,
        ] {
            assert!(names.insert(k.name()), "duplicate name: {}", k.name());
        }
    }
}
