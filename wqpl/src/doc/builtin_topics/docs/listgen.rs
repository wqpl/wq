use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const ALLOC_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Fill a two-dimensional shape",
    code: "alloc[(2;3);7]",
    expectation: ExampleExpectation::ResultContains("((7;7;7);(7;7;7))"),
}];

const TIL_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Generate row-major indices",
    code: "til (2;3)",
    expectation: ExampleExpectation::ResultContains("((0;1;2);(3;4;5))"),
}];

const IOTA_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Generate coordinates for a shape",
    code: "iota (2;2)",
    expectation: ExampleExpectation::ResultContains("(((0;0);(0;1));((1;0);(1;1)))"),
}];

const RANGE_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Generate a stepped int range",
        code: "range[1;10;2]",
        expectation: ExampleExpectation::ResultContains("(1;3;5;7;9)"),
    },
    DocExample {
        title: "Generate a stepped char range",
        code: "range[\"a\";\"h\";2]",
        expectation: ExampleExpectation::ResultContains("\"aceg\""),
    },
];

const RESHAPE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Cycle flattened values into a new shape",
    code: "reshape[(1;2;3);5]",
    expectation: ExampleExpectation::ResultContains("(1;2;3;1;2)"),
}];

const TRANSPOSE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Swap matrix rows and columns",
    code: "transpose ((1;2;3);(4;5;6))",
    expectation: ExampleExpectation::ResultContains("((1;4);(2;5);(3;6))"),
}];

const REPEAT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Repeat a string",
    code: "repeat[\"ab\";3]",
    expectation: ExampleExpectation::ResultContains("\"ababab\""),
}];

const WHERE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Find non-zero ints and T bools",
    code: "where (0;2;F;T;3)",
    expectation: ExampleExpectation::ResultContains("(1;3;4)"),
}];

pub(super) const ALLOC: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Alloc,
    summary: "Allocate a value with a requested shape.",
    details: "`alloc[shape]` fills a requested shape of non-negative ints with `0`. `alloc[shape;x]` fills every leaf with `x`; common int shapes and int fills are cached.",
    examples: ALLOC_EXAMPLES,
    related: &["til", "iota", "reshape"],
};

pub(super) const TIL: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Til,
    summary: "Generate row-major int positions for a shape.",
    details: "`til[n]` returns the list of ints `0..n-1`. With a multi-axis shape, `til[shape]` fills that nested shape with consecutive ints in row-major order.",
    examples: TIL_EXAMPLES,
    related: &["iota", "alloc", "where"],
};

pub(super) const IOTA: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Iota,
    summary: "Generate indices or coordinate vectors for a shape.",
    details: "`iota[n]` is the one-dimensional index list `0..n-1`. With a multi-axis shape, `iota[shape]` returns nested lists of coordinates, one coordinate list for each leaf position.",
    examples: IOTA_EXAMPLES,
    related: &["til", "range", "shape", "where"],
};

pub(super) const RANGE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Range,
    summary: "Generate a half-open range.",
    details: "`range[start;end]` returns the same half-open range as `start..end`, inferring a positive or negative step from the bounds. `range[start;end;step]` uses an explicit step and errors when the step is zero or points away from the end. Char ranges return strings and use Unicode scalar order.",
    examples: RANGE_EXAMPLES,
    related: &["ranges", "til", "iota"],
};

pub(super) const RESHAPE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Reshape,
    summary: "Reshape a value by cycling its flattened leaves.",
    details: "`reshape[xs;shape]` flattens `xs`, then fills `shape` left to right. If the shape needs more leaves than `xs` has, values cycle from the beginning; empty input fills with `0`. `R` is an alias.",
    examples: RESHAPE_EXAMPLES,
    related: &["flatten", "alloc", "shape"],
};

pub(super) const TRANSPOSE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Transpose,
    summary: "Transpose a uniform nested list.",
    details: "`transpose[x]` leaves atoms and vectors unchanged, transposes a matrix, and swaps the last two axes of higher-rank lists. `transpose[x;axes]` maps each source axis to a result axis; repeated axes select diagonals. `TP` is an alias.",
    examples: TRANSPOSE_EXAMPLES,
    related: &["shape", "reshape"],
};

pub(super) const REPEAT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Repeat,
    summary: "Repeat a value a non-negative number of times.",
    details: "`repeat[xs;n]` repeats strings and char values as strings, repeats list contents into one longer list, and repeats atoms by returning a list of copies.",
    examples: REPEAT_EXAMPLES,
    related: &["alloc", "reshape", ","],
};

pub(super) const WHERE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Where,
    summary: "Return positions of non-zero int leaves and T bool leaves.",
    details: "`where[xs]` accepts lists whose leaves are ints or bools. In a flat vector it returns indices where an int leaf is non-zero or a bool leaf is `T`; in nested input it returns coordinate vectors for those leaves.",
    examples: WHERE_EXAMPLES,
    related: &["til", "iota", "find"],
};
