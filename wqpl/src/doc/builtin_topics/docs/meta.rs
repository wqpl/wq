use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const STRONG_COUNT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Immediate atoms report one reference",
    code: "strong_count 42",
    expectation: ExampleExpectation::ResultContains("1"),
}];

const SHAPE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Describe a regular nested outline",
    code: "shape ((1;2);(3;4))",
    expectation: ExampleExpectation::ResultContains("(2;2)"),
}];

const DEPTH_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Count container layers",
    code: "(depth 7;depth (7;8);depth ((1;2);(3;4)))",
    expectation: ExampleExpectation::ResultContains("(0;1;2)"),
}];

const UNIFORM_Q_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Detect whether nested sizes match",
    code: "(uniform? ((1;2);(3;4));uniform? ((1;2);(3;4;5)))",
    expectation: ExampleExpectation::ResultContains("(T;F)"),
}];

pub(super) const STRONG_COUNT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::StrongCount,
    summary: "Report how many references share a value's backing storage.",
    details: "`strong_count[x]` is a runtime diagnostic. Values stored behind shared backing storage, such as strings, lists, dicts, functions, and streams, report the current reference count of that storage. Immediate atoms such as ints, floats, chars, bools, and builtin functions are not shared this way and report `1`. Treat the number as an implementation detail for debugging memory sharing, not as a property of the data itself.",
    examples: STRONG_COUNT_EXAMPLES,
    related: &["shape", "depth", "len"],
};

pub(super) const SHAPE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Shape,
    summary: "Describe the size outline of a value.",
    details: "`shape[xs]` answers: if you look from the outside inward, how many items are at each regular layer? A plain atom has no container layer, so its shape is `()`. A string has shape `(characters)`, a flat list has shape `(items)`, and `((1;2);(3;4))` has shape `(2;2)` because there are two outer items and each has two inner items. Dicts use their values in key order. When nested children do not all have the same shape, the value is ragged; `shape` then returns only the outer length, and `uniform?` tells you that the full outline is not regular.",
    examples: SHAPE_EXAMPLES,
    related: &["uniform?", "depth", "len", "reshape"],
};

pub(super) const DEPTH: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Depth,
    summary: "Count how many container layers a value has.",
    details: "`depth[xs]` answers: how many list, string, or dict layers must you pass through to reach the deepest plain item? Atoms have depth `0`, flat lists and strings have depth `1`, and a list of lists has depth `2`. For uneven data, depth follows the deepest branch, so it can still be useful when `shape` is ragged.",
    examples: DEPTH_EXAMPLES,
    related: &["shape", "uniform?", "@depth"],
};

pub(super) const UNIFORM_Q: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::UniformQ,
    summary: "Return true when a value has a regular shape.",
    details: "`uniform?[xs]` is true when `shape` can describe the whole nested outline: each child at a layer has the same shape as its siblings. Atoms, strings, flat lists, empty lists, and regular nested lists or dicts are uniform. Ragged values such as `((1;2);(3;4;5))` are not uniform because one branch is longer than the other.",
    examples: UNIFORM_Q_EXAMPLES,
    related: &["shape", "depth", "reshape"],
};
