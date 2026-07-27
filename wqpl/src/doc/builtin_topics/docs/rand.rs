use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const RAND_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Use a one-value int range",
    code: "rand[1]=0",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const RNG_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Create a reproducible generator",
    code: "str rng 42",
    expectation: ExampleExpectation::ResultContains("/* rng */"),
}];

pub(super) const RAND: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Rand,
    summary: "Generate a random number.",
    details: "`rand[]` returns a float in the half-open range `0.0..1.0`.
`rand[upper]` requires a positive finite int or float; int bounds return an int in `0..upper`, while float bounds return a float in `0.0..upper`.
`rand[lower;upper]` requires finite bounds with `lower < upper`; two int bounds return an int in `lower..upper`, and any float bound makes the result a float.
The upper bound is never included.
The runtime generator starts with system entropy, so `rand` is nondeterministic unless wq-cli receives `--seed`.",
    examples: RAND_EXAMPLES,
    related: &["rng", "til", "map", "asciiplot"],
};

pub(super) const RNG: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Rng,
    summary: "Create a reproducible random generator.",
    details: "`rng[seed]` accepts an int in the signed 64-bit range and returns a stateful callable rng atom.
Call the result as `r[]`, `r[upper]`, or `r[lower;upper]`; these forms have the same ranges and result types as `rand`.
Every call advances the generator.
Two fresh generators with the same seed and call sequence return the same values on every supported platform.
Assignment aliases the generator, so `b:a` shares state while `b:rng seed` creates an independent matching stream.
The stable `wq-rng-v1` stream uses xoshiro256++ with SplitMix64 seed expansion.
It is not intended for cryptographic use.",
    examples: RNG_EXAMPLES,
    related: &["rand", "til", "map"],
};
