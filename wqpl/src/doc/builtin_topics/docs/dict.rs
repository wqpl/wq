use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const KEYS_EXAMPLES: &[DocExample] = &[DocExample {
    title: "List keys in dict order",
    code: "keys (`a:1;`b:2)",
    expectation: ExampleExpectation::ResultContains("`a"),
}];

const IDX_TO_KEY_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Find the last key",
    code: "itk[(`a:1;`b:2);-1]",
    expectation: ExampleExpectation::ResultContains("`b"),
}];

const KEY_TO_IDX_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Find a key's position",
    code: "kti[(`a:1;`b:2);`b]",
    expectation: ExampleExpectation::ResultContains("1"),
}];

pub(super) const KEYS: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Keys,
    summary: "Return a dict's keys in order.",
    details: "`keys[dct]` requires a dict and returns a list of its keys as tags. The order is the dict's stored order, so it matches positional dict indexing and the positions used by `itk` and `kti`.",
    examples: KEYS_EXAMPLES,
    related: &["dict", "itk", "kti"],
};

pub(super) const IDX_TO_KEY: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::IdxToKey,
    summary: "Return the key at a dict position.",
    details: "`itk[dct;i]` maps a zero-based dict position to its tag key. Negative positions count from the end, so `-1` is the last key. Out-of-range positions return an empty list instead of raising.",
    examples: IDX_TO_KEY_EXAMPLES,
    related: &["keys", "kti", "dict"],
};

pub(super) const KEY_TO_IDX: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::KeyToIdx,
    summary: "Return a key's position in a dict.",
    details: "`kti[dct;k]` requires `k` to be a tag and returns the zero-based position of that key in the dict's stored order. Missing keys return an empty list instead of raising.",
    examples: KEY_TO_IDX_EXAMPLES,
    related: &["keys", "itk", "dict"],
};
