use super::super::model::{DocExample, DocKind, ExampleExpectation, StaticDoc};

const ASSIGNMENT_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Bind and update a value",
        code: "x:1;x+:4;x",
        expectation: ExampleExpectation::ResultContains("5"),
    },
    DocExample {
        title: "Append with comma assignment",
        code: "xs:(1;2);xs,:3;xs",
        expectation: ExampleExpectation::ResultContains("(1;2;3)"),
    },
    DocExample {
        title: "Unpack nested values",
        code: "(a;(b;c)):(1;(2;3));a+b+c",
        expectation: ExampleExpectation::ResultContains("6"),
    },
    DocExample {
        title: "Skip the middle with ellipsis",
        code: "(head;...;tail):(1;2;3;4);(head;tail)",
        expectation: ExampleExpectation::ResultContains("(1;4)"),
    },
    DocExample {
        title: "Checkpoint a pipe value",
        code: "10|x:;x",
        expectation: ExampleExpectation::ResultContains("10"),
    },
];

const EQUALITY_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Compare two values",
    code: "1=1",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const LIST_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Create and index a list",
    code: "(10;20;30) 1",
    expectation: ExampleExpectation::ResultContains("20"),
}];

const DICT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Create and read a dict",
    code: "(`a:1;`b:2)`a",
    expectation: ExampleExpectation::ResultContains("1"),
}];

const CALL_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Bracket call/index syntax",
    code: "(10;20;30)[1]",
    expectation: ExampleExpectation::ResultContains("20"),
}];

const RANGE_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Half-open range",
        code: "0..3",
        expectation: ExampleExpectation::ResultContains("(0;1;2)"),
    },
    DocExample {
        title: "Inclusive range",
        code: "0..=3",
        expectation: ExampleExpectation::ResultContains("(0;1;2;3)"),
    },
    DocExample {
        title: "Stepped range",
        code: "0..=10..2",
        expectation: ExampleExpectation::ResultContains("(0;2;4;6;8;10)"),
    },
    DocExample {
        title: "Slice with a range",
        code: "xs:(10;20;30;40);xs[1..3]",
        expectation: ExampleExpectation::ResultContains("(20;30)"),
    },
];

const INDEX_MUTATION_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Assign through an index",
        code: "xs:(10;20;30);xs 1:99;xs",
        expectation: ExampleExpectation::ResultContains("(10;99;30)"),
    },
    DocExample {
        title: "Update through an index",
        code: "xs:(10;20;30);xs 1+:5;xs",
        expectation: ExampleExpectation::ResultContains("(10;25;30)"),
    },
    DocExample {
        title: "Pop the last item",
        code: "xs:(10;20;30);xs[!];xs",
        expectation: ExampleExpectation::ResultContains("(10;20)"),
    },
    DocExample {
        title: "Remove an item by index",
        code: "xs:(10;20;30);xs[!1];xs",
        expectation: ExampleExpectation::ResultContains("(10;30)"),
    },
    DocExample {
        title: "Insert at an index",
        code: "xs:(10;30);xs[!1]:20;xs",
        expectation: ExampleExpectation::ResultContains("(10;20;30)"),
    },
];

const POSTFIX_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Single-argument postfix call",
    code: "{x*x} 9",
    expectation: ExampleExpectation::ResultContains("81"),
}];

const FUNCTION_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Call an explicit-parameter function",
        code: "add:{[x;y]x+y};add[2;3]",
        expectation: ExampleExpectation::ResultContains("5"),
    },
    DocExample {
        title: "Use implicit x in a mapper",
        code: "(1;2;3)|map{x*x}",
        expectation: ExampleExpectation::ResultContains("(1;4;9)"),
    },
];

const NAMED_ARGUMENT_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Use a named default",
        code: "scale:{[x;`by:2]x*by};(scale[3];scale[3;`by:4])",
        expectation: ExampleExpectation::ResultContains("(6;12)"),
    },
    DocExample {
        title: "Pass named arguments out of order",
        code: "size:{[`width:40;`height:10]width+height};size[`height:2;`width:3]",
        expectation: ExampleExpectation::ResultContains("5"),
    },
    DocExample {
        title: "Use tags as dict keys",
        code: "(`name:\"wq\";`fun:T)`fun",
        expectation: ExampleExpectation::ResultContains("T"),
    },
];

const PIPE_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Pipe as the first argument",
        code: "10|-[3]",
        expectation: ExampleExpectation::ResultContains("7"),
    },
    DocExample {
        title: "Pipe as the last argument",
        code: "10||-[3]",
        expectation: ExampleExpectation::ResultContains("-7"),
    },
    DocExample {
        title: "Tap returns the original value",
        code: "5|.{x+1}",
        expectation: ExampleExpectation::ResultContains("5"),
    },
];

const PRECEDENCE_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Math operators group by precedence",
        code: "2+3*4",
        expectation: ExampleExpectation::ResultContains("14"),
    },
    DocExample {
        title: "Postfix calls bind before addition",
        code: "({x*x} 1+2;{x*x}(1+2))",
        expectation: ExampleExpectation::ResultContains("(3;9)"),
    },
];

const CONDITIONAL_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Choose a branch",
    code: "$[1=1;2;3]",
    expectation: ExampleExpectation::ResultContains("2"),
}];

const N_LOOP_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Repeat with a counter",
    code: "N[3;_n]",
    expectation: ExampleExpectation::ResultContains("2"),
}];

const W_LOOP_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Repeat while true",
    code: "i:0;W[i<3;i+:1]",
    expectation: ExampleExpectation::ResultContains("3"),
}];

const BLOCK_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate a statement block",
    code: "B[1;2]",
    expectation: ExampleExpectation::ResultContains("2"),
}];

pub(super) const ASSIGNMENT: StaticDoc = StaticDoc {
    id: "assignment-forms",
    title: "Assignment Forms",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &[
        ":",
        "assignment",
        "assign",
        "binding",
        "compound assignment",
        "augmented assignment",
        "unpack",
    ],
    summary: "Bind, update, unpack, or checkpoint values with assignment forms.",
    details: "`name:expr` binds a value. Operator-colon forms such as `x+:1` update from the old value, and `xs,:x` appends with comma assignment. List-shaped left sides unpack values, so `(a;b):(1;2)` binds both names; patterns may nest, and `...` skips the middle. Index targets can be assigned too, and `value|name:` checkpoints a pipe value under a name.",
    examples: ASSIGNMENT_EXAMPLES,
    related: &["equality", "index-mutation", "pipes"],
};

pub(super) const EQUALITY: StaticDoc = StaticDoc {
    id: "equality",
    title: "Equality",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["=", "equality", "equal"],
    summary: "Compare values with `=`.",
    details: "`a=b` is equality. Use `a:b` for assignment.",
    examples: EQUALITY_EXAMPLES,
    related: &["assignment-forms"],
};

pub(super) const LISTS: StaticDoc = StaticDoc {
    id: "lists",
    title: "Lists",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["list", "lists", "()"],
    summary: "Create lists with semicolon-separated parentheses.",
    details: "`(1;2;3)` is a list. `(1)` is just the atom `1`; use leading comma to enlist a single value.",
    examples: LIST_EXAMPLES,
    related: &[",", "len", "ranges", "index-mutation"],
};

pub(super) const DICTS: StaticDoc = StaticDoc {
    id: "dicts",
    title: "Dicts",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["dict", "dicts", "dictionary"],
    summary: "Create dictionaries with symbol keys.",
    details: "Dict keys are tags, written with a leading backtick. The empty dict is (`).",
    examples: DICT_EXAMPLES,
    related: &["keys", "named-arguments"],
};

pub(super) const CALLS: StaticDoc = StaticDoc {
    id: "calls",
    title: "Calls and Indexing",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["call", "calls", "index", "indexing", "[]"],
    summary: "Call or index with brackets and semicolons.",
    details: "`target[expr1;expr2]` passes multiple arguments or indexes multiple positions depending on the target value.",
    examples: CALL_EXAMPLES,
    related: &[
        "postfix",
        "pipes",
        "named-arguments",
        "ranges",
        "index-mutation",
    ],
};

pub(super) const RANGES: StaticDoc = StaticDoc {
    id: "ranges",
    title: "Ranges",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["range", "ranges", "slice", "slices", "..", "..="],
    summary: "Build integer ranges for lists, loops, and slices.",
    details: "`a..b` builds a half-open range that stops before `b`; `a..=b` includes the end. Add a second range marker for the step, as in `0..10..2` or `0..=10..2`. Ranges are ordinary values, but they are most often used as indexes and slices, such as `xs[1..3]`.",
    examples: RANGE_EXAMPLES,
    related: &["lists", "calls", "index-mutation", "precedence"],
};

pub(super) const INDEX_MUTATION: StaticDoc = StaticDoc {
    id: "index-mutation",
    title: "Index Mutation",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["index assignment", "mutation", "mutating index", "[!]"],
    summary: "Mutate list contents through index assignment and bang indexing.",
    details: "`xs i:v` assigns through ordinary postfix indexing, and `xs i+:v` reads, updates, and writes the indexed element. Bang indexing mutates list shape: `xs[!]` pops the last item, `xs[!i]` removes the item at `i`, and `xs[!i]:v` inserts `v` at that position. These forms are useful for stack-like and in-place list workflows.",
    examples: INDEX_MUTATION_EXAMPLES,
    related: &["assignment-forms", "calls", "lists", "ranges"],
};

pub(super) const POSTFIX: StaticDoc = StaticDoc {
    id: "postfix",
    title: "Postfix Calls",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["postfix", "postfix call"],
    summary: "A function followed by one expression calls it.",
    details: "`fn arg` is a one-argument call. `fn1 fn2 arg` chains calls. `fn arg1 arg2` is not a two-argument call.",
    examples: POSTFIX_EXAMPLES,
    related: &["calls"],
};

pub(super) const FUNCTIONS: StaticDoc = StaticDoc {
    id: "functions",
    title: "Functions",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["function", "functions", "fn", "lambda", "closure", "{}"],
    summary: "Create function values with braces.",
    details: "Function literals use braces: `{body}` creates a function with implicit `x`, `y`, and `z`, while `{[a;b] body}` declares parameters. Use `{[] body}` for a no-argument function. Functions are values, so bind them with `name:{...}`, pass them to higher-order builtins, or call them with `fn[arg]` and `fn arg`.",
    examples: FUNCTION_EXAMPLES,
    related: &["calls", "postfix", "named-arguments", "map", "fold"],
};

pub(super) const NAMED_ARGUMENTS: StaticDoc = StaticDoc {
    id: "named-arguments",
    title: "Named Arguments",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &[
        "named argument",
        "named arguments",
        "named arg",
        "named args",
        "tag",
        "tags",
        "`name:value",
    ],
    summary: "Use backtick tags for named call arguments and dict keys.",
    details: "A tag is a backtick-prefixed name. In a call, a backtick-tagged `name:value` pair passes a named argument; in a function parameter list, a backtick-tagged `name:default` declares a named parameter with a default. Named call arguments may appear out of order, and each callee decides which names it accepts. The same tag syntax also names dict keys, so `(`a:1)`a` reads the value stored under `a`.",
    examples: NAMED_ARGUMENT_EXAMPLES,
    related: &["functions", "calls", "dicts"],
};

pub(super) const PIPES: StaticDoc = StaticDoc {
    id: "pipes",
    title: "Pipes",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["|", "||", "|.", "||.", "pipe", "pipes", "tap pipe"],
    summary: "Pipe a value into the next call.",
    details: "`x | f[y]` behaves like `f[x;y]`, while `x || f[y]` behaves like `f[y;x]`. Dotted pipes `|.` and `||.` run the right-hand call but return the original left value, which is useful for tracing or side effects inside a pipeline. Pipes bind looser than calls and arithmetic, so `1+2|*[10]` pipes `3` into `*[10]`.",
    examples: PIPE_EXAMPLES,
    related: &["calls", "postfix", "assignment-forms", "precedence"],
};

pub(super) const PRECEDENCE: StaticDoc = StaticDoc {
    id: "precedence",
    title: "Precedence",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &[
        "precedence",
        "operator precedence",
        "binding power",
        "order of operations",
    ],
    summary: "Understand which syntax groups first.",
    details: "From tight to loose: grouping/literals, postfix calls and indexing, power, unary operators, ranges, multiply/divide/modulo/matmul, add/subtract, shifts, bitwise operators, comparisons, bool `&|` then `\\|`, comma, pipes, and assignment. Postfix binds before binary operators, so `fn 1+2` means `(fn 1)+2`; use `fn(1+2)`, `fn[1+2]`, or `1+2|fn` when the whole expression is the argument.",
    examples: PRECEDENCE_EXAMPLES,
    related: &["operators", "postfix", "pipes", "calls", "ranges"],
};

pub(super) const CONDITIONALS: StaticDoc = StaticDoc {
    id: "conditionals",
    title: "Conditionals",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["$", "$.", "$$", "conditional", "conditionals"],
    summary: "Choose between branches with dollar forms.",
    details: "`$[c;t;f]` is a ternary. `$.[c;t]` is a guard-like conditional. `$$[...]` chains condition/action pairs.",
    examples: CONDITIONAL_EXAMPLES,
    related: &["bool"],
};

pub(super) const N_LOOP: StaticDoc = StaticDoc {
    id: "n-loop",
    title: "N Loop",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["N", "N loop", "n-loop"],
    summary: "Repeat a body a fixed number of times.",
    details: "`N[n;body]` exposes `_n` as the zero-based iteration counter.",
    examples: N_LOOP_EXAMPLES,
    related: &["@b", "@c"],
};

pub(super) const W_LOOP: StaticDoc = StaticDoc {
    id: "w-loop",
    title: "W Loop",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["W", "W loop", "w-loop"],
    summary: "Repeat while a bool condition remains true.",
    details: "`W[cond;body]` requires `cond` to evaluate to a bool.",
    examples: W_LOOP_EXAMPLES,
    related: &["@b", "@c"],
};

pub(super) const BLOCK: StaticDoc = StaticDoc {
    id: "block",
    title: "B Block",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["B", "B block", "block"],
    summary: "Evaluate statements as a single expression.",
    details: "`B[...]` groups multiple statements in expression positions such as condition branches.",
    examples: BLOCK_EXAMPLES,
    related: &["conditionals"],
};
