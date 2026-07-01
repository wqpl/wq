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
    BoolAnd, // A[...]
    BoolOr,  // O[...]
    Cat,     // , (augmented-assignment marker only; compiled to Instruction::Cat)
    BitAnd,
    BitOr,
    Shl,
    Shr,
    BitXor,
    FloorDiv,
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

        BoolAnd => "A",
        BoolOr => "O",
        BitAnd => "band",
        BitOr => "bor",
        Shl => "shl",
        Shr => "shr",
        BitXor => "xor",
        FloorDiv => "/%",
        Lt => "<",
        Lte => "<=",
        Gt => ">",
        Gte => ">=",
        Cat => ",",
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
