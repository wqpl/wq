use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const BXOR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Apply bitwise xor",
    code: "bxor[5;3]",
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

pub(super) const BXOR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Bxor,
    summary: "Apply bitwise xor.",
    details: "`bxor[xs;ys+]` folds bitwise xor over ints, bools, and compatible lists of them.",
    examples: BXOR_EXAMPLES,
    related: &["band", "bor"],
};

pub(super) const BAND: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Band,
    summary: "Apply bitwise and.",
    details: "`band[xs;ys+]` folds bitwise and over ints, bools, and compatible lists of them.",
    examples: BAND_EXAMPLES,
    related: &["bor", "bxor"],
};

pub(super) const BOR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Bor,
    summary: "Apply bitwise or.",
    details: "`bor[xs;ys+]` folds bitwise or over ints, bools, and compatible lists of them.",
    examples: BOR_EXAMPLES,
    related: &["band", "bxor"],
};

pub(super) const SHL: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Shl,
    summary: "Shift int bits left.",
    details: "`shl[xs;shift+]` folds exact left shifts over ints. Results remain exact when they exceed the signed 64-bit range. Shift counts must be non-negative and fit the runtime shift range.",
    examples: SHL_EXAMPLES,
    related: &["shr", "band", "bor"],
};

pub(super) const SHR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Shr,
    summary: "Shift int bits right.",
    details: "`shr[xs;shift+]` folds exact arithmetic right shifts over ints. Shift counts must be non-negative and fit the runtime shift range.",
    examples: SHR_EXAMPLES,
    related: &["shl", "band", "bor"],
};
