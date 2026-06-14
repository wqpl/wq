use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const COMPLEX_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Build a complex number",
    code: "complex[3;4]",
    expectation: ExampleExpectation::ResultContains("3+4i"),
}];

const RE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Take the real part",
    code: "re[3+4i]",
    expectation: ExampleExpectation::ResultContains("3.0"),
}];

const IM_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Take the imaginary part",
    code: "im[3+4i]",
    expectation: ExampleExpectation::ResultContains("4.0"),
}];

const CONJ_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Reflect across the real axis",
    code: "conj[3+4i]",
    expectation: ExampleExpectation::ResultContains("3-4i"),
}];

pub(super) const COMPLEX: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Complex,
    summary: "Build a complex number from real and imaginary parts.",
    details: "`complex[re;im]` converts both parts to real `f64` numbers and returns `re+im*i`. wq also accepts imaginary literals with an `i` suffix, so `4i` is pure imaginary and `3+4i` is ordinary arithmetic that produces the same value as `complex[3;4]`. Complex values are atoms, not two-item lists; arithmetic and many math builtins work with them directly.",
    examples: COMPLEX_EXAMPLES,
    related: &["re", "im", "conj", "sqrt"],
};

pub(super) const RE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Re,
    summary: "Return the real part of a real or complex value.",
    details: "`re[x]` returns the real component of a complex number. Real numeric inputs, including fractions, are already real and are returned unchanged. When given a list, `re` applies to each item and stops at complex atoms rather than treating them as containers.",
    examples: RE_EXAMPLES,
    related: &["im", "conj", "complex"],
};

pub(super) const IM: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Im,
    summary: "Return the imaginary part of a real or complex value.",
    details: "`im[x]` returns the imaginary component of a complex number. Real numeric inputs have imaginary part `0`. When given a list, `im` applies to each item and stops at complex atoms.",
    examples: IM_EXAMPLES,
    related: &["re", "conj", "complex"],
};

pub(super) const CONJ: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Conj,
    summary: "Return the complex conjugate.",
    details: "`conj[x]` changes `a+bi` into `a-bi`. Real numeric inputs are returned unchanged. When given a list, `conj` applies to each item and stops at complex atoms.",
    examples: CONJ_EXAMPLES,
    related: &["re", "im", "complex"],
};
