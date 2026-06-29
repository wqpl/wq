//! Concrete syntax kinds for the green/red tree.
//!
//! Every leaf in the green tree is a token kind; every internal node is a node
//! kind. Trivia kinds are tokens, but they are tagged so the formatter and
//! lowering pass can skip them when building the AST.
//!
//! The numeric layout matters: token kinds are kept contiguously in the lower
//! half of the enum so [`SyntaxKind::is_token`] can be expressed as a single
//! comparison. This keeps the helper inlinable without a giant `match`.
//!
//! Adding a new kind:
//! 1. If it is a token, insert it before [`SyntaxKind::__LAST_TOKEN`].
//! 2. If it is a node, insert it after [`SyntaxKind::__LAST_TOKEN`].
//! 3. Update the trivia helper if it is whitespace-like.
//!
//! The leading `__LAST_TOKEN` sentinel is private; consumers should use
//! [`SyntaxKind::is_token`] / [`SyntaxKind::is_node`] /
//! [`SyntaxKind::is_trivia`].

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum SyntaxKind {
    // ===== trivia tokens =====
    Whitespace,
    Newline,
    Comment,

    // ===== literal tokens =====
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
    /// Entire `@f"..."` literal kept as one token in this phase. A future
    /// refinement will split it into a node containing text + interpolation
    /// children, so the formatter can re-flow long strings.
    FString,

    // ===== identifiers =====
    Ident,
    Apostrophe,
    Ellipsis,

    // ===== `@`-keywords =====
    AtAssert,
    AtBreak,
    AtContinue,
    AtReturn,
    AtDebug,
    AtPause,
    AtDepth,
    AtSymbolic,
    AtTry,

    // ===== `$`-keywords =====
    Dollar,
    DollarDot,
    DollarDollar,

    // ===== arithmetic operators =====
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

    // ===== binary operators =====
    Shl,
    Shr,

    // ===== augmented assignments =====
    PlusColon,
    MinusColon,
    StarColon,
    SlashColon,
    SlashDotColon,
    PercentColon,
    PowerColon,
    PowerDotColon,
    CommaColon,
    ShlColon,
    ShrColon,
    FloorDivColon,

    Colon,

    // ===== comparison =====
    EqEq,
    EqDot,
    NotEq,
    NotEqDot,
    Lt,
    Le,
    Gt,
    Ge,

    // ===== pipes / range / misc =====
    Hash,
    Pipe,
    PipeDot,
    PipePipe,
    PipePipeDot,
    RangeOp,
    RangeIncOp,

    // ===== brackets / punctuation =====
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

    /// A token the lexer could not classify; emitted in recovery mode so the
    /// CST still covers every byte of source.
    ErrorTok,

    /// Sentinel for the boundary between token and node kinds. Not a real
    /// kind -- never emitted; never matched on by user code.
    #[doc(hidden)]
    __LastToken,

    // ===== nodes =====
    /// Top of the tree. Every parse produces exactly one of these.
    Root,
    Block,
    Shebang,
    ScriptDirective,

    // ----- expression-shaped nodes -----
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
    AssertExpr,
    DebugExpr,
    PauseExpr,
    TryExpr,
    SymbolicExpr,
    BreakExpr,
    ContinueExpr,
    EllipsisExpr,
    PipeExpr,
    PipeTapExpr,

    /// Error-recovery node. Holds whatever tokens the parser was forced to
    /// skip; the lowering pass turns these into [`AstNode::Error`].
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

    /// True when this kind is whitespace-like trivia: it carries no semantic
    /// meaning and is dropped by lowering.
    #[inline]
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            SyntaxKind::Whitespace | SyntaxKind::Newline | SyntaxKind::Comment
        )
    }

    /// Short, stable, human-readable name. Used in debug output and snapshot
    /// tests; do not rely on it for any semantic decision -- match on the
    /// variant instead.
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
            Apostrophe => "APOS",
            Ellipsis => "ELLIPSIS",
            AtAssert => "AT_ASSERT",
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

            Shl => "SHL",
            Shr => "SHR",
            PlusColon => "PLUS_COLON",
            MinusColon => "MINUS_COLON",
            StarColon => "STAR_COLON",
            SlashColon => "SLASH_COLON",
            SlashDotColon => "SLASH_DOT_COLON",
            PercentColon => "PERCENT_COLON",
            PowerColon => "POWER_COLON",
            PowerDotColon => "POWER_DOT_COLON",
            CommaColon => "COMMA_COLON",
            ShlColon => "SHL_COLON",
            ShrColon => "SHR_COLON",
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
            AssertExpr => "ASSERT_EXPR",
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
        // Every variant must satisfy exactly one of (is_token, is_node, ==__LastToken).
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
            let count = (k.is_token() as u8) + (k.is_node() as u8) + (is_sentinel as u8);
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
        // Walk a representative slice; full enum coverage is checked by the
        // exhaustive match in `name()`.
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
