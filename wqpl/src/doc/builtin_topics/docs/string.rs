use crate::builtins::BuiltinEnum;

use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};

const WORDS_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Split on whitespace",
    code: "words \"red green blue\"",
    expectation: ExampleExpectation::ResultContains("(\"red\";\"green\";\"blue\")"),
}];

const STR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Convert a value to text",
    code: "str (1;2)",
    expectation: ExampleExpectation::ResultContains("(1;2)"),
}];

pub(super) const WORDS: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Words,
    summary: "Split text into whitespace-delimited words.",
    details: "`words` is a string-focused convenience for common tokenization. It trims whitespace and omits empty runs.",
    examples: WORDS_EXAMPLES,
    related: &["split", "trim", "graphemes"],
};

pub(super) const STR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Str,
    summary: "Convert a value to a string.",
    details: "`str` is useful when a display representation is needed as data rather than as terminal output.",
    examples: STR_EXAMPLES,
    related: &["fmt", "@f", "echo"],
};
