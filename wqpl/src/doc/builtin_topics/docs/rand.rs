use crate::builtins::BuiltinEnum;

use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};

const RAND_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Use a one-value integer range",
    code: "rand[1]=0",
    expectation: ExampleExpectation::ResultContains("T"),
}];

pub(super) const RAND: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Rand,
    summary: "Generate a random number.",
    details: "`rand[]` returns a float in the half-open range `0.0..1.0`. `rand[upper]` requires a positive int or float; int bounds return an int in `0..upper`, while float bounds return a float in `0.0..upper`. `rand[lower;upper]` requires `lower < upper`; two int bounds return an int in `lower..upper`, and any float bound makes the result a float. The upper bound is never included. This builtin uses the runtime random generator and is intentionally not deterministic.",
    examples: RAND_EXAMPLES,
    related: &["til", "map", "asciiplot"],
};
