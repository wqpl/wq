use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const AND_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Fold boolean and",
    code: "and[T;F;T]",
    expectation: ExampleExpectation::ResultContains("F"),
}];

const OR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Fold boolean or",
    code: "or[F;F;T]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const XOR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Apply bitwise xor",
    code: "xor[5;3]",
    expectation: ExampleExpectation::ResultContains("6"),
}];

const BAND_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Apply bitwise and",
    code: "band[6;3]",
    expectation: ExampleExpectation::ResultContains("2"),
}];

const BOR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Apply bitwise or",
    code: "bor[4;1]",
    expectation: ExampleExpectation::ResultContains("5"),
}];

const SHL_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Shift bits left",
    code: "shl[3;2]",
    expectation: ExampleExpectation::ResultContains("12"),
}];

const SHR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Shift bits right",
    code: "shr[16;2]",
    expectation: ExampleExpectation::ResultContains("4"),
}];

pub(super) const AND: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::And,
    summary: "Combine bools eagerly with boolean and.",
    details: "`and[xs;ys+]` folds boolean and over bool values. All arguments are evaluated before the call. Use `A[xs;ys+]` when later expressions should short-circuit.",
    examples: AND_EXAMPLES,
    related: &["A", "or", "all"],
};

pub(super) const OR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Or,
    summary: "Combine bools eagerly with boolean or.",
    details: "`or[xs;ys+]` folds boolean or over bool values. All arguments are evaluated before the call. Use `O[xs;ys+]` when later expressions should short-circuit.",
    examples: OR_EXAMPLES,
    related: &["O", "and", "any"],
};

pub(super) const XOR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Xor,
    summary: "Apply bitwise xor.",
    details: "`xor[xs;ys+]` folds bitwise xor over integers, integer lists, and bool pairs.",
    examples: XOR_EXAMPLES,
    related: &["band", "bor"],
};

pub(super) const BAND: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Band,
    summary: "Apply bitwise and.",
    details: "`band[xs;ys+]` folds bitwise and over integers and integer lists.",
    examples: BAND_EXAMPLES,
    related: &["bor", "xor"],
};

pub(super) const BOR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Bor,
    summary: "Apply bitwise or.",
    details: "`bor[xs;ys+]` folds bitwise or over integers and integer lists.",
    examples: BOR_EXAMPLES,
    related: &["band", "xor"],
};

pub(super) const SHL: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Shl,
    summary: "Shift integer bits left.",
    details: "`shl[xs;shift+]` folds exact left shifts over integer values, promoting to bigint when an int result no longer fits. Shift counts must be non-negative and fit the runtime shift range.",
    examples: SHL_EXAMPLES,
    related: &["shr", "band", "bor"],
};

pub(super) const SHR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Shr,
    summary: "Shift integer bits right.",
    details: "`shr[xs;shift+]` folds exact arithmetic right shifts over integer values. Shift counts must be non-negative and fit the runtime shift range.",
    examples: SHR_EXAMPLES,
    related: &["shl", "band", "bor"],
};
