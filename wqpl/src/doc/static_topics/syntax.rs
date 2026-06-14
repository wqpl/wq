use super::super::model::{DocExample, DocKind, ExampleExpectation, StaticDoc};

const ASSIGNMENT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Bind a value",
    code: "a:1;a",
    expectation: ExampleExpectation::ResultContains("1"),
}];

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
    id: "assignment",
    title: "Assignment",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &[":", "assignment", "binding"],
    summary: "Bind a name with `lhs:rhs`.",
    details: "A single equals sign is equality; colon performs assignment.",
    examples: ASSIGNMENT_EXAMPLES,
    related: &["equality"],
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
    related: &["assignment"],
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
    related: &[",", "len"],
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
    related: &["keys", "tag"],
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
    related: &["postfix", "pipes"],
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
    related: &["calls", "postfix", "map", "fold"],
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
    related: &["calls", "postfix", "precedence"],
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
    related: &["operators", "postfix", "pipes", "calls"],
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
