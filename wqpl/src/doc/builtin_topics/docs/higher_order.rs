use crate::builtins::BuiltinEnum;

use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};

const MAP_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Map a function over a list",
    code: "(1;2;3)|map{x*x}",
    expectation: ExampleExpectation::ResultContains("(1;4;9)"),
}];

pub(super) const MAP: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Map,
    summary: "Apply a function to each item of a value.",
    details: "The short alias is `M`. Depth-aware calls can use modifiers such as `@1` on builtins that support depth sugar.",
    examples: MAP_EXAMPLES,
    related: &["M", "filter", "fold", "@depth"],
};
