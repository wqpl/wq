use colored::{Color, Colorize};

use crate::value::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum AstNode {
    Literal(Value),

    /// Variable reference
    Variable(String),

    BinaryOp {
        left: Box<AstNode>,
        operator: BinaryOperator,
        right: Box<AstNode>,
    },

    UnaryOp {
        operator: UnaryOperator,
        operand: Box<AstNode>,
    },

    Assignment {
        name: String,
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

    /// While loop
    WhileLoop {
        condition: Box<AstNode>,
        body: Box<AstNode>,
    },

    /// Numeric for loop, exposes counter `_n`
    ForLoop {
        count: Box<AstNode>,
        body: Box<AstNode>,
    },

    Break,
    Continue,
    Return(Option<Box<AstNode>>),
    Assert(Box<AstNode>),
    Try(Box<AstNode>),

    /// Sequence of statements
    Block(Vec<AstNode>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Power,
    Divide,
    DivideDot,
    Modulo,
    ModuloDot,
    Equal,
    NotEqual,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOperator {
    Negate,
    Count, // '#' operator
}

use std::fmt;

// --- helpers ---------------------------------------------------------------

const COLORS: [Color; 12] = [
    // Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    // Color::White,
    // Color::BrightBlack,
    Color::BrightRed,
    Color::BrightGreen,
    Color::BrightYellow,
    Color::BrightBlue,
    Color::BrightMagenta,
    Color::BrightCyan,
    // Color::BrightWhite,
];

fn parens(depth: usize) -> (String, String, Color) {
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

fn fmt_value<V: fmt::Debug>(v: &V) -> String {
    format!("{v:?}")
}

fn bin_op(op: &BinaryOperator) -> &'static str {
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
        Equal => "=",
        NotEqual => "~",
        LessThan => "<",
        LessThanOrEqual => "<=",
        GreaterThan => ">",
        GreaterThanOrEqual => ">=",
    }
}

fn un_op(op: &UnaryOperator) -> &'static str {
    use UnaryOperator::*;
    match op {
        Negate => "-",
        Count => "#",
    }
}

// --- printer ---------------------------------------------------------------

impl AstNode {
    pub fn sexpr(&self) -> String {
        self.sexpr_with_depth(0)
    }

    fn sexpr_with_depth(&self, depth: usize) -> String {
        use AstNode::*;
        match self {
            Literal(v) => fmt_value(v),
            Variable(name) => atom_ident(name),

            UnaryOp { operator, operand } => group(
                depth,
                [
                    un_op(operator).to_string(),
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
                    bin_op(operator).to_string(),
                    left.sexpr_with_depth(depth + 1),
                    right.sexpr_with_depth(depth + 1),
                ],
            ),

            Assignment { name, value } => group(
                depth,
                [
                    "ASSIGN".into(),
                    atom_ident(name),
                    value.sexpr_with_depth(depth + 1),
                ],
            ),

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
                    "POSTFIX!"
                } else {
                    "POSTFIX"
                };
                let mut parts = Vec::with_capacity(items.len() + 2);
                parts.push(head.into());
                parts.push(object.sexpr_with_depth(depth + 1));
                parts.extend(items.iter().map(|i| i.sexpr_with_depth(depth + 1)));
                group(depth, parts)
            }

            // Renders as: (foo arg1 arg2) — classic s-expr call
            Call { name, args } => {
                let mut parts = Vec::with_capacity(args.len() + 1);
                parts.push(atom_ident(name));
                parts.extend(args.iter().map(|a| a.sexpr_with_depth(depth + 1)));
                group(depth, parts)
            }

            // Renders as: ((<expr>) arg1 arg2)
            CallAnonymous { object, args } => {
                let mut parts = Vec::with_capacity(args.len() + 1);
                parts.push(object.sexpr_with_depth(depth + 1));
                parts.extend(args.iter().map(|a| a.sexpr_with_depth(depth + 1)));
                group(depth, parts)
            }

            Index { object, index } => group(
                depth,
                [
                    "IDX".into(),
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
                    "IDX-ASSIGN".into(),
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

            WhileLoop { condition, body } => group(
                depth,
                [
                    "W-LOOP".into(),
                    condition.sexpr_with_depth(depth + 1),
                    body.sexpr_with_depth(depth + 1),
                ],
            ),

            ForLoop { count, body } => group(
                depth,
                [
                    "N-LOOP".into(),
                    count.sexpr_with_depth(depth + 1),
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

            Assert(expr) => group(depth, ["@a".into(), expr.sexpr_with_depth(depth + 1)]),
            Try(expr) => group(depth, ["@t".into(), expr.sexpr_with_depth(depth + 1)]),

            Block(stmts) => {
                let mut parts = Vec::with_capacity(stmts.len() + 1);
                parts.push("BLOCK".into());
                parts.extend(stmts.iter().map(|s| s.sexpr_with_depth(depth + 1)));
                group(depth, parts)
            }
        }
    }
}

// make it printable with println!("{}", node);
impl fmt::Display for AstNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.sexpr())
    }
}
