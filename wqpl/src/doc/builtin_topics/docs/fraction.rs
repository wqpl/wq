use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const FRACTION_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Build and reduce a fraction",
    code: "fraction[6;8]",
    expectation: ExampleExpectation::ResultContains("3/4"),
}];

const FRACTIONL_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Approximate a float with a limited denominator",
    code: "fractionl 0.1",
    expectation: ExampleExpectation::ResultContains("1/10"),
}];

pub(super) const FRACTION: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Fraction,
    summary: "Convert a value to an exact rational number.",
    details: "`fraction[x]` accepts ints, existing fractions, finite floats, fraction strings like `\"3/4\"`, int-literal strings such as `\"0x10\"`, decimal strings like `\"0.3\"`, chars that parse as numbers, and two-item lists of ints. Floats are converted exactly from their stored binary value, so `fraction 0.3` is not the same as `fraction \"0.3\"`. `fraction[n;d]` builds the reduced fraction `n/d` when both arguments are ints and rejects a zero denominator. Otherwise the second argument is a positive maximum denominator used to approximate the first argument, as in `fraction[1.0/3.0;10]`. Fractions display as `n/d`, or just `n` when the denominator is `1`; index a fraction with `n`/`numer` or `d`/`denom` to inspect its parts.",
    examples: FRACTION_EXAMPLES,
    related: &["fractionl", "int", "float"],
};

pub(super) const FRACTIONL: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Fractionl,
    summary: "Convert a value to a nearby rational with a bounded denominator.",
    details: "`fractionl[x]` is the limited-denominator form of `fraction`: it converts `x` to a rational value and then chooses a close fraction whose denominator is at most `1_000_000`. This is usually the friendlier choice for floats that came from decimal input, measurements, or numeric calculations.",
    examples: FRACTIONL_EXAMPLES,
    related: &["fraction", "round", "float"],
};
