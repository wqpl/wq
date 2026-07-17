use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const UNIQUE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Keep first occurrences",
    code: "unique (2;1;2;3)",
    expectation: ExampleExpectation::ResultContains("(2;1;3)"),
}];

const COUNTS_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Count distinct chars",
    code: "counts \"banana\"|len",
    expectation: ExampleExpectation::ResultContains("3"),
}];

const UNION_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Combine two sequences",
    code: "union[(2;1;2);(1;3)]",
    expectation: ExampleExpectation::ResultContains("(2;1;3)"),
}];

const INTERSECT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Keep values also present on the right",
    code: "intersect[(2;1;2;3);(3;1)]",
    expectation: ExampleExpectation::ResultContains("(1;3)"),
}];

const WITHOUT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Drop values present on the right",
    code: "without[(2;1;2;3);(1;3)]",
    expectation: ExampleExpectation::ResultContains("(2;2)"),
}];

const SYMDIFF_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Keep values present on exactly one side",
    code: "symdiff[(2;1;3);(3;4;1)]",
    expectation: ExampleExpectation::ResultContains("(2;4)"),
}];

const SUB_Q_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Ignore duplicate items in subset checks",
    code: "sub?[(1;1;2);(1;2;3)]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const SUPER_Q_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Check whether the left side contains the right side",
    code: "super?[(1;2;3);(1;2)]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const P_SUB_Q_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Require at least one value to be missing from the left",
    code: "psub?[(1;2);(1;2;3)]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const P_SUPER_Q_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Require at least one extra value on the left",
    code: "psuper?[(1;2;3);(1;2)]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const MEMBER_Q_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Test each left item against a right-hand set",
    code: "member?[(1;2;3);(1;3)]",
    expectation: ExampleExpectation::ResultContains("(T;F;T)"),
}];

const CART_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Count all Cartesian pairs",
    code: "len cart[(1;2);(3;4)]",
    expectation: ExampleExpectation::ResultContains("4"),
}];

const IN_Q_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Search nested values at a requested depth",
    code: "in?[2;((1;2);(3;4));1]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const HAS_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Search nested values with depth sugar",
    code: "((1;2);(3;4))|has?@1[4]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const DISJOINT_Q_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Check that two sets do not overlap",
    code: "disjoint?[(1;2);(3;4)]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const MULTIPLICITY_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Count occurrences in a list",
    code: "multiplicity[1;(1;2;1;3)]",
    expectation: ExampleExpectation::ResultContains("2"),
}];

pub(super) const UNIQUE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Unique,
    summary: "Return unique items in first-seen order.",
    details: "`unique` views lists as their items, strings as chars, dicts as keys, and atoms as singleton values. It returns the first occurrence of each distinct item.",
    examples: UNIQUE_EXAMPLES,
    related: &["union", "multiplicity"],
};

pub(super) const COUNTS: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Counts,
    summary: "Count distinct items in first-seen order.",
    details: "`counts[xs]` returns a list of `(item;count)` pairs. Lists contribute items, strings contribute chars, dicts contribute keys, and atoms behave as singletons.",
    examples: COUNTS_EXAMPLES,
    related: &["unique", "multiplicity", "member?"],
};

pub(super) const UNION: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Union,
    summary: "Return the ordered set union of two values.",
    details: "`union[xs;ys]` emits unique items seen in `xs` followed by new items from `ys`. Lists contribute items, strings contribute chars, dicts contribute keys, and atoms behave as singletons.",
    examples: UNION_EXAMPLES,
    related: &["intersect", "symdiff", "unique"],
};

pub(super) const INTERSECT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Intersect,
    summary: "Return items from the left that also occur on the right.",
    details: "`intersect[xs;ys]` treats the right argument as a set and returns unique matching left items in first-seen left order. Duplicate matches in the left are collapsed.",
    examples: INTERSECT_EXAMPLES,
    related: &["union", "without", "disjoint?"],
};

pub(super) const WITHOUT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Without,
    summary: "Return left items that are absent from the right.",
    details: "`without[xs;ys]` treats `ys` as a set of values to remove from `xs`. Unlike most set algebra builtins, duplicate left items that survive the removal are preserved.",
    examples: WITHOUT_EXAMPLES,
    related: &["intersect", "symdiff"],
};

pub(super) const SYMDIFF: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Symdiff,
    summary: "Return items present on exactly one side.",
    details: "`symdiff[xs;ys]` keeps values that occur in `xs` or `ys`, but not both. Results are unique, ordered by first-seen left-only items followed by first-seen right-only items.",
    examples: SYMDIFF_EXAMPLES,
    related: &["union", "without", "intersect"],
};

pub(super) const SUB_Q: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::SubQ,
    summary: "Test whether the left set is contained in the right set.",
    details: "`sub?[xs;ys]` ignores multiplicity and returns true when every distinct item in `xs` is also present in `ys`.",
    examples: SUB_Q_EXAMPLES,
    related: &["psub?", "super?", "member?"],
};

pub(super) const SUPER_Q: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::SuperQ,
    summary: "Test whether the left set contains the right set.",
    details: "`super?[xs;ys]` is the reverse of `sub?`: it returns true when every distinct item in `ys` is present in `xs`.",
    examples: SUPER_Q_EXAMPLES,
    related: &["psuper?", "sub?", "has?"],
};

pub(super) const P_SUB_Q: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::PSubQ,
    summary: "Test whether the left set is a proper subset of the right set.",
    details: "`psub?[xs;ys]` returns true when `xs` is contained in `ys` and `ys` has at least one additional distinct item. Multiplicity is ignored.",
    examples: P_SUB_Q_EXAMPLES,
    related: &["sub?", "psuper?"],
};

pub(super) const P_SUPER_Q: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::PSuperQ,
    summary: "Test whether the left set is a proper superset of the right set.",
    details: "`psuper?[xs;ys]` returns true when `xs` contains `ys` and `xs` has at least one additional distinct item. Multiplicity is ignored.",
    examples: P_SUPER_Q_EXAMPLES,
    related: &["super?", "psub?"],
};

pub(super) const MEMBER_Q: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::MemberQ,
    summary: "Test left-side items for membership in a right-hand set.",
    details: "`member?[xs;ys]` returns a bool for each item of `xs`, using `ys` as the membership set. When `xs` is an atom, the result is a single bool instead of a one-item list.",
    examples: MEMBER_Q_EXAMPLES,
    related: &["in?", "has?", "sub?"],
};

pub(super) const CART: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Cart,
    summary: "Return the Cartesian product of two values.",
    details: "`cart[xs;ys]` pairs every item from `xs` with every item from `ys`, returning a list of two-item lists in left-major order.",
    examples: CART_EXAMPLES,
    related: &["zip", "map"],
};

pub(super) const IN_Q: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::InQ,
    summary: "Test whether a value is in a container.",
    details: "`in?[x;xs;d?]` searches for `x` in `xs`. Without depth it checks only top-level items or dict keys; with depth it can search into nested lists and dict values. Non-negative depths count from the root, negative depths count back from the leaves, `inf` searches all leaves, and `-inf` checks the root.",
    examples: IN_Q_EXAMPLES,
    related: &["has?", "member?", "@depth"],
};

pub(super) const HAS_Q: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::HasQ,
    summary: "Test whether a container has a value.",
    details: "`has?[xs;x;d?]` is the container-first form of `in?`. It checks top-level items by default, and the optional depth follows the same root and leaves model as `in?`.",
    examples: HAS_EXAMPLES,
    related: &["in?", "member?", "@depth"],
};

pub(super) const DISJOINT_Q: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::DisjointQ,
    summary: "Test whether two sets share no items.",
    details: "`disjoint?[xs;ys]` returns true when no distinct item from `xs` is present in `ys`. Multiplicity is ignored.",
    examples: DISJOINT_Q_EXAMPLES,
    related: &["intersect", "member?"],
};

pub(super) const MULTIPLICITY: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Multiplicity,
    summary: "Count how many times a value occurs in a container.",
    details: "`multiplicity[x;xs]` counts occurrences of `x` in lists and strings. Dicts count matching keys as `1` or `0`, and atoms are treated as singleton values.",
    examples: MULTIPLICITY_EXAMPLES,
    related: &["unique", "has?", "member?"],
};
