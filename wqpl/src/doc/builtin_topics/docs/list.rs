use crate::builtins::BuiltinEnum;

use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};

const SPLIT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Split into runs",
    code: "len split[(1;2;3);2]",
    expectation: ExampleExpectation::ResultContains("2"),
}];

pub(super) const SPLIT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Split,
    summary: "Split a value into groups.",
    details: "`split` separates a value using a delimiter or delimiter-like rule. Some modes accept named options such as `maxsplit`.",
    examples: SPLIT_EXAMPLES,
    related: &["splitw", "words"],
};
