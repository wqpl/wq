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

const OP_POWER_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Raise to a power",
        code: "^[2;3]",
        expectation: ExampleExpectation::ResultContains("8"),
    },
    DocExample {
        title: "Use classic fractional power",
        code: "^[8/.27;1/.3]",
        expectation: ExampleExpectation::ResultContains("0.666"),
    },
];

const OP_POWER_DOT_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Raise to a power exactly",
        code: "^.[2;-3]",
        expectation: ExampleExpectation::ResultContains("1/8"),
    },
    DocExample {
        title: "Take an exact fractional power",
        code: "^.[8/.27;1/.3]",
        expectation: ExampleExpectation::ResultContains("2/3"),
    },
];

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
    details: "`/.[xs;ys+]` folds exact division left to right, preserving fraction-like results when possible instead of immediately converting integer division to float. Use it inside exact exponent literals such as `1/.3`.",
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
    details: "`^[xs;ys+]` folds exponentiation left to right. Integer positive powers stay exact when possible; negative or fractional numeric exponents follow the classic runtime power operation and may produce floats or complex values.",
    examples: OP_POWER_EXAMPLES,
    related: &["^.", "sqrt", "exp"],
};

pub(super) const OP_POWER_DOT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpPowerDot,
    summary: "Raise values to exact powers with the `^.` operator.",
    details: "`^.[xs;ys+]` folds exact exponentiation left to right. Negative integer exponents and exact fractional exponents with rational results can produce exact fraction-like results. Use `/.` to write exact rational exponents, for example `1/.3`; `1/3` is already a float.",
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
    details: "`=.[xs;ys+]` checks equality with depth-1 broadcasting over compatible nested values. This is the element-wise counterpart to whole-value `=`, so `(1;2)=.(1;3)` returns `(T;F)` while `(1;2)=(1;3)` returns `F`.",
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
    details: "`~.[xs;ys+]` checks inequality with depth-1 broadcasting over compatible nested values. This is the element-wise counterpart to whole-value `~`, mirroring the relationship between `=.` and `=`.",
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
    details: "`,[xs;ys+]` concatenates strings, lists, and atoms into one value. Leading comma in source is separate enlist syntax; this builtin is the binary cat form.",
    examples: OP_CAT_EXAMPLES,
    related: &["list", "flatten", "repeat"],
};

pub(super) const OP_SHARP: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::OpSharp,
    summary: "Count values with the `#` operator.",
    details: "`#[x]` returns the same length/count result as unary `#x` and `len[x]`, counting the outer length of containers.",
    examples: OP_SHARP_EXAMPLES,
    related: &["len", "shape"],
};
