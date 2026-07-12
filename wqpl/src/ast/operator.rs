#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Power,
    PowerDot,
    Divide,
    DivideDot,
    Modulo,
    Matmul,

    Equal,
    EqualDot,
    NotEqual,
    NotEqualDot,
    Lt,
    Lte,
    Gt,
    Gte,
    Cat, // aug-assign marker only
    BitAnd,
    BitOr,
    Shl,
    Shr,
    BitXor,
    FloorDiv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoolOperator {
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOperator {
    Negate,
    Count, // #
    Not,
}

pub(crate) fn binary_op_display(op: &BinaryOperator) -> &'static str {
    use BinaryOperator::*;
    match op {
        Add => "+",
        Subtract => "-",
        Multiply => "*",
        Power => "^",
        PowerDot => "^.",
        Divide => "/",
        DivideDot => "/.",
        Modulo => "%",
        Matmul => "**",

        Equal => "=",
        EqualDot => "=.",
        NotEqual => "~",
        NotEqualDot => "~.",

        BitAnd => "band",
        BitOr => "bor",
        Shl => "shl",
        Shr => "shr",
        BitXor => "bxor",
        FloorDiv => "/%",
        Lt => "<",
        Lte => "<=",
        Gt => ">",
        Gte => ">=",
        Cat => ",",
    }
}

pub(crate) fn bool_op_display(op: &BoolOperator) -> &'static str {
    match op {
        BoolOperator::And => "and",
        BoolOperator::Or => "or",
    }
}

pub(crate) fn unary_op_display(op: &UnaryOperator) -> &'static str {
    use UnaryOperator::*;
    match op {
        Negate => "-",
        Count => "#",
        Not => "~",
    }
}
