use crate::builtins::BuiltinEnum;

use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};

const HAS_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Membership with depth sugar",
    code: "(1;2;3)|has?@1[2]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

pub(super) const HAS_Q: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::HasQ,
    summary: "Test whether a container has a value.",
    details: "`has?` returns a bool. It is depth-aware, so postfix depth modifiers can be used when searching nested values.",
    examples: HAS_EXAMPLES,
    related: &["in?", "@depth"],
};
