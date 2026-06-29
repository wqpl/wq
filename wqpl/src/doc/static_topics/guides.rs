use super::super::model::{DocExample, DocKind, ExampleExpectation, StaticDoc};

const OPERATOR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Operators are functions too",
    code: "+[1;2;3]",
    expectation: ExampleExpectation::ResultContains("6"),
}];

const BUILTIN_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Ask code which builtins are enabled",
        code: "bfn[]|has?[\"echo\"]",
        expectation: ExampleExpectation::ResultContains("T"),
    },
    DocExample {
        title: "Show enabled builtins in the REPL",
        code: r"\bfn",
        expectation: ExampleExpectation::NoRun(
            "REPL command: shows the current preset and enabled builtin table",
        ),
    },
    DocExample {
        title: "Switch builtin preset in the REPL",
        code: r"\bfn pure",
        expectation: ExampleExpectation::NoRun("REPL command: switches to the pure builtin preset"),
    },
];

const INTERPRETER_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Run one command with the profiler interpreter",
        code: "wq exec '1+1' -i profiler -p",
        expectation: ExampleExpectation::NoRun(
            "CLI command: prints the normal result and a profile summary",
        ),
    },
    DocExample {
        title: "Show the current REPL interpreter",
        code: r"\interpreter",
        expectation: ExampleExpectation::NoRun(
            "REPL command: shows the current interpreter and available names",
        ),
    },
    DocExample {
        title: "Switch interpreter in the REPL",
        code: r"\i sample",
        expectation: ExampleExpectation::NoRun("REPL command: switches to the sample interpreter"),
    },
];

const WQDB_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Pause from source",
    code: "@p x",
    expectation: ExampleExpectation::NoRun("pauses in the host debugger"),
}];

pub(super) const BUILTINS: StaticDoc = StaticDoc {
    id: "builtins",
    title: "Builtins",
    kind: DocKind::Guide,
    group: "Reference",
    aliases: &["bfn", "builtin", "builtins", "builtin presets", r"\bfn", "\\"],
    summary: "Built-in functions are values provided by wq.",
    details: "Builtins can be called with bracket syntax, postfix syntax for one argument, or through pipes.
Individual builtin pages always render their signature and arity from `builtins.rs` metadata.
The `bfn[]` builtin returns a sorted list of builtin names enabled in the current preset, which lets wq code inspect its own runtime surface.
The standard CLI and REPL expose four preset names: `all`, `pure`, `minimal`, and `constrained`; short names `a`, `p`, `m`, and `c` are accepted where a preset is parsed.
`all` enables every builtin, `minimal` keeps only intrinsic operators, `pure` keeps pure builtin groups, and `constrained` keeps groups allowed by constrained hosts.
At the command line, `--builtins <preset>` selects the initial preset.
In the interactive REPL, `\\bfn` or `\\` shows the current preset and enabled builtin table, while `\\bfn pure`, `\\bfn minimal`, and similar commands switch the live session preset.",
    examples: BUILTIN_EXAMPLES,
    related: &["bfn", "operators", "calls"],
};

pub(super) const OPERATORS: StaticDoc = StaticDoc {
    id: "operators",
    title: "Operators",
    kind: DocKind::Guide,
    group: "Reference",
    aliases: &["operator", "operators", "+", "-", "*", "/", ","],
    summary: "Operators are also builtin functions.",
    details: "Most binary operators broadcast over compatible values. The comma operator concatenates, while leading comma enlists a value.",
    examples: OPERATOR_EXAMPLES,
    related: &["builtins", "lists", "pipes"],
};

pub(super) const INTERPRETERS: StaticDoc = StaticDoc {
    id: "interpreters",
    title: "Interpreters",
    kind: DocKind::Guide,
    group: "Reference",
    aliases: &[
        "interpreter",
        "interpreters",
        "-i",
        "--interpreter",
        r"\interpreter",
        r"\i",
        "vanilla",
        "sample",
        "profiler",
    ],
    summary: "Choose how VM instructions are executed and observed.",
    details: "wq currently has three instruction interpreters.
`vanilla` (`v`) is the default interpreter and is the normal choice for scripts, `exec`, the REPL, wqdb, and tests.
`profiler` (`p`) runs with the same user-visible evaluation result as `vanilla`, but installs profiling hooks and prints a profile summary to stderr when the run finishes; it reports instruction counts, stack and call depth, cache activity, and allocation summaries.
`sample` (`s`) also delegates to vanilla execution, but samples instructions into terminal art on stderr; `WQ_SAMPLE_ART=off`, `static` or `final`, and `on` or `animate` control whether it is quiet, final-only, or animated.
Select an interpreter with `-i <name>` or `--interpreter <name>` on the CLI, or with `\\interpreter <name>` / `\\i <name>` in the REPL.
`\\interpreter` with no name shows the current interpreter and the available names.",
    examples: INTERPRETER_EXAMPLES,
    related: &["wqdb", "builtins", "debug"],
};

pub(super) const WQDB: StaticDoc = StaticDoc {
    id: "wqdb",
    title: "wqdb",
    kind: DocKind::Guide,
    group: "Debugging",
    aliases: &["debugger", "debugging"],
    summary: "wqdb is the source-level debugger used by wq hosts.",
    details: "wqdb pauses execution at source locations, records backtraces and locals, and gives hosts enough debug metadata to implement stepping, breakpoints, symbol tracking, and stop hooks.
The core `wqpl` crate owns this debug model and APIs such as pause state, breakpoint state, and source-location metadata.
Concrete command names, aliases, colored terminal help, and command-line flags such as `-w`, `-o`, and `--wqdb-script` belong to the host application.
In the standard CLI, `@p` pauses in wqdb when debugging is enabled, `-w` enables wqdb, and repeated `-o <cmd>` values run once at the first debugger stop.",
    examples: WQDB_EXAMPLES,
    related: &["@p", "@d", "assignment-forms"],
};
