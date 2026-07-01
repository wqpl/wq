mod operator;
mod print;

pub use operator::{BinaryOperator, UnaryOperator};
pub(crate) use operator::{binary_op_display, unary_op_display};

use crate::value::Value;
use crate::wqerror::WqError;

/// Source span: `None` means synthetic (not from source text).
pub(crate) type AstSpan = Option<(usize, usize)>;

#[derive(Debug, Clone, PartialEq)]
pub enum Parameter {
    Pos {
        name: String,
        span: AstSpan,
    },
    Named {
        name: String,
        span: AstSpan,
        default: Option<Box<AstNode>>,
    },
}

impl Parameter {
    pub(crate) fn name(&self) -> &str {
        match self {
            Parameter::Pos { name, .. } | Parameter::Named { name, .. } => name,
        }
    }

    pub(crate) fn span(&self) -> AstSpan {
        match self {
            Parameter::Pos { span, .. } | Parameter::Named { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstNode {
    Error(WqError, AstSpan),
    Literal(Value, AstSpan),
    Variable(String, AstSpan),
    OuterVariable(String, AstSpan),
    BinaryOp {
        left: Box<AstNode>,
        operator: BinaryOperator,
        right: Box<AstNode>,
        span: AstSpan,
    },
    /// Chain of comparison operators like `a < b <= c`
    ComparisonChain {
        first: Box<AstNode>,
        rest: Vec<(BinaryOperator, AstNode)>,
        span: AstSpan,
    },
    UnaryOp {
        operator: UnaryOperator,
        operand: Box<AstNode>,
        span: AstSpan,
    },
    Group {
        expr: Box<AstNode>,
        span: AstSpan,
    },
    Range {
        start: Box<AstNode>,
        end: Box<AstNode>,
        step: Option<Box<AstNode>>,
        inclusive: bool,
        span: AstSpan,
    },
    Assignment {
        name: String,
        op: Option<BinaryOperator>,
        value: Box<AstNode>,
        span: AstSpan,
        name_span: AstSpan,
    },
    OuterAssignment {
        name: String,
        op: Option<BinaryOperator>,
        value: Box<AstNode>,
        span: AstSpan,
        name_span: AstSpan,
    },
    /// Unpack assignment like (x;y):rhs
    UnpackAssignment {
        lhs: Vec<AstNode>,
        op: Option<BinaryOperator>,
        rhs: Box<AstNode>,
        span: AstSpan,
    },
    /// Ellipsis (for unpack patterns)
    Ellipsis(AstSpan),
    /// List construction
    List(Vec<AstNode>, AstSpan),
    /// N-ary concatenation (comma-separated items)
    Cat(Vec<AstNode>, AstSpan),
    /// Dictionary construction
    Dict(Vec<(String, AstNode)>, AstSpan),
    /// Generic postfix expression
    Postfix {
        object: Box<AstNode>,
        items: Vec<AstNode>,
        explicit_call: bool,
        depth: Option<i64>,
        span: AstSpan,
    },
    /// Placeholder used while compiling tap-pipe effects.
    PipeInput,
    /// Pipe expression (not yet lowered).
    Pipe {
        input: Box<AstNode>,
        effect: Box<AstNode>,
        kind: PipeKind,
        span: AstSpan,
    },
    /// Pipe that runs an effect but yields the original input.
    PipeTap {
        input: Box<AstNode>,
        effect: Box<AstNode>,
        span: AstSpan,
    },
    /// Function call
    CallName {
        name: String,
        args: Vec<AstNode>,
        span: AstSpan,
        name_span: AstSpan,
    },
    CallAnonymous {
        object: Box<AstNode>,
        args: Vec<AstNode>,
        span: AstSpan,
    },
    /// Index access
    Index {
        object: Box<AstNode>,
        index: Box<AstNode>,
        span: AstSpan,
    },
    /// Index assignment like `a[1]:3`
    IndexAssign {
        object: Box<AstNode>,
        index: Box<AstNode>,
        op: Option<BinaryOperator>,
        value: Box<AstNode>,
        span: AstSpan,
    },
    /// Mutating index access: `x[!]` (pop), `x[!i]` (remove)
    MutatingIndex {
        object: Box<AstNode>,
        index: Box<AstNode>,
        span: AstSpan,
    },
    /// Mutating index assignment: `x[!]:v` (insert between), `x[!i]:v` (insert
    /// at)
    MutatingIndexAssign {
        object: Box<AstNode>,
        index: Box<AstNode>,
        value: Box<AstNode>,
        span: AstSpan,
    },
    /// Named argument at a call site: `name: expr
    NamedArg {
        name: String,
        value: Box<AstNode>,
        span: AstSpan,
    },
    /// Function def
    Function {
        params: Option<Vec<Parameter>>, // None for implicit params (x, y, z)
        ref_capture: bool,
        body: Box<AstNode>,
        span: AstSpan,
    },
    /// Conditional expression
    Conditional {
        condition: Box<AstNode>,
        true_branch: Box<AstNode>,
        false_branch: Option<Box<AstNode>>,
        span: AstSpan,
    },
    /// Conditional dot from $.[...] (no false branch)
    ConditionalDot {
        condition: Box<AstNode>,
        true_branch: Box<AstNode>,
        span: AstSpan,
    },
    /// Conditional chain from $$[...]
    ConditionalChain {
        pairs: Vec<(AstNode, AstNode)>,
        default_branch: Box<AstNode>,
        span: AstSpan,
    },
    WLoop {
        condition: Box<AstNode>,
        body: Box<AstNode>,
        span: AstSpan,
    },
    NLoop {
        count: Box<AstNode>,
        body: Box<AstNode>,
        span: AstSpan,
    },
    Break(AstSpan),
    Continue(AstSpan),
    Return(Option<Box<AstNode>>, AstSpan),
    Assert {
        expr: Box<AstNode>,
        span: AstSpan,
    },
    Debug {
        expr: Box<AstNode>,
        span: AstSpan,
    },
    Pause {
        expr: Option<Box<AstNode>>,
        span: AstSpan,
    },
    Try(Box<AstNode>, AstSpan),
    /// Sequence of statements
    Block(Vec<AstNode>, AstSpan),
    /// Block expression from `[...]` or legacy `B[...]`.
    BlockExpr(Vec<AstNode>, AstSpan),
    /// F-string literal (@f"...{expr}...")
    FString {
        parts: Vec<FStringPart>,
        span: AstSpan,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum FStringPart {
    Text(String),
    Expr {
        expr: AstNode,
        spec: Option<String>,
        encoded_spec: Option<String>,
        spec_exprs: Vec<AstNode>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeKind {
    Pipe,
    PipeDot,
    PipePipe,
    PipePipeDot,
}

impl AstNode {
    pub(crate) fn span(&self) -> AstSpan {
        use AstNode::*;
        match self {
            Error(_, s) | Literal(_, s) | Variable(_, s) | OuterVariable(_, s) => *s,
            BinaryOp { span, .. }
            | ComparisonChain { span, .. }
            | Range { span, .. }
            | Assignment { span, .. }
            | OuterAssignment { span, .. }
            | UnpackAssignment { span, .. }
            | Postfix { span, .. }
            | Pipe { span, .. }
            | PipeTap { span, .. }
            | CallName { span, .. }
            | CallAnonymous { span, .. }
            | Index { span, .. }
            | IndexAssign { span, .. }
            | MutatingIndex { span, .. }
            | MutatingIndexAssign { span, .. }
            | Function { span, .. }
            | Conditional { span, .. }
            | ConditionalDot { span, .. }
            | ConditionalChain { span, .. }
            | WLoop { span, .. }
            | NLoop { span, .. }
            | Assert { span, .. }
            | Debug { span, .. }
            | Pause { span, .. }
            | NamedArg { span, .. }
            | FString { span, .. }
            | UnaryOp { span, .. }
            | Group { span, .. } => *span,
            Cat(_, span) | List(_, span) | Dict(_, span) | Block(_, span) => *span,
            BlockExpr(_, span) => *span,
            Return(_, span) | Try(_, span) | Ellipsis(span) | Break(span) | Continue(span) => *span,
            PipeInput => None,
        }
    }
}
