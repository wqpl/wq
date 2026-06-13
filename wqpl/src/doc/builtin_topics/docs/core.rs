use crate::builtins::BuiltinEnum;

use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};

const ECHO_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Print a value",
    code: "echo \"hello\"",
    expectation: ExampleExpectation::NoRun("writes to stdout"),
}];

const LEN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Count list items",
    code: "len (10;20;30)",
    expectation: ExampleExpectation::ResultContains("3"),
}];

pub(super) const ECHO: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Echo,
    summary: "Print values to stdout and return unit.",
    details: "Use `echo` for display-oriented output. In expression-heavy code, prefer `expr |echo` when the expression would otherwise need parentheses before a postfix call.",
    examples: ECHO_EXAMPLES,
    related: &["print", "str", "pipes"],
};

pub(super) const LEN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Len,
    summary: "Return the length of a value.",
    details: "For lists and strings, `len` returns the number of top-level items. Atoms have length 1 and unit has length 0.",
    examples: LEN_EXAMPLES,
    related: &["shape", "#"],
};
