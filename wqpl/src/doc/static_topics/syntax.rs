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

const COMMENT_DETAILS: &str = "`// text` starts a line comment that runs to the next newline. `/* text */` starts a block comment that can appear between tokens, span multiple lines, or be empty as `/**/`. Block comments nest, so every `/*` inside a block needs its own matching `*/`. Comments are ignored as trivia during evaluation and are used for notes, expected output, and temporarily disabling source.";

const COMMENT_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Use a trailing line comment",
        code: "1+2 // ignored",
        expectation: ExampleExpectation::ResultContains("3"),
    },
    DocExample {
        title: "Put a block comment between tokens",
        code: "1 /* ignored */ + 2",
        expectation: ExampleExpectation::ResultContains("3"),
    },
    DocExample {
        title: "Nest block comments",
        code: "1 /* outer /* inner */ outer */ + 2",
        expectation: ExampleExpectation::ResultContains("3"),
    },
    DocExample {
        title: "Use an empty block comment",
        code: "1/**/+2",
        expectation: ExampleExpectation::ResultContains("3"),
    },
];

const CALL_DETAILS: &str = "`target[expr1;expr2]` applies a target to semicolon-separated arguments.
If the target is callable, that form is a call.
If the target is a list or dict, it is an index.
This shared shape is deliberate: a list or dict can be read as a discrete function from indexes or keys to values, so functions, builtins, lists, and dicts all use `target[...]` and the one-argument postfix form `target arg`.
Multiple bracket entries on an indexable target are a bulk index: `xs[0;2]` returns positions 0 and 2, and ``d[`a;`b]`` returns both dict keys.
An explicit list key such as `xs[(0;2)]` is one argument that ordinary lists also treat as multiple positions.
Index paths are deep: `xs[1][0]` and `xs[1] 0` first read `xs[1]`, then index that result.
For assignment, only the final path segment may be bulk; see `index-mutation`.
A trailing semicolon is legacy call-path syntax: `target[a;b;]` is forced to call, even when `target[a;b]` would index.
Postfix has one argument slot, so use brackets for zero arguments, named arguments, or more than one argument.";

const CALL_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Call a function with multiple arguments",
        code: "add:{[x;y]x+y};add[2;3]",
        expectation: ExampleExpectation::ResultContains("5"),
    },
    DocExample {
        title: "Index a list",
        code: "xs:(10;20;30);xs[1]",
        expectation: ExampleExpectation::ResultContains("20"),
    },
    DocExample {
        title: "Index a dict by key",
        code: "d:(`name:\"wq\";`fun:T);d`name",
        expectation: ExampleExpectation::ResultContains("\"wq\""),
    },
    DocExample {
        title: "Bulk index several list positions",
        code: "xs:(10;20;30;40);xs[0;2]",
        expectation: ExampleExpectation::ResultContains("(10;30)"),
    },
    DocExample {
        title: "Bulk index several dict keys",
        code: "d:(`a:1;`b:2);d[`a;`b]",
        expectation: ExampleExpectation::ResultContains("(1;2)"),
    },
    DocExample {
        title: "Descend through a deep index path",
        code: "xs:((1;2);(3;4));xs[1][0]",
        expectation: ExampleExpectation::ResultContains("3"),
    },
    DocExample {
        title: "Bulk index at a deep path",
        code: "xs:((1;2);(3;4));xs[1][0;1]",
        expectation: ExampleExpectation::ResultContains("(3;4)"),
    },
    DocExample {
        title: "A trailing semicolon forces the call path",
        code: "xs:(10;20;30);xs[0;1;]",
        expectation: ExampleExpectation::ErrorContains("cannot call 'xs'"),
    },
];

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
        code: "0..2..=10",
        expectation: ExampleExpectation::ResultContains("(0;2;4;6;8;10)"),
    },
    DocExample {
        title: "Char range",
        code: "@u\"a\"..=@u\"d\"",
        expectation: ExampleExpectation::ResultContains("\"abcd\""),
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
        title: "Assign through a deep index path",
        code: "xs:((0;0);(0;0));xs[0][0]:1;xs",
        expectation: ExampleExpectation::ResultContains("((1;0);(0;0))"),
    },
    DocExample {
        title: "Mix bracket and postfix path segments",
        code: "xs:((0;0);(0;0));xs[0] 1:2;xs",
        expectation: ExampleExpectation::ResultContains("((0;2);(0;0))"),
    },
    DocExample {
        title: "Bulk assign at a deep leaf",
        code: "xs:((0;0);(0;0));xs[0][0;1]:(2;3);xs",
        expectation: ExampleExpectation::ResultContains("((2;3);(0;0))"),
    },
    DocExample {
        title: "Pop the last item",
        code: "xs:(10;20;30);xs!;xs",
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

const POSTFIX_DETAILS: &str = "`target arg` is the one-expression form of `target[arg]` when writing it with a space does not change the parse.
It can call a function or index a container; the resolver lowers it from context when it can, and unresolved cases are decided at runtime.
Chaining is nested postfix, so `floor sqrt x` behaves like `floor[sqrt[x]]`.
Postfix binds before ordinary binary operators: `fn 1+2` means `(fn 1)+2`, not `fn[1+2]`.
Use grouping, brackets, or a pipe when the whole expression is the argument.
`fn arg1 arg2` is a chain, not a two-argument call; write `fn[arg1;arg2]` for that.";

const POSTFIX_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Call one argument with postfix",
        code: "{x*x} 9",
        expectation: ExampleExpectation::ResultContains("81"),
    },
    DocExample {
        title: "Index one argument with postfix",
        code: "xs:(10;20;30);xs 1",
        expectation: ExampleExpectation::ResultContains("20"),
    },
    DocExample {
        title: "Chain nested postfix calls",
        code: "floor sqrt 81",
        expectation: ExampleExpectation::ResultContains("9"),
    },
    DocExample {
        title: "Group a wider argument",
        code: "({x*x} 1+2;{x*x}(1+2))",
        expectation: ExampleExpectation::ResultContains("(3;9)"),
    },
];

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
        title: "Pipe as the first argument for divide",
        code: "10|/[2]",
        expectation: ExampleExpectation::ResultContains("5"),
    },
    DocExample {
        title: "Pipe as the last argument for divide",
        code: "10||/[2]",
        expectation: ExampleExpectation::ResultContains("0.2"),
    },
    DocExample {
        title: "Tap as the first argument",
        code: "10|./[2]",
        expectation: ExampleExpectation::ResultContains("10"),
    },
    DocExample {
        title: "Tap as the last argument",
        code: "10||./[2]",
        expectation: ExampleExpectation::ResultContains("10"),
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

const BOOLEAN_LOGIC_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Short-circuit boolean and",
        code: "and[F;raise \"unreached\"]",
        expectation: ExampleExpectation::ResultContains("F"),
    },
    DocExample {
        title: "Short-circuit boolean or",
        code: "or[T;raise \"unreached\"]",
        expectation: ExampleExpectation::ResultContains("T"),
    },
    DocExample {
        title: "Apply bitwise operations to bools",
        code: "(band[T;F];bor[T;F];bxor[T;F])",
        expectation: ExampleExpectation::ResultContains("(F;T;T)"),
    },
];

const CONDITIONAL_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Choose a branch",
        code: "$[1=1;2;3]",
        expectation: ExampleExpectation::ResultContains("2"),
    },
    DocExample {
        title: "Use a multi-expression false branch",
        code: "$[F;0;x:1;x]",
        expectation: ExampleExpectation::ResultContains("1"),
    },
    DocExample {
        title: "Run a multi-expression guard body",
        code: "$.[T;x:1;x]",
        expectation: ExampleExpectation::ResultContains("1"),
    },
    DocExample {
        title: "Choose from condition/branch pairs",
        code: "$$[F;\"a\";T;\"b\";\"c\"]",
        expectation: ExampleExpectation::ResultContains("b"),
    },
    DocExample {
        title: "Use an implicit unit chain default",
        code: "$$[F;\"a\";F;\"b\"]",
        expectation: ExampleExpectation::ResultContains("()"),
    },
];

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
    code: "[1;2]",
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
    details: "`name:expr` binds a value. Operator-colon forms such as `x+:1` update from the old value, and `xs,:x` appends with comma assignment. If a right operand runs code that mutates a binding used on the left, binary expressions and operator-colon updates still use the left value from before that mutation. List-shaped left sides unpack values, so `(a;b):(1;2)` binds both names; patterns may nest, and `...` skips the middle. Index targets can be assigned too, and `value|name:` checkpoints a pipe value under a name.",
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

pub(super) const COMMENTS: StaticDoc = StaticDoc {
    id: "comments",
    title: "Comments",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["comment", "comments", "//", "/*", "*/", "/* */", "/**/"],
    summary: "Ignore source text with line or block comments.",
    details: COMMENT_DETAILS,
    examples: COMMENT_EXAMPLES,
    related: &[],
};

pub(super) const CALLS: StaticDoc = StaticDoc {
    id: "calls",
    title: "Calls, Indexing, and Postfix",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &[
        "call",
        "calls",
        "index",
        "indexing",
        "bulk index",
        "deep index",
        "[]",
    ],
    summary: "Call functions or index containers with shared bracket and postfix syntax.",
    details: CALL_DETAILS,
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
    summary: "Build ranges for lists, loops, strings, and slices.",
    details: "`a..b` builds a half-open range that stops before `b`; `a..=b` includes the end. Use `a..next..b` or `a..next..=b` when you want a stride, as in `0..2..10` or `0..2..=10`. Numeric ranges produce lists of numbers. Char ranges return strings in Unicode scalar order, so `@u\"a\"..=@u\"d\"` is `\"abcd\"`. Ranges are ordinary values, but they are most often used as indexes and slices, such as `xs[1..3]`.",
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
    details: "`xs i:v` assigns through ordinary postfix indexing, and `xs i+:v` reads, updates, and writes the indexed element. Index chains assign through nested containers, so `xs[0][1]:v` and `xs[0] 1:v` descend into `xs[0]` and write index `1`; semicolons stay bulk assignment at that depth, so `xs[0;1]:v` still writes top-level positions while `xs[0][0;1]:v` writes multiple positions inside `xs[0]`. Bang indexing mutates list shape: `xs[!]` or `xs!` pops the last item, `xs[!i]` removes the item at `i`, `xs[!]:v` or `xs!:v` inserts between items, and `xs[!i]:v` inserts `v` at that position. These forms are useful for stack-like and in-place list workflows.",
    examples: INDEX_MUTATION_EXAMPLES,
    related: &["assignment-forms", "calls", "lists", "ranges"],
};

pub(super) const POSTFIX: StaticDoc = StaticDoc {
    id: "postfix",
    title: "Postfix Syntax",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["postfix", "postfix call", "postfix calls", "space call"],
    summary: "Apply or index a target with one following expression.",
    details: POSTFIX_DETAILS,
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
    aliases: &[
        "|",
        "||",
        "|.",
        "||.",
        "pipe",
        "pipes",
        "left pipe",
        "right pipe",
        "tap pipe",
    ],
    summary: "Pipe a value into the next call.",
    details: "`x|f[y]` behaves like `f[x;y]`. Most builtins are designed with the main data as the first argument, so `|` is the everyday pipe. `x||f[y]` behaves like `f[y;x]`, putting the value last; this matters most for calls such as `/` and `-`, where `10|/[2]` is `10/2` but `10||/[2]` is `2/10`. Dotted pipes `|.` and `||.` use the same first-argument or last-argument insertion, run the right-hand call, discard its result, and return the original left value. Pipes bind looser than calls and arithmetic, so `1+2|*[10]` pipes `3` into `*[10]`.",
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
    details: "From tight to loose: grouping/literals, postfix calls and indexing, power, unary operators, ranges, multiply/divide/modulo/matmul, add/subtract, comparisons, comma, pipes, and assignment. Use named calls such as `band[x;y]`, `bor[x;y]`, `bxor[x;y]`, `shl[x;y]`, and `shr[x;y]` for bitwise operations. Lazy boolean forms are special bracket syntax: `A[x;y]` and its `and[x;y]` alias short-circuit and, while `O[x;y]` and its `or[x;y]` alias short-circuit or. Postfix binds before binary operators, so `fn 1+2` means `(fn 1)+2`; use `fn(1+2)`, `fn[1+2]`, or `1+2|fn` when the whole expression is the argument.",
    examples: PRECEDENCE_EXAMPLES,
    related: &["operators", "postfix", "pipes", "calls", "ranges"],
};

pub(super) const BOOLEAN_LOGIC: StaticDoc = StaticDoc {
    id: "boolean-logic",
    title: "Boolean and Bitwise Logic",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["A", "O", "and", "or", "boolean", "logical"],
    summary: "Combine bools lazily or apply eager bitwise operations.",
    details: "`A[xs;ys+]` and `O[xs;ys+]` combine bool expressions with short-circuit evaluation.
`and[xs;ys+]` and `or[xs;ys+]` are aliases.
`A`, `and`, `O`, and `or` are reserved names.
`band[xs;ys+]`, `bor[xs;ys+]`, and `bxor[xs;ys+]` eagerly fold bitwise and, or, and xor over integers, bools, and compatible lists of them.",
    examples: BOOLEAN_LOGIC_EXAMPLES,
    related: &["conditionals", "precedence", "not", "all", "any"],
};

pub(super) const CONDITIONALS: StaticDoc = StaticDoc {
    id: "conditionals",
    title: "Conditionals",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["$", "$.", "$$", "conditional", "conditionals"],
    summary: "Choose between branches with dollar forms.",
    details: "`$[c;t;f]` is a ternary; extra expressions after `f` are part of the false branch, so `$[c;t;f1;f2]` runs `f1` then returns `f2` when `c` is false. `$.[c;t1;t2...]` is a guard-like conditional that runs its body only when `c` is true and otherwise returns unit. `$$[c1;t1;c2;t2;default]` checks condition/branch pairs in order; the final default is optional, and an omitted default is unit.",
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
    details: "`N[n;body]` exposes `_n` as the zero-based iteration counter. `N` is a reserved name.",
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
    details: "`W[cond;body]` requires `cond` to evaluate to a bool. `W` is a reserved name.",
    examples: W_LOOP_EXAMPLES,
    related: &["@b", "@c"],
};

pub(super) const BLOCK: StaticDoc = StaticDoc {
    id: "block",
    title: "Block",
    kind: DocKind::Syntax,
    group: "Syntax",
    aliases: &["B", "B block", "block"],
    summary: "Evaluate statements as a single expression.",
    details:
        "`[...]` groups multiple statements in expression positions such as condition branches.
`B[...]` is an alternative spelling.
`B` is a reserved name.",
    examples: BLOCK_EXAMPLES,
    related: &["conditionals"],
};
