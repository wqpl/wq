use std::fmt;

use crate::{
    colored::{Color, Colorize},
    value::Value,
};

#[derive(Debug, Clone, PartialEq)]
pub enum UnpackItem {
    Bind(String),
    Ellipsis,
    Skip,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstNode {
    Literal(Value),
    Variable(String),
    BinaryOp {
        left: Box<AstNode>,
        operator: BinaryOperator,
        right: Box<AstNode>,
    },
    /// Chain of comparison operators like `a < b <= c`
    ComparisonChain {
        first: Box<AstNode>,
        rest: Vec<(BinaryOperator, AstNode)>,
    },
    UnaryOp {
        operator: UnaryOperator,
        operand: Box<AstNode>,
    },
    Range {
        start: Box<AstNode>,
        end: Box<AstNode>,
        step: Option<Box<AstNode>>,
        inclusive: bool,
    },
    Assignment {
        name: String,
        value: Box<AstNode>,
    },
    /// Ellipsis (for unpack patterns)
    Ellipsis,
    /// Unpack assignment like `(x;y):a` or `(x;y):(y;x)`
    UnpackAssign {
        pattern: Vec<UnpackItem>,
        value: Box<AstNode>,
    },
    /// List construction
    List(Vec<AstNode>),
    /// Dictionary construction
    Dict(Vec<(String, AstNode)>),
    /// Generic postfix expression
    Postfix {
        object: Box<AstNode>,
        items: Vec<AstNode>,
        explicit_call: bool,
    },
    /// Function call
    Call {
        name: String,
        args: Vec<AstNode>,
    },
    CallAnonymous {
        object: Box<AstNode>,
        args: Vec<AstNode>,
    },
    /// Index access
    Index {
        object: Box<AstNode>,
        index: Box<AstNode>,
    },
    /// Index assignment like `a[1]:3`
    IndexAssign {
        object: Box<AstNode>,
        index: Box<AstNode>,
        value: Box<AstNode>,
    },
    /// Function def
    Function {
        params: Option<Vec<String>>, // None for implicit params (x, y, z)
        body: Box<AstNode>,
    },
    /// Conditional expression
    Conditional {
        condition: Box<AstNode>,
        true_branch: Box<AstNode>,
        false_branch: Option<Box<AstNode>>,
    },
    WLoop {
        condition: Box<AstNode>,
        body: Box<AstNode>,
    },
    NLoop {
        count: Box<AstNode>,
        body: Box<AstNode>,
    },
    FLoop {
        iterable: Box<AstNode>,
        body: Box<AstNode>,
    },
    Break,
    Continue,
    Return(Option<Box<AstNode>>),
    Try(Box<AstNode>),
    /// Sequence of statements
    Block(Vec<AstNode>),
    /// Block expression from B[...]
    BlockExpr(Vec<AstNode>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Power,
    Divide,
    DivideDot,
    Modulo,
    ModuloDot,
    Matmul,

    Equal,
    NotEqual,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    Cat, // ,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOperator {
    Negate,
    Count, // #
}

fn parens(depth: usize) -> (String, String, Color) {
    const COLORS: [Color; 12] = [
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::BrightRed,
        Color::BrightGreen,
        Color::BrightYellow,
        Color::BrightBlue,
        Color::BrightMagenta,
        Color::BrightCyan,
    ];

    let color = COLORS[depth % COLORS.len()];
    (
        "(".color(color).bold().to_string(),
        ")".color(color).bold().to_string(),
        color,
    )
}

fn group(depth: usize, parts: impl IntoIterator<Item = String>) -> String {
    let (open, close, color) = parens(depth);
    let mut out = String::new();
    out.push_str(&open);
    let mut first = true;
    for p in parts {
        if !first {
            out.push(' ');
        }
        let s = p.as_str();
        if first {
            // color the first element the same as parentheses
            #[cfg_attr(target_arch = "wasm32", allow(clippy::unnecessary_to_owned))]
            out.push_str(&s.color(color).bold().to_string());
        } else {
            out.push_str(s);
        }
        first = false;
    }
    out.push_str(&close);
    out
}

fn atom_ident(s: &str) -> String {
    // bare if simple symbol, otherwise quoted like Rust's Debug string
    if s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        s.to_string()
    } else {
        format!("{s:?}")
    }
}

pub fn binary_op_display(op: &BinaryOperator) -> &'static str {
    use BinaryOperator::*;
    match op {
        Add => "+",
        Subtract => "-",
        Multiply => "*",
        Power => "^",
        Divide => "/",
        DivideDot => "/.",
        Modulo => "%",
        ModuloDot => "%.",
        Matmul => "**",

        Equal => "=",
        NotEqual => "~",
        LessThan => "<",
        LessThanOrEqual => "<=",
        GreaterThan => ">",
        GreaterThanOrEqual => ">=",
        Cat => ",",
    }
}

pub fn unary_op_display(op: &UnaryOperator) -> &'static str {
    use UnaryOperator::*;
    match op {
        Negate => "-",
        Count => "#",
    }
}

// printer ================================================================

impl AstNode {
    pub fn sexpr(&self) -> String {
        self.sexpr_with_depth(0)
    }

    fn sexpr_with_depth(&self, depth: usize) -> String {
        use AstNode::*;
        match self {
            Literal(v) => format!("{v:?}"),
            Variable(name) => atom_ident(name),
            UnaryOp { operator, operand } => group(
                depth,
                [
                    unary_op_display(operator).to_string(),
                    operand.sexpr_with_depth(depth + 1),
                ],
            ),
            BinaryOp {
                left,
                operator,
                right,
            } => group(
                depth,
                [
                    binary_op_display(operator).to_string(),
                    left.sexpr_with_depth(depth + 1),
                    right.sexpr_with_depth(depth + 1),
                ],
            ),
            ComparisonChain { first, rest } => {
                let mut parts = Vec::with_capacity(rest.len() * 2 + 2);
                parts.push("CMP-CHAIN".into());
                parts.push(first.sexpr_with_depth(depth + 1));
                for (op, node) in rest {
                    parts.push(binary_op_display(op).to_string());
                    parts.push(node.sexpr_with_depth(depth + 1));
                }
                group(depth, parts)
            }
            Range {
                start,
                end,
                step,
                inclusive,
            } => {
                let mut parts = vec![
                    if *inclusive {
                        "RANGE=".into()
                    } else {
                        "RANGE".into()
                    },
                    start.sexpr_with_depth(depth + 1),
                    end.sexpr_with_depth(depth + 1),
                ];
                if let Some(step) = step {
                    parts.push(step.sexpr_with_depth(depth + 1));
                }
                group(depth, parts)
            }
            Assignment { name, value } => group(
                depth,
                [
                    "ASSIGN".into(),
                    atom_ident(name),
                    value.sexpr_with_depth(depth + 1),
                ],
            ),
            Ellipsis => group(depth, ["...".into()]),
            UnpackAssign { pattern, value } => {
                let mut parts = Vec::with_capacity(pattern.len() + 2);
                parts.push("UNPACK-ASSIGN".into());
                // Show pattern as a grouped list
                let pat_group = group(
                    depth + 1,
                    pattern.iter().map(|p| match p {
                        UnpackItem::Bind(n) => atom_ident(n),
                        UnpackItem::Skip => "_".into(),
                        UnpackItem::Ellipsis => "...".into(),
                    }),
                );
                parts.push(pat_group);
                parts.push(value.sexpr_with_depth(depth + 1));
                group(depth, parts)
            }
            List(items) => {
                let mut parts = Vec::with_capacity(items.len() + 1);
                parts.push("LIST".into());
                parts.extend(items.iter().map(|i| i.sexpr_with_depth(depth + 1)));
                group(depth, parts)
            }
            Dict(kvs) => {
                let mut parts = Vec::with_capacity(kvs.len() + 1);
                parts.push("DICT".into());
                // each pair gets its own (/[/{ … … })
                for (k, v) in kvs {
                    parts.push(group(
                        depth + 1,
                        [atom_ident(k), v.sexpr_with_depth(depth + 2)],
                    ));
                }
                group(depth, parts)
            }
            Postfix {
                object,
                items,
                explicit_call,
            } => {
                let head = if *explicit_call {
                    "POSTFIX*"
                } else {
                    "POSTFIX"
                };
                let mut parts = Vec::with_capacity(items.len() + 2);
                parts.push(head.into());
                parts.push(object.sexpr_with_depth(depth + 1));
                parts.extend(items.iter().map(|i| i.sexpr_with_depth(depth + 1)));
                group(depth, parts)
            }
            Call { name, args } => {
                let mut parts = Vec::with_capacity(args.len() + 1);
                parts.push(atom_ident(name));
                parts.extend(args.iter().map(|a| a.sexpr_with_depth(depth + 1)));
                group(depth, parts)
            }
            CallAnonymous { object, args } => {
                let mut parts = Vec::with_capacity(args.len() + 1);
                parts.push(object.sexpr_with_depth(depth + 1));
                parts.extend(args.iter().map(|a| a.sexpr_with_depth(depth + 1)));
                group(depth, parts)
            }
            Index { object, index } => group(
                depth,
                [
                    "INDEX".into(),
                    object.sexpr_with_depth(depth + 1),
                    index.sexpr_with_depth(depth + 1),
                ],
            ),
            IndexAssign {
                object,
                index,
                value,
            } => group(
                depth,
                [
                    "INDEX-ASSIGN".into(),
                    object.sexpr_with_depth(depth + 1),
                    index.sexpr_with_depth(depth + 1),
                    value.sexpr_with_depth(depth + 1),
                ],
            ),
            Function { params, body } => {
                let params_grp = match params {
                    Some(ps) => group(depth + 1, ps.iter().map(|p| atom_ident(p))),
                    None => group(depth + 1, ["implicit".into()]),
                };
                group(
                    depth,
                    ["FN".into(), params_grp, body.sexpr_with_depth(depth + 1)],
                )
            }
            Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                let mut parts = vec![
                    "IF".into(),
                    condition.sexpr_with_depth(depth + 1),
                    true_branch.sexpr_with_depth(depth + 1),
                ];
                if let Some(fb) = false_branch {
                    parts.push(fb.sexpr_with_depth(depth + 1));
                }
                group(depth, parts)
            }
            WLoop { condition, body } => group(
                depth,
                [
                    "W-LOOP".into(),
                    condition.sexpr_with_depth(depth + 1),
                    body.sexpr_with_depth(depth + 1),
                ],
            ),
            NLoop { count, body } => group(
                depth,
                [
                    "N-LOOP".into(),
                    count.sexpr_with_depth(depth + 1),
                    body.sexpr_with_depth(depth + 1),
                ],
            ),
            FLoop { iterable, body } => group(
                depth,
                [
                    "F-LOOP".into(),
                    iterable.sexpr_with_depth(depth + 1),
                    body.sexpr_with_depth(depth + 1),
                ],
            ),
            Break => group(depth, ["@b".into()]),
            Continue => group(depth, ["@c".into()]),
            Return(opt) => {
                let mut parts = vec!["@r".into()];
                if let Some(v) = opt {
                    parts.push(v.sexpr_with_depth(depth + 1));
                }
                group(depth, parts)
            }
            Try(expr) => group(depth, ["@t".into(), expr.sexpr_with_depth(depth + 1)]),
            Block(stmts) => {
                let mut parts = Vec::with_capacity(stmts.len() + 1);
                parts.push("BLOCK".into());
                parts.extend(stmts.iter().map(|s| s.sexpr_with_depth(depth + 1)));
                group(depth, parts)
            }
            BlockExpr(stmts) => {
                let mut parts = Vec::with_capacity(stmts.len() + 1);
                parts.push("B".into());
                parts.extend(stmts.iter().map(|s| s.sexpr_with_depth(depth + 1)));
                group(depth, parts)
            }
        }
    }
}

impl fmt::Display for AstNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.sexpr())
    }
}
