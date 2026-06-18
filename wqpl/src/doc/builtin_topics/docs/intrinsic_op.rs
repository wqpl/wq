use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const OP_ADD_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Broadcast addition over a list",
    code: "+[1;(2;3)]",
    expectation: ExampleExpectation::ResultContains("(3;4)"),
}];

const OP_SUB_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Subtract left to right",
    code: "-[10;3;2]",
    expectation: ExampleExpectation::ResultContains("5"),
}];

const OP_MUL_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Multiply left to right",
    code: "*[2;3;4]",
    expectation: ExampleExpectation::ResultContains("24"),
}];

const OP_DIV_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Divide left to right",
    code: "/[20;2;5]",
    expectation: ExampleExpectation::ResultContains("2.0"),
}];

const OP_DIV_DOT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Divide exactly",
    code: "/.[1;3]",
    expectation: ExampleExpectation::ResultContains("1/3"),
}];

const OP_MOD_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Take a remainder",
    code: "%[17;5]",
    expectation: ExampleExpectation::ResultContains("2"),
}];

const OP_FLOORDIV_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Floor-divide integers",
    code: "/%[7;2]",
    expectation: ExampleExpectation::ResultContains("3"),
}];

const OP_POWER_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Raise to a power",
    code: "^[2;3]",
    expectation: ExampleExpectation::ResultContains("8"),
}];

const OP_POWER_DOT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Raise to a power exactly",
    code: "^.[2;-3]",
    expectation: ExampleExpectation::ResultContains("1/8"),
}];

const OP_MATMUL_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Compute a dot product",
    code: "**[(1;2);(3;4)]",
    expectation: ExampleExpectation::ResultContains("11"),
}];

const OP_EQUAL_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Compare whole values",
    code: "=[(1;2);(1;2)]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const OP_EQUAL_DOT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Compare leaves with broadcasting",
    code: "=.[(1;2);1]",
    expectation: ExampleExpectation::ResultContains("(T;F)"),
}];

const OP_TILDE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Compare whole values for inequality",
    code: "~[1;2]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const OP_TILDE_DOT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Compare leaves for inequality",
    code: "~.[(1;2);(1;3)]",
    expectation: ExampleExpectation::ResultContains("(F;T)"),
}];

const OP_LT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Check an increasing chain",
    code: "<[1;2;3]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const OP_LTE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Check a nondecreasing chain",
    code: "<=[1;1;2]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const OP_GT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Check a decreasing chain",
    code: ">[3;2;1]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const OP_GTE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Check a nonincreasing chain",
    code: ">=[3;3;2]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const OP_CAT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Concatenate values",
    code: ",[(1;2);3;(4;5)]",
    expectation: ExampleExpectation::ResultContains("(1;2;3;4;5)"),
}];

const OP_SHARP_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Count a list",
    code: "#[(10;20;30)]",
    expectation: ExampleExpectation::ResultContains("3"),
}];

const OP_BOOL_AND_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Fold boolean and",
    code: "&|[T;F;T]",
    expectation: ExampleExpectation::ResultContains("F"),
}];

const OP_BOOL_OR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Fold boolean or",
    code: r"\|[F;F;T]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const OP_BIT_AND_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Apply bitwise and",
    code: "&[6;3]",
    expectation: ExampleExpectation::ResultContains("2"),
}];

const OP_BIT_OR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Apply bitwise or",
    code: r"\[4;1]",
    expectation: ExampleExpectation::ResultContains("5"),
}];

const OP_XOR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Apply bitwise xor",
    code: r"^\[5;3]",
    expectation: ExampleExpectation::ResultContains("6"),
}];

const OP_SHL_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Shift bits left",
    code: "<<[3;2]",
    expectation: ExampleExpectation::ResultContains("12"),
}];

const OP_SHR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Shift bits right",
    code: ">>[16;2]",
    expectation: ExampleExpectation::ResultContains("4"),
}];

pub(super) const OP_ADD: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpAdd,
    summary: "Add values with the `+` operator.",
    details: "`+[xs;ys+]` folds addition left to right. Numeric values broadcast over compatible nested shapes. Callable operands form pointwise callable operator expressions: `(f+g)[x]` evaluates `f[x]+g[x]`.",
    examples: OP_ADD_EXAMPLES,
    related: &["sum", "-", "*"],
};

pub(super) const OP_SUB: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpSub,
    summary: "Negate or subtract values with the `-` operator.",
    details: "`-[x]` negates one value. `-[xs;ys+]` folds subtraction left to right. Callable operands use pointwise callable operator expression behavior, receiving the same positional arguments as each other.",
    examples: OP_SUB_EXAMPLES,
    related: &["neg", "+", "/"],
};

pub(super) const OP_MUL: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpMul,
    summary: "Multiply values with the `*` operator.",
    details: "`*[xs;ys+]` folds multiplication left to right. Numeric values broadcast over compatible nested shapes, and callable operands form pointwise callable operator expressions.",
    examples: OP_MUL_EXAMPLES,
    related: &["product", "+", "**"],
};

pub(super) const OP_DIV: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpDiv,
    summary: "Divide values with the `/` operator.",
    details: "`/[xs;ys+]` folds division left to right. Integer division through `/` produces floating results; use `/.` for exact rational division when possible.",
    examples: OP_DIV_EXAMPLES,
    related: &["/.", "/%", "%"],
};

pub(super) const OP_DIV_DOT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpDivDot,
    summary: "Divide values exactly with the `/.` operator.",
    details: "`/.[xs;ys+]` folds exact division left to right, preserving fraction-like results when possible instead of immediately converting integer division to float.",
    examples: OP_DIV_DOT_EXAMPLES,
    related: &["/", "fraction", "^."],
};

pub(super) const OP_MOD: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpMod,
    summary: "Return remainders with the `%` operator.",
    details: "`%[xs;ys+]` folds modulo left to right. Integer and compatible nested inputs use the same remainder behavior as infix `%`.",
    examples: OP_MOD_EXAMPLES,
    related: &["/%", "/", "int"],
};

pub(super) const OP_FLOORDIV: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpFloorDiv,
    summary: "Floor-divide values with the `/%` operator.",
    details: "`/%[xs;ys+]` folds floor division left to right. For integers, the quotient rounds toward negative infinity rather than toward zero.",
    examples: OP_FLOORDIV_EXAMPLES,
    related: &["%", "/", "floor"],
};

pub(super) const OP_POWER: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpPower,
    summary: "Raise values to powers with the `^` operator.",
    details: "`^[xs;ys+]` folds exponentiation left to right. Integer positive powers stay exact when possible; other numeric cases follow the runtime power operation.",
    examples: OP_POWER_EXAMPLES,
    related: &["^.", "sqrt", "exp"],
};

pub(super) const OP_POWER_DOT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpPowerDot,
    summary: "Raise values to exact powers with the `^.` operator.",
    details: "`^.[xs;ys+]` folds exact exponentiation left to right. Negative integer exponents can produce exact fraction-like results.",
    examples: OP_POWER_DOT_EXAMPLES,
    related: &["^", "/.", "fraction"],
};

pub(super) const OP_MATMUL: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpMatmul,
    summary: "Matrix-multiply values with the `**` operator.",
    details: "`**[xs;ys+]` folds matrix multiplication left to right. Vectors produce dot products, matrix-vector and matrix-matrix inputs use the shared matrix multiplication implementation.",
    examples: OP_MATMUL_EXAMPLES,
    related: &["transpose", "*", "shape"],
};

pub(super) const OP_EQUAL: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpEqual,
    summary: "Compare whole values with the `=` operator.",
    details: "`=[xs;ys+]` checks structural equality across a comparison chain. Unlike `=.`, this whole-value form does not broadcast list leaves.",
    examples: OP_EQUAL_EXAMPLES,
    related: &["=.", "~", "eq"],
};

pub(super) const OP_EQUAL_DOT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpEqualDot,
    summary: "Compare leaves with the `=.` operator.",
    details: "`=.[xs;ys+]` checks equality with broadcasting over compatible nested values. This is the leaf-wise counterpart to whole-value `=`.",
    examples: OP_EQUAL_DOT_EXAMPLES,
    related: &["=", "~.", "in?"],
};

pub(super) const OP_TILDE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpTilde,
    summary: "Compare whole values with `~`, or invert one value.",
    details: "`~[xs;ys+]` checks structural inequality across a comparison chain. `~[x]` is unary bitwise not for integers and boolean not for bools.",
    examples: OP_TILDE_EXAMPLES,
    related: &["~.", "=", "not"],
};

pub(super) const OP_TILDE_DOT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpTildeDot,
    summary: "Compare leaves with the `~.` operator.",
    details: "`~.[xs;ys+]` checks inequality with broadcasting over compatible nested values. This is the leaf-wise counterpart to whole-value `~`.",
    examples: OP_TILDE_DOT_EXAMPLES,
    related: &["~", "=.", "member?"],
};

pub(super) const OP_LT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpLt,
    summary: "Check less-than comparisons with the `<` operator.",
    details: "`<[xs;ys+]` evaluates a chained less-than comparison. Comparisons broadcast compatible nested values, and every adjacent comparison must be true.",
    examples: OP_LT_EXAMPLES,
    related: &["<=", ">", ">="],
};

pub(super) const OP_LTE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpLte,
    summary: "Check less-than-or-equal comparisons with the `<=` operator.",
    details: "`<=[xs;ys+]` evaluates a chained nondecreasing comparison. Comparisons broadcast compatible nested values, and every adjacent comparison must be true.",
    examples: OP_LTE_EXAMPLES,
    related: &["<", ">=", "="],
};

pub(super) const OP_GT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpGt,
    summary: "Check greater-than comparisons with the `>` operator.",
    details: "`>[xs;ys+]` evaluates a chained greater-than comparison. Comparisons broadcast compatible nested values, and every adjacent comparison must be true.",
    examples: OP_GT_EXAMPLES,
    related: &[">=", "<", "<="],
};

pub(super) const OP_GTE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpGte,
    summary: "Check greater-than-or-equal comparisons with the `>=` operator.",
    details: "`>=[xs;ys+]` evaluates a chained nonincreasing comparison. Comparisons broadcast compatible nested values, and every adjacent comparison must be true.",
    examples: OP_GTE_EXAMPLES,
    related: &[">", "<=", "="],
};

pub(super) const OP_CAT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpCat,
    summary: "Concatenate values with the `,` operator.",
    details: "`,[xs;ys+]` concatenates strings, lists, int lists, and atoms into one value. Leading comma in source is separate enlist syntax; this builtin is the binary cat form.",
    examples: OP_CAT_EXAMPLES,
    related: &["list", "flatten", "repeat"],
};

pub(super) const OP_SHARP: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpSharp,
    summary: "Count values with the `#` operator.",
    details: "`#[x]` returns the same length/count result as unary `#x` and `len[x]`, counting the outer length of list-like values.",
    examples: OP_SHARP_EXAMPLES,
    related: &["len", "shape"],
};

pub(super) const OP_BOOL_AND: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpBoolAnd,
    summary: "Combine bools with the `&|` operator.",
    details: "`&|[xs;ys+]` folds boolean and over bool values. Infix `&|` short-circuits expression evaluation; the callable builtin form receives already evaluated arguments.",
    examples: OP_BOOL_AND_EXAMPLES,
    related: &[r"\|", "and", "all"],
};

pub(super) const OP_BOOL_OR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpBoolOr,
    summary: r"Combine bools with the `\|` operator.",
    details: r"`\|[xs;ys+]` folds boolean or over bool values. Infix `\|` short-circuits expression evaluation; the callable builtin form receives already evaluated arguments.",
    examples: OP_BOOL_OR_EXAMPLES,
    related: &["&|", "or", "any"],
};

pub(super) const OP_BIT_AND: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpBitAnd,
    summary: "Apply bitwise and with the `&` operator.",
    details: "`&[xs;ys+]` folds bitwise and over integers and integer lists. It is distinct from boolean `&|`.",
    examples: OP_BIT_AND_EXAMPLES,
    related: &[r"\", r"^\", "band"],
};

pub(super) const OP_BIT_OR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpBitOr,
    summary: r"Apply bitwise or with the `\` operator.",
    details: r"`\[xs;ys+]` folds bitwise or over integers and integer lists. It is distinct from boolean `\|`.",
    examples: OP_BIT_OR_EXAMPLES,
    related: &["&", r"^\", "bor"],
};

pub(super) const OP_XOR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpXor,
    summary: r"Apply bitwise xor with the `^\` operator.",
    details: r"`^\[xs;ys+]` folds bitwise xor over integers, integer lists, and bool pairs.",
    examples: OP_XOR_EXAMPLES,
    related: &["&", r"\", "xor"],
};

pub(super) const OP_SHL: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpShl,
    summary: "Shift integer bits left with the `<<` operator.",
    details: "`<<[xs;ys+]` folds left shifts over integer values. Shift counts must be non-negative and fit the runtime shift range.",
    examples: OP_SHL_EXAMPLES,
    related: &[">>", "shl", "&"],
};

pub(super) const OP_SHR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpShr,
    summary: "Shift integer bits right with the `>>` operator.",
    details: "`>>[xs;ys+]` folds right shifts over integer values. Shift counts must be non-negative and fit the runtime shift range.",
    examples: OP_SHR_EXAMPLES,
    related: &["<<", "shr", "&"],
};
