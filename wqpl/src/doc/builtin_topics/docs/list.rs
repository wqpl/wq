use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const SUM_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Add all items",
    code: "sum (1;2;3;4)",
    expectation: ExampleExpectation::ResultContains("10"),
}];

const PRODUCT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Multiply all items",
    code: "product (2;3;4)",
    expectation: ExampleExpectation::ResultContains("24"),
}];

const MIN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Find the smallest item",
    code: "min (5;1;9;3)",
    expectation: ExampleExpectation::ResultContains("1"),
}];

const MAX_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Find the largest item",
    code: "max (5;1;9;3)",
    expectation: ExampleExpectation::ResultContains("9"),
}];

const FLATTEN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Collect nested leaves",
    code: "flatten (1;(2;3);(4;(5;6)))",
    expectation: ExampleExpectation::ResultContains("(1;2;3;4;5;6)"),
}];

const REVERSE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Reverse top-level order",
    code: "reverse (1;2;3)",
    expectation: ExampleExpectation::ResultContains("(3;2;1)"),
}];

const SORT_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Sort comparable values",
        code: "sort (3;1;2)",
        expectation: ExampleExpectation::ResultContains("(1;2;3)"),
    },
    DocExample {
        title: "Sort a dict by key",
        code: "sort[(`b:1;`a:2);`by:`key]",
        expectation: ExampleExpectation::ResultContains("(`a:2;`b:1)"),
    },
];

const SPLIT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Limit delimiter splits",
    code: "split[\"a,b,c\";\",\";`m:1]",
    expectation: ExampleExpectation::ResultContains("(\"a\";\"b,c\")"),
}];

const FIND_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Find the first matching path",
    code: "find[(1;2;3;2);2]",
    expectation: ExampleExpectation::ResultContains(",(,1)"),
}];

const RFIND_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Find the last matching path",
    code: "rfind[(1;2;3;2);2]",
    expectation: ExampleExpectation::ResultContains(",(,3)"),
}];

const ZIP_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Pair corresponding items",
    code: "zip[(1;2);(10;20)]",
    expectation: ExampleExpectation::ResultContains("((1;10);(2;20))"),
}];

pub(super) const SUM: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Sum,
    summary: "Add arguments or the items of one list.",
    details: "`sum` with multiple arguments adds those arguments left to right. With one non-atom argument, it adds that value's immediate items; empty lists sum to `0`, and atoms are returned unchanged.",
    examples: SUM_EXAMPLES,
    related: &["product", "+", "fold"],
};

pub(super) const PRODUCT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Product,
    summary: "Multiply arguments or the items of one list.",
    details: "`product` with multiple arguments multiplies those arguments left to right. With one non-atom argument, it multiplies that value's immediate items; empty lists multiply to `1`, and atoms are returned unchanged.",
    examples: PRODUCT_EXAMPLES,
    related: &["sum", "*", "fold"],
};

pub(super) const MIN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Min,
    summary: "Return the smallest comparable atom.",
    details: "`min[xs]` compares the immediate atom items of `xs`, skipping nested non-atoms; for dicts it compares values. `min[x;y;...]` compares the arguments directly. Empty inputs with no comparable atom return an empty list.",
    examples: MIN_EXAMPLES,
    related: &["max", "sort"],
};

pub(super) const MAX: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Max,
    summary: "Return the largest comparable atom.",
    details: "`max[xs]` compares the immediate atom items of `xs`, skipping nested non-atoms; for dicts it compares values. `max[x;y;...]` compares the arguments directly. Empty inputs with no comparable atom return an empty list.",
    examples: MAX_EXAMPLES,
    related: &["min", "sort"],
};

pub(super) const FLATTEN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Flatten,
    summary: "Return all leaves of a value as one list.",
    details: "`flatten` walks nested lists, returning the leaf values in traversal order. It is often useful before string-building or simple reductions over nested data.",
    examples: FLATTEN_EXAMPLES,
    related: &["sum", "map", "depth"],
};

pub(super) const REVERSE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Reverse,
    summary: "Reverse the top-level order of a value.",
    details: "`reverse` reverses lists, strings, and dict insertion order. Other values are returned unchanged. `V` is an alias of `reverse`.",
    examples: REVERSE_EXAMPLES,
    related: &["sort"],
};

pub(super) const SORT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Sort,
    summary: "Sort a list or dict.",
    details: "`sort` sorts lists of ints numerically and strings by Unicode scalar. General lists sort by string form when available, otherwise by atom comparison where possible. Dicts sort entries by value while preserving key-value associations. Use ``sort[dct;`by:`key]`` to sort a dict by key or ``sort[dct;`by:`value]`` to request the default explicitly. The `by` option requires a dict when set to the `` `key `` tag. Other values are returned unchanged.",
    examples: SORT_EXAMPLES,
    related: &["min", "max", "reverse", "keys", "values"],
};

pub(super) const SPLIT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Split,
    summary: "Split a value into groups.",
    details: "`split[xs]` splits strings on whitespace. `split[xs;delim]` splits strings and lists on a delimiter. The named `m` option limits the number of splits; `inf` means unlimited.",
    examples: SPLIT_EXAMPLES,
    related: &["splitw", "words"],
};

pub(super) const FIND: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Find,
    summary: "Find paths to matching values from the front.",
    details: "`find[xs;elem;threshold?;d?]` always returns a list of index paths to matching items. No match returns an empty list, and one match returns a one-item list containing its path. Threshold accepts non-negative ints or `inf`; depth defaults to `1`.",
    examples: FIND_EXAMPLES,
    related: &["rfind", "findw", "@depth"],
};

pub(super) const RFIND: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::RFind,
    summary: "Find paths to matching values from the back.",
    details: "`rfind` has the same return shape and arguments as `find`, but searches list and dict values from the end toward the front.",
    examples: RFIND_EXAMPLES,
    related: &["find", "rfindw", "@depth"],
};

pub(super) const ZIP: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Zip,
    summary: "Pair corresponding values from two structures.",
    details: "`zip[xs;ys;d?]` walks two values together and returns two-item pairs at the requested depth. The optional depth follows the same root and leaves model as `map`; with two inputs, depth is normalized against the deeper input.",
    examples: ZIP_EXAMPLES,
    related: &["zipw", "cart", "@depth"],
};
