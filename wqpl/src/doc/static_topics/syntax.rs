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

const PIPE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Pipe into a call",
    code: "(1+2)|*[10]",
    expectation: ExampleExpectation::ResultContains("30"),
}];

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

pub(super) const PIPES: StaticDoc = StaticDoc {
    id: "pipes",
    title: "Pipes",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["|", "pipe", "pipes"],
    summary: "Pipe inserts the left value as the first argument to a right-hand call.",
    details: "`x | f[y]` behaves like `f[x;y]`. Pipe syntax is often the clearest way to apply display or transformation builtins to larger expressions.",
    examples: PIPE_EXAMPLES,
    related: &["calls", "postfix"],
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
