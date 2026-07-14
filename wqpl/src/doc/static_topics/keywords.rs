use super::super::model::{DocExample, DocKind, ExampleExpectation, StaticDoc};

const AT_ASSERT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Assert a condition",
    code: "@a 1=1",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const AT_BREAK_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Break from the nearest loop",
    code: "i:0;N[10;$.[_n=3;@b];i+:1];i",
    expectation: ExampleExpectation::ResultContains("3"),
}];

const AT_CONTINUE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Skip one loop iteration",
    code: "i:0;N[5;$.[_n=2;@c];i+:1];i",
    expectation: ExampleExpectation::ResultContains("4"),
}];

const AT_RETURN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Return early from a function",
    code: "{[x]$.[x=0;@r -1];x}0",
    expectation: ExampleExpectation::ResultContains("-1"),
}];

const AT_DEBUG_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Trace an expression",
    code: "@d 1+2",
    expectation: ExampleExpectation::NoRun("prints a debug trace"),
}];

const AT_PAUSE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Pause in the debugger",
    code: "@p",
    expectation: ExampleExpectation::NoRun("enters wqdb"),
}];

const AT_TRY_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Capture a structured error",
    code: "@t 1/0",
    expectation: ExampleExpectation::ResultContains("`error"),
}];

const AT_SYMBOLIC_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Create a CAS expression",
        code: "type @s x+1",
        expectation: ExampleExpectation::ResultContains("\"cas\""),
    },
    DocExample {
        title: "Call a single-variable expression",
        code: "(@s x^2+1)[3]",
        expectation: ExampleExpectation::ResultContains("10"),
    },
    DocExample {
        title: "Quote an opaque algebraic root",
        code: "@s root[_^3-_-1;1;2]",
        expectation: ExampleExpectation::ResultContains("root[_^3 - _ - 1;1;2]"),
    },
];

const AT_FSTRING_DETAILS: &str = concat!(
    "`@f\"...{expr}...\"` is inline formatting. Braces contain wq expressions, ",
    "and `{[spec]expr}` formats the expression with the same spec accepted by ",
    "`fmt` placeholders. Dynamic width and precision use expressions inside ",
    "the spec, such as `{[>{width}.2]value}`. Use doubled braces for literal ",
    "braces.\n\n",
    "Spec contents are `[fill][align][sign][#][0][width][.precision][type]`. ",
    "Align is `<`, `>`, `^`, or `=`. Sign is `+`, `-`, or a space. ",
    "`#` adds integer base prefixes and selects pretty debug with `?`; `0` ",
    "is sign-aware zero padding. Type is `b`, `B`, `o`, `O`, `x`, `X`, ",
    "`e`, `E`, `,`, `%`, or `?`. See `fmt` for the same spec in template form."
);

const AT_FSTRING_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Interpolate an expression",
        code: "@f\"{1+2}\"",
        expectation: ExampleExpectation::ResultContains("3"),
    },
    DocExample {
        title: "Use the shared format spec",
        code: "@f\"hex={[#06x]255}\"",
        expectation: ExampleExpectation::ResultContains("hex=0x00ff"),
    },
    DocExample {
        title: "Use an expression for dynamic width",
        code: "width:6;pi:3.14159;@f\"pi={[>{width}.2]pi}\"",
        expectation: ExampleExpectation::ResultContains("pi=  3.14"),
    },
    DocExample {
        title: "Escape literal braces",
        code: "@f\"{{{1+2}}}\"",
        expectation: ExampleExpectation::ResultContains("{3}"),
    },
];

const AT_RAW_STRING_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Keep backslashes raw",
    code: "len @l\"\\n\"",
    expectation: ExampleExpectation::ResultContains("2"),
}];

const AT_UNICODE_SCALAR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Create a char atom",
    code: "type @u\"a\"",
    expectation: ExampleExpectation::ResultContains("char"),
}];

const AT_DEPTH_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Search one level deep",
        code: "(1;2;3)|has?@1[2]",
        expectation: ExampleExpectation::ResultContains("T"),
    },
    DocExample {
        title: "Map items at depth one",
        code: "((1;2);(3;4))|map@1[{sum x}]",
        expectation: ExampleExpectation::ResultContains("(3;7)"),
    },
    DocExample {
        title: "Map leaves with negative depth",
        code: "((1;2);(3;4))|map@-1[{x+10}]",
        expectation: ExampleExpectation::ResultContains("((11;12);(13;14))"),
    },
];

pub(super) const AT_ASSERT: StaticDoc = StaticDoc {
    id: "at-assert",
    title: "@a Assert",
    kind: DocKind::Keyword,
    group: "Keywords",
    aliases: &["@a", "assert"],
    summary: "Assert that an expression is true.",
    details: "`@a expr` evaluates `expr` and raises if it is false. It is useful for executable examples and invariants.",
    examples: AT_ASSERT_EXAMPLES,
    related: &["@t", "raise"],
};

pub(super) const AT_BREAK: StaticDoc = StaticDoc {
    id: "at-break",
    title: "@b Break",
    kind: DocKind::Keyword,
    group: "Keywords",
    aliases: &["@b", "break"],
    summary: "Leave the nearest enclosing loop.",
    details: "`@b` is only valid inside a loop body. It applies to the nearest loop.",
    examples: AT_BREAK_EXAMPLES,
    related: &["@c", "N", "W"],
};

pub(super) const AT_CONTINUE: StaticDoc = StaticDoc {
    id: "at-continue",
    title: "@c Continue",
    kind: DocKind::Keyword,
    group: "Keywords",
    aliases: &["@c", "continue"],
    summary: "Skip to the next loop iteration.",
    details: "`@c` is only valid inside a loop body. It applies to the nearest loop.",
    examples: AT_CONTINUE_EXAMPLES,
    related: &["@b", "N", "W"],
};

pub(super) const AT_RETURN: StaticDoc = StaticDoc {
    id: "at-return",
    title: "@r Return",
    kind: DocKind::Keyword,
    group: "Keywords",
    aliases: &["@r", "return"],
    summary: "Return early from the current function.",
    details: "`@r value` exits immediately with `value`. Bare `@r` returns unit.",
    examples: AT_RETURN_EXAMPLES,
    related: &["functions"],
};

pub(super) const AT_DEBUG: StaticDoc = StaticDoc {
    id: "at-debug",
    title: "@d Debug",
    kind: DocKind::Keyword,
    group: "Keywords",
    aliases: &["@d", "debug"],
    summary: "Evaluate an expression while printing a trace.",
    details: "`@d expr` is a runtime debugging probe. It yields the expression value after showing trace information. For values stored in shared Arc-backed storage, the output includes `strong=N`, the reference count observed when the value was traced. Immediate atoms do not show a strong count.",
    examples: AT_DEBUG_EXAMPLES,
    related: &["@p"],
};

pub(super) const AT_PAUSE: StaticDoc = StaticDoc {
    id: "at-pause",
    title: "@p Pause",
    kind: DocKind::Keyword,
    group: "Keywords",
    aliases: &["@p", "pause"],
    summary: "Pause execution in wqdb.",
    details: "`@p` optionally accepts an expression and then pauses execution when debugging is enabled.",
    examples: AT_PAUSE_EXAMPLES,
    related: &["@d"],
};

pub(super) const AT_TRY: StaticDoc = StaticDoc {
    id: "at-try",
    title: "@t Try",
    kind: DocKind::Keyword,
    group: "Keywords",
    aliases: &["@t", "try"],
    summary: "Capture success or failure as a tagged result.",
    details: "`@t expr` returns ``(`ok; value)`` on success or ``(`error; error_dict)`` on failure.
The error dict has stable `version`, `kind`, `message`, `source`, `span`, `notes`, `data`, `stack`, and `cause` fields.
Errors with kind `vm` are not caught.
Return, break, and continue remain control flow and are not caught as errors.",
    examples: AT_TRY_EXAMPLES,
    related: &["raise"],
};

pub(super) const AT_SYMBOLIC: StaticDoc = StaticDoc {
    id: "at-symbolic",
    title: "@s Symbolic",
    kind: DocKind::Keyword,
    group: "Keywords",
    aliases: &["@s", "symbolic", "cas"],
    summary: "Quote an expression into a symbolic CAS value.",
    details: "Use `@s` once at the start of a CAS expression, then apply CAS builtins directly. Bare arithmetic without `@s` is normal evaluation. Single-variable CAS expressions can be called with one positional argument, while named arguments bind symbols by name. CAS-only special forms such as `root[...]` are recognized inside `@s` quoting and are not ordinary builtins.",
    examples: AT_SYMBOLIC_EXAMPLES,
    related: &["diff", "integrate", "simplify", "numeric"],
};

pub(super) const AT_FSTRING: StaticDoc = StaticDoc {
    id: "at-fstring",
    title: "@f Format String",
    kind: DocKind::Keyword,
    group: "Keywords",
    aliases: &["@f", "format string", "fstring"],
    summary: "Create a string by interpolating expressions.",
    details: AT_FSTRING_DETAILS,
    examples: AT_FSTRING_EXAMPLES,
    related: &["fmt", "str"],
};

pub(super) const AT_RAW_STRING: StaticDoc = StaticDoc {
    id: "at-raw-string",
    title: "@l Raw String",
    kind: DocKind::Keyword,
    group: "Keywords",
    aliases: &["@l", "raw string"],
    summary: "Read a string without escape processing.",
    details: "`@l\"...\"` keeps backslashes as ordinary characters.",
    examples: AT_RAW_STRING_EXAMPLES,
    related: &["@f"],
};

pub(super) const AT_UNICODE_SCALAR: StaticDoc = StaticDoc {
    id: "at-unicode-scalar",
    title: "@u Unicode Scalar",
    kind: DocKind::Keyword,
    group: "Keywords",
    aliases: &["@u", "unicode scalar", "char", "character"],
    summary: "Create a char atom from one Unicode scalar.",
    details: "`@u\"...\"` decodes escapes and requires exactly one Unicode scalar. Ordinary quoted literals are strings at every length, so `\"a\"` is a one-character string while `@u\"a\"` is a char atom. Hex escapes require exactly two digits, such as `\\x41`; malformed hex and Unicode escapes are syntax errors. A user-perceived character may contain more than one Unicode scalar; use `graphemes` when that distinction matters.",
    examples: AT_UNICODE_SCALAR_EXAMPLES,
    related: &["@l", "chr", "ord", "graphemes"],
};

pub(super) const AT_DEPTH: StaticDoc = StaticDoc {
    id: "at-depth",
    title: "@depth Modifier",
    kind: DocKind::Keyword,
    group: "Keywords",
    aliases: &["@depth", "@1", "@2", "@-1", "depth modifier"],
    summary: "Append a depth argument to depth-aware builtin calls.",
    details: "`@1`, `@2`, `@-1`, and other signed integer depth modifiers are postfix call modifiers. They are valid only on builtins whose metadata declares depth sugar, and they append the depth as an ordinary final argument. A non-negative depth is relative to the container root: `0` means the container itself, `1` means the immediate items of the container, and `2` means one layer deeper. A negative depth is relative to the leaves: `-1` means the deepest items, `-2` means their parent layer, and values beyond the measured depth clamp at the root. Builtins that accept the full depth model also accept explicit `inf` for leaves and `-inf` for the root; check each builtin's argument docs for any narrower depth domain. Most depth-aware traversal defaults to depth `1`; broadcast comparison operators such as `=.` and `~.` are the depth-1, element-wise counterparts to whole-value `=` and `~`.",
    examples: AT_DEPTH_EXAMPLES,
    related: &["depth", "map", "has?", "find", "=."],
};
