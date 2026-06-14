use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const APPLY_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Apply several functions to one value",
    code: "apply[(neg;abs);-3]",
    expectation: ExampleExpectation::ResultContains("(3;3)"),
}];

const MAP_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Map a function over a list",
    code: "(1;2;3)|map{x*x}",
    expectation: ExampleExpectation::ResultContains("(1;4;9)"),
}];

const FOLD_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Fold from the left",
    code: "fold[(1;2;3;4);{x+y}]",
    expectation: ExampleExpectation::ResultContains("10"),
}];

const SCAN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Keep each running fold result",
    code: "scan[(1;2;3);{x+y}]",
    expectation: ExampleExpectation::ResultContains("(1;3;6)"),
}];

const RSCAN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Scan from the right",
    code: "rscan[(1;2;3);{x+y}]",
    expectation: ExampleExpectation::ResultContains("(6;5;3)"),
}];

const ANY_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Test whether any item matches",
    code: "any[(1;2;3);{x>2}]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const ALL_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Test whether all items match",
    code: "all[(1;2;3);{x>0}]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const FILTER_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Keep matching items",
    code: "filter[(1;2;3;4);{x%2=0}]",
    expectation: ExampleExpectation::ResultContains("(2;4)"),
}];

const ZIPW_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Combine corresponding items",
    code: "zipw[(1;2;3);(10;20;30);{x+y}]",
    expectation: ExampleExpectation::ResultContains("(11;22;33)"),
}];

const SPLITW_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Split where a predicate matches",
    code: "splitw[\"a,b,c\";{x=\",\"};`m:1]",
    expectation: ExampleExpectation::ResultContains("\"b,c\""),
}];

const FINDW_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Find the first matching path",
    code: "findw[(1;2;3);{x>1}]",
    expectation: ExampleExpectation::ResultContains(",1"),
}];

const RFINDW_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Find the last matching path",
    code: "rfindw[(1;2;3;4);{x%2=0}]",
    expectation: ExampleExpectation::ResultContains(",3"),
}];

pub(super) const APPLY: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Apply,
    summary: "Apply one function or several functions to a value.",
    details: "`apply[fs;x]` calls each function in `fs` with `x`. When `fs` is a list, the result is a list of callback results; when `fs` is a single callable, the callback result is returned directly.",
    examples: APPLY_EXAMPLES,
    related: &["A", "map", "fold"],
};

pub(super) const MAP: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Map,
    summary: "Apply a function to each item of a value.",
    details: "`map[xs;f;d?]` applies `f` across `xs`. The default depth is `1`; non-negative depths count from the root, negative depths count back from the value depth, `inf` maps leaves, and `-inf` applies at the root.",
    examples: MAP_EXAMPLES,
    related: &["M", "filter", "zipw", "@depth"],
};

pub(super) const FOLD: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Fold,
    summary: "Reduce a value from left to right.",
    details: "`fold[xs;f]` uses the first item as the initial accumulator; `fold[xs;f;i]` starts with `i`. Empty lists without an initial value return unit, and atoms are returned unchanged.",
    examples: FOLD_EXAMPLES,
    related: &["reduce", "scan", "rscan"],
};

pub(super) const SCAN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Scan,
    summary: "Return the running results of a left fold.",
    details: "`scan[xs;f]` keeps each intermediate accumulator. With an explicit accumulator, the initial value is used to process each item but is not included as a separate output item.",
    examples: SCAN_EXAMPLES,
    related: &["fold", "rscan"],
};

pub(super) const RSCAN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::RScan,
    summary: "Return the running results of a right-to-left fold.",
    details: "`rscan` walks the input from the right, then returns the accumulated results in input order. Like `scan`, an explicit accumulator seeds the computation without being emitted by itself.",
    examples: RSCAN_EXAMPLES,
    related: &["scan", "fold"],
};

pub(super) const ANY: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Any,
    summary: "Return true when any item satisfies a predicate.",
    details: "`any[xs;f;d?]` calls `f` until one result is true or the search is exhausted. The predicate must return a bool, and the optional depth follows the same depth model as `map`.",
    examples: ANY_EXAMPLES,
    related: &["all", "filter", "findw"],
};

pub(super) const ALL: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::All,
    summary: "Return true when every item satisfies a predicate.",
    details: "`all[xs;f;d?]` calls `f` until one result is false or the search is exhausted. The predicate must return a bool, and the optional depth follows the same depth model as `map`.",
    examples: ALL_EXAMPLES,
    related: &["any", "filter"],
};

pub(super) const FILTER: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Filter,
    summary: "Keep items whose predicate result is true.",
    details: "`filter[xs;f]` calls `f` for each item and returns the matching values. Predicates must return bools; atoms are returned unchanged, and dicts are filtered by value rather than by key.",
    examples: FILTER_EXAMPLES,
    related: &["any", "all", "findw"],
};

pub(super) const ZIPW: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::ZipW,
    summary: "Zip two values with a binary callback.",
    details: "`zipw[xs;ys;f;d?]` pairs corresponding items from `xs` and `ys`, calling `f` with each pair. The optional depth follows the same root, negative, `inf`, and `-inf` model as `map`.",
    examples: ZIPW_EXAMPLES,
    related: &["zip", "map"],
};

pub(super) const SPLITW: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::SplitW,
    summary: "Split a string or list where a predicate is true.",
    details: "`splitw` calls `f` for each item and starts a new chunk when it returns true. Matching delimiter items are dropped while splits remain; the named `m` option limits the number of splits.",
    examples: SPLITW_EXAMPLES,
    related: &["split", "words", "filter"],
};

pub(super) const FINDW: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::FindW,
    summary: "Find paths to items that satisfy a predicate.",
    details: "`findw[xs;f;threshold?;d?]` searches forward and returns index paths. No match returns unit, one match is returned directly, and multiple matches are returned as a list of paths.",
    examples: FINDW_EXAMPLES,
    related: &["rfindw", "find", "filter"],
};

pub(super) const RFINDW: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::RFindW,
    summary: "Find matching paths from the end of a value.",
    details: "`rfindw` has the same return shape as `findw`, but searches list, int-list, and dict values in reverse order. Threshold and depth accept non-negative ints, with `inf` allowed for an unlimited search.",
    examples: RFINDW_EXAMPLES,
    related: &["findw", "find", "filter"],
};
