use super::super::model::{DocExample, DocKind, ExampleExpectation, StaticDoc};

const OPERATOR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Operators are functions too",
    code: "+[1;2;3]",
    expectation: ExampleExpectation::ResultContains("6"),
}];

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
    aliases: &["bfn", "builtin", "builtins"],
    summary: "Built-in functions are values provided by wq.",
    details: "Builtins can be called with bracket syntax, postfix syntax for one argument, or through pipes. Individual builtin pages always render their signature and arity from `builtins.rs` metadata.",
    examples: &[],
    related: &["operators", "calls"],
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

pub(super) const WQDB: StaticDoc = StaticDoc {
    id: "wqdb",
    title: "wqdb",
    kind: DocKind::Guide,
    group: "Debugging",
    aliases: &["debugger", "debugging"],
    summary: "wqdb is the source-level debugger used by wq hosts.",
    details: "wqdb pauses execution at source locations, records backtraces and locals, and gives hosts enough debug metadata to implement stepping, breakpoints, symbol tracking, and stop hooks. The core `wqpl` crate owns this debug model and APIs such as pause state, breakpoint state, and source-location metadata. Concrete command names, aliases, colored terminal help, and command-line flags such as `-w`, `-o`, and `--wqdb-script` belong to the host application. In the standard CLI, `@p` pauses in wqdb when debugging is enabled, `-w` enables wqdb, and repeated `-o <cmd>` values run once at the first debugger stop.",
    examples: WQDB_EXAMPLES,
    related: &["@p", "@d", "assignment-forms"],
};
