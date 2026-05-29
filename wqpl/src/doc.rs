use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::builtins::{BUILTIN_GROUPS, BuiltinEnum, BuiltinGroup, Builtins};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    Builtin,
    Keyword,
    Syntax,
    Guide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocRenderTarget {
    Cli,
    Lsp,
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExampleExpectation {
    Runs,
    ResultContains(&'static str),
    ErrorContains(&'static str),
    StdoutContains(&'static str),
    NoRun(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocExample {
    pub title: &'static str,
    pub code: &'static str,
    pub expectation: ExampleExpectation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocTopic {
    pub id: String,
    pub title: String,
    pub kind: DocKind,
    pub group: String,
    pub aliases: Vec<String>,
    pub summary: String,
    pub details: String,
    pub examples: Vec<DocExample>,
    pub related: Vec<String>,
    pub builtin: Option<BuiltinEnum>,
    pub canonical_builtin: Option<BuiltinEnum>,
}

#[derive(Debug, Clone, Copy)]
struct StaticDoc {
    id: &'static str,
    title: &'static str,
    kind: DocKind,
    group: &'static str,
    aliases: &'static [&'static str],
    summary: &'static str,
    details: &'static str,
    examples: &'static [DocExample],
    related: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
struct BuiltinDoc {
    builtin: BuiltinEnum,
    summary: &'static str,
    details: &'static str,
    examples: &'static [DocExample],
    related: &'static [&'static str],
}

const MAP_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Map a function over a list",
    code: "(1;2;3)|map{x*x}",
    expectation: ExampleExpectation::ResultContains("(1;4;9)"),
}];

const WORDS_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Split on whitespace",
    code: "words \"red green blue\"",
    expectation: ExampleExpectation::ResultContains("(\"red\";\"green\";\"blue\")"),
}];

const SPLIT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Split into runs",
    code: "len split[(1;2;3);2]",
    expectation: ExampleExpectation::ResultContains("2"),
}];

const LEN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Count list items",
    code: "len (10;20;30)",
    expectation: ExampleExpectation::ResultContains("3"),
}];

const STR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Convert a value to text",
    code: "str (1;2)",
    expectation: ExampleExpectation::ResultContains("(1;2)"),
}];

const HAS_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Membership with depth sugar",
    code: "(1;2;3)|has?@1[2]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const ECHO_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Print a value",
    code: "echo \"hello\"",
    expectation: ExampleExpectation::NoRun("writes to stdout"),
}];

const BUILTIN_DOCS: &[BuiltinDoc] = &[
    BuiltinDoc {
        builtin: BuiltinEnum::Echo,
        summary: "Print values to stdout and return unit.",
        details: "Use `echo` for display-oriented output. In expression-heavy code, prefer `expr |echo` when the expression would otherwise need parentheses before a postfix call.",
        examples: ECHO_EXAMPLES,
        related: &["print", "str", "pipes"],
    },
    BuiltinDoc {
        builtin: BuiltinEnum::Len,
        summary: "Return the length of a value.",
        details: "For lists and strings, `len` returns the number of top-level items. Atoms have length 1 and unit has length 0.",
        examples: LEN_EXAMPLES,
        related: &["shape", "#"],
    },
    BuiltinDoc {
        builtin: BuiltinEnum::Map,
        summary: "Apply a function to each item of a value.",
        details: "The short alias is `M`. Depth-aware calls can use modifiers such as `@1` on builtins that support depth sugar.",
        examples: MAP_EXAMPLES,
        related: &["M", "filter", "fold", "@depth"],
    },
    BuiltinDoc {
        builtin: BuiltinEnum::Split,
        summary: "Split a value into groups.",
        details: "`split` separates a value using a delimiter or delimiter-like rule. Some modes accept named options such as `maxsplit`.",
        examples: SPLIT_EXAMPLES,
        related: &["splitw", "words"],
    },
    BuiltinDoc {
        builtin: BuiltinEnum::Words,
        summary: "Split text into whitespace-delimited words.",
        details: "`words` is a string-focused convenience for common tokenization. It trims whitespace and omits empty runs.",
        examples: WORDS_EXAMPLES,
        related: &["split", "trim", "graphemes"],
    },
    BuiltinDoc {
        builtin: BuiltinEnum::Str,
        summary: "Convert a value to a string.",
        details: "`str` is useful when a display representation is needed as data rather than as terminal output.",
        examples: STR_EXAMPLES,
        related: &["fmt", "@f", "echo"],
    },
    BuiltinDoc {
        builtin: BuiltinEnum::HasQ,
        summary: "Test whether a container has a value.",
        details: "`has?` returns a bool. It is depth-aware, so postfix depth modifiers can be used when searching nested values.",
        examples: HAS_EXAMPLES,
        related: &["in?", "@depth"],
    },
];

const BUILTIN_ALIASES: &[(BuiltinEnum, BuiltinEnum)] = &[
    (BuiltinEnum::E, BuiltinEnum::Echo),
    (BuiltinEnum::V, BuiltinEnum::Reverse),
    (BuiltinEnum::R, BuiltinEnum::Reshape),
    (BuiltinEnum::TP, BuiltinEnum::Transpose),
    (BuiltinEnum::Z, BuiltinEnum::Where),
    (BuiltinEnum::A, BuiltinEnum::Apply),
    (BuiltinEnum::M, BuiltinEnum::Map),
    (BuiltinEnum::Reduce, BuiltinEnum::Fold),
    (BuiltinEnum::D, BuiltinEnum::Diff),
    (BuiltinEnum::I, BuiltinEnum::Integrate),
    (BuiltinEnum::U, BuiltinEnum::UnitQ),
];

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
    title: "Convert an error to false",
    code: "@t 1/0",
    expectation: ExampleExpectation::ResultContains("F"),
}];

const AT_SYMBOLIC_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Create a CAS expression",
    code: "type @s x+1",
    expectation: ExampleExpectation::ResultContains("\"cas\""),
}];

const AT_FSTRING_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Interpolate an expression",
    code: "@f\"{1+2}\"",
    expectation: ExampleExpectation::ResultContains("3"),
}];

const AT_RAW_STRING_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Keep backslashes raw",
    code: "len @l\"\\n\"",
    expectation: ExampleExpectation::ResultContains("2"),
}];

const AT_DEPTH_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Search one level deep",
    code: "(1;2;3)|has?@1[2]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

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

const OPERATOR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Operators are functions too",
    code: "+[1;2;3]",
    expectation: ExampleExpectation::ResultContains("6"),
}];

const SYNTAX_DOCS: &[StaticDoc] = &[
    StaticDoc {
        id: "builtins",
        title: "Builtins",
        kind: DocKind::Guide,
        group: "Reference",
        aliases: &["bfn", "builtin", "builtins"],
        summary: "Built-in functions are values provided by wq.",
        details: "Builtins can be called with bracket syntax, postfix syntax for one argument, or through pipes. Individual builtin pages always render their signature and arity from `builtins.rs` metadata.",
        examples: &[],
        related: &["operators", "calls"],
    },
    StaticDoc {
        id: "operators",
        title: "Operators",
        kind: DocKind::Guide,
        group: "Reference",
        aliases: &["operator", "operators", "+", "-", "*", "/", ","],
        summary: "Operators are also builtin functions.",
        details: "Most binary operators broadcast over compatible values. The comma operator concatenates, while leading comma enlists a value.",
        examples: OPERATOR_EXAMPLES,
        related: &["builtins", "lists", "pipes"],
    },
    StaticDoc {
        id: "at-assert",
        title: "@a Assert",
        kind: DocKind::Keyword,
        group: "Keywords",
        aliases: &["@a", "assert"],
        summary: "Assert that an expression is true.",
        details: "`@a expr` evaluates `expr` and raises if it is false. It is useful for executable examples and invariants.",
        examples: AT_ASSERT_EXAMPLES,
        related: &["@t", "raise"],
    },
    StaticDoc {
        id: "at-break",
        title: "@b Break",
        kind: DocKind::Keyword,
        group: "Keywords",
        aliases: &["@b", "break"],
        summary: "Leave the nearest enclosing loop.",
        details: "`@b` is only valid inside a loop body. It applies to the nearest loop.",
        examples: AT_BREAK_EXAMPLES,
        related: &["@c", "N", "W"],
    },
    StaticDoc {
        id: "at-continue",
        title: "@c Continue",
        kind: DocKind::Keyword,
        group: "Keywords",
        aliases: &["@c", "continue"],
        summary: "Skip to the next loop iteration.",
        details: "`@c` is only valid inside a loop body. It applies to the nearest loop.",
        examples: AT_CONTINUE_EXAMPLES,
        related: &["@b", "N", "W"],
    },
    StaticDoc {
        id: "at-return",
        title: "@r Return",
        kind: DocKind::Keyword,
        group: "Keywords",
        aliases: &["@r", "return"],
        summary: "Return early from the current function.",
        details: "`@r value` exits immediately with `value`. Bare `@r` returns unit.",
        examples: AT_RETURN_EXAMPLES,
        related: &["functions"],
    },
    StaticDoc {
        id: "at-debug",
        title: "@d Debug",
        kind: DocKind::Keyword,
        group: "Keywords",
        aliases: &["@d", "debug"],
        summary: "Evaluate an expression while printing a trace.",
        details: "`@d expr` is a runtime debugging probe. It yields the expression value after showing trace information.",
        examples: AT_DEBUG_EXAMPLES,
        related: &["@p"],
    },
    StaticDoc {
        id: "at-pause",
        title: "@p Pause",
        kind: DocKind::Keyword,
        group: "Keywords",
        aliases: &["@p", "pause"],
        summary: "Pause execution in wqdb.",
        details: "`@p` optionally accepts an expression and then pauses execution when debugging is enabled.",
        examples: AT_PAUSE_EXAMPLES,
        related: &["@d"],
    },
    StaticDoc {
        id: "at-try",
        title: "@t Try",
        kind: DocKind::Keyword,
        group: "Keywords",
        aliases: &["@t", "try"],
        summary: "Turn a failing expression into a false result.",
        details: "`@t expr` catches runtime errors from `expr`, returning the value on success or `F` on failure.",
        examples: AT_TRY_EXAMPLES,
        related: &["raise"],
    },
    StaticDoc {
        id: "at-symbolic",
        title: "@s Symbolic",
        kind: DocKind::Keyword,
        group: "Keywords",
        aliases: &["@s", "symbolic", "cas"],
        summary: "Quote an expression into a symbolic CAS value.",
        details: "Use `@s` once at the start of a CAS expression, then apply CAS builtins directly. Bare arithmetic without `@s` is normal evaluation.",
        examples: AT_SYMBOLIC_EXAMPLES,
        related: &["diff", "integrate", "simplify"],
    },
    StaticDoc {
        id: "at-fstring",
        title: "@f Format String",
        kind: DocKind::Keyword,
        group: "Keywords",
        aliases: &["@f", "format string", "fstring"],
        summary: "Create a string by interpolating expressions.",
        details: "`@f\"...{expr}...\"` evaluates braces as wq expressions. Use doubled braces for literal braces.",
        examples: AT_FSTRING_EXAMPLES,
        related: &["fmt", "str"],
    },
    StaticDoc {
        id: "at-raw-string",
        title: "@l Raw String",
        kind: DocKind::Keyword,
        group: "Keywords",
        aliases: &["@l", "raw string"],
        summary: "Read a string without escape processing.",
        details: "`@l\"...\"` keeps backslashes as ordinary characters.",
        examples: AT_RAW_STRING_EXAMPLES,
        related: &["@f"],
    },
    StaticDoc {
        id: "at-depth",
        title: "@depth Modifier",
        kind: DocKind::Keyword,
        group: "Keywords",
        aliases: &["@depth", "@1", "@2", "depth modifier"],
        summary: "Append a depth argument to depth-aware builtin calls.",
        details: "`@1`, `@2`, and other non-negative depth modifiers are postfix call modifiers. They are valid only on builtins whose metadata declares depth sugar.",
        examples: AT_DEPTH_EXAMPLES,
        related: &["map", "has?", "find"],
    },
    StaticDoc {
        id: "assignment",
        title: "Assignment",
        kind: DocKind::Syntax,
        group: "Syntax",
        aliases: &[":", "assignment", "binding"],
        summary: "Bind a name with `lhs:rhs`.",
        details: "A single equals sign is equality; colon performs assignment.",
        examples: ASSIGNMENT_EXAMPLES,
        related: &["equality"],
    },
    StaticDoc {
        id: "equality",
        title: "Equality",
        kind: DocKind::Syntax,
        group: "Syntax",
        aliases: &["=", "equality", "equal"],
        summary: "Compare values with `=`.",
        details: "`a=b` is equality. Use `a:b` for assignment.",
        examples: EQUALITY_EXAMPLES,
        related: &["assignment"],
    },
    StaticDoc {
        id: "lists",
        title: "Lists",
        kind: DocKind::Syntax,
        group: "Syntax",
        aliases: &["list", "lists", "()"],
        summary: "Create lists with semicolon-separated parentheses.",
        details: "`(1;2;3)` is a list. `(1)` is just the atom `1`; use leading comma to enlist a single value.",
        examples: LIST_EXAMPLES,
        related: &[",", "len"],
    },
    StaticDoc {
        id: "dicts",
        title: "Dicts",
        kind: DocKind::Syntax,
        group: "Syntax",
        aliases: &["dict", "dicts", "dictionary"],
        summary: "Create dictionaries with symbol keys.",
        details: "Dict keys are tags, written with a leading backtick. The empty dict is (`).",
        examples: DICT_EXAMPLES,
        related: &["keys", "tag"],
    },
    StaticDoc {
        id: "calls",
        title: "Calls and Indexing",
        kind: DocKind::Syntax,
        group: "Syntax",
        aliases: &["call", "calls", "index", "indexing", "[]"],
        summary: "Call or index with brackets and semicolons.",
        details: "`target[expr1;expr2]` passes multiple arguments or indexes multiple positions depending on the target value.",
        examples: CALL_EXAMPLES,
        related: &["postfix", "pipes"],
    },
    StaticDoc {
        id: "postfix",
        title: "Postfix Calls",
        kind: DocKind::Syntax,
        group: "Syntax",
        aliases: &["postfix", "postfix call"],
        summary: "A function followed by one expression calls it.",
        details: "`fn arg` is a one-argument call. `fn1 fn2 arg` chains calls. `fn arg1 arg2` is not a two-argument call.",
        examples: POSTFIX_EXAMPLES,
        related: &["calls"],
    },
    StaticDoc {
        id: "pipes",
        title: "Pipes",
        kind: DocKind::Syntax,
        group: "Syntax",
        aliases: &["|", "pipe", "pipes"],
        summary: "Pipe inserts the left value as the first argument to a right-hand call.",
        details: "`x | f[y]` behaves like `f[x;y]`. Pipe syntax is often the clearest way to apply display or transformation builtins to larger expressions.",
        examples: PIPE_EXAMPLES,
        related: &["calls", "postfix"],
    },
    StaticDoc {
        id: "conditionals",
        title: "Conditionals",
        kind: DocKind::Syntax,
        group: "Syntax",
        aliases: &["$", "$.", "$$", "conditional", "conditionals"],
        summary: "Choose between branches with dollar forms.",
        details: "`$[c;t;f]` is a ternary. `$.[c;t]` is a guard-like conditional. `$$[...]` chains condition/action pairs.",
        examples: CONDITIONAL_EXAMPLES,
        related: &["bool"],
    },
    StaticDoc {
        id: "n-loop",
        title: "N Loop",
        kind: DocKind::Syntax,
        group: "Syntax",
        aliases: &["N", "N loop", "n-loop"],
        summary: "Repeat a body a fixed number of times.",
        details: "`N[n;body]` exposes `_n` as the zero-based iteration counter.",
        examples: N_LOOP_EXAMPLES,
        related: &["@b", "@c"],
    },
    StaticDoc {
        id: "w-loop",
        title: "W Loop",
        kind: DocKind::Syntax,
        group: "Syntax",
        aliases: &["W", "W loop", "w-loop"],
        summary: "Repeat while a bool condition remains true.",
        details: "`W[cond;body]` requires `cond` to evaluate to a bool.",
        examples: W_LOOP_EXAMPLES,
        related: &["@b", "@c"],
    },
    StaticDoc {
        id: "block",
        title: "B Block",
        kind: DocKind::Syntax,
        group: "Syntax",
        aliases: &["B", "B block", "block"],
        summary: "Evaluate statements as a single expression.",
        details: "`B[...]` groups multiple statements in expression positions such as condition branches.",
        examples: BLOCK_EXAMPLES,
        related: &["conditionals"],
    },
];

pub fn resolve(query: &str) -> Option<DocTopic> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }

    if let Some(topic) = resolve_builtin(query) {
        return Some(topic);
    }

    if is_depth_query(query) {
        return static_topic("at-depth");
    }

    resolve_static(query)
}

pub fn all_topics() -> Vec<DocTopic> {
    let mut topics: Vec<DocTopic> = SYNTAX_DOCS.iter().map(static_doc_topic).collect();
    topics.extend(Builtins::ENUMS.iter().copied().map(builtin_topic));
    topics
}

pub fn topics_by_group() -> Vec<(String, Vec<DocTopic>)> {
    let mut groups: BTreeMap<String, Vec<DocTopic>> = BTreeMap::new();
    for topic in all_topics() {
        groups.entry(topic.group.clone()).or_default().push(topic);
    }
    groups.into_iter().collect()
}

pub fn render_markdown(topic: &DocTopic, target: DocRenderTarget) -> String {
    let mut out = String::new();
    let heading = match target {
        DocRenderTarget::Cli | DocRenderTarget::Web => "#",
        DocRenderTarget::Lsp => "##",
    };
    let _ = writeln!(out, "{} {}", heading, topic.title);
    let _ = writeln!(out);
    let _ = writeln!(out, "_{} · {}_", kind_label(topic.kind), topic.group);
    let _ = writeln!(out);
    let _ = writeln!(out, "{}", topic.summary);

    if let Some(builtin) = topic.builtin {
        let _ = writeln!(out);
        let _ = writeln!(out, "```wq");
        let _ = writeln!(out, "{}", builtin.usage());
        let _ = writeln!(out, "```");
        let _ = writeln!(out);
        let _ = writeln!(out, "arity: `{}`", builtin.arity());
        if let Some(canonical) = topic.canonical_builtin
            && canonical != builtin
        {
            let _ = writeln!(out);
            let _ = writeln!(out, "Alias of `{}`.", canonical.name());
        }
    }

    if !topic.details.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "{}", topic.details);
    }

    if !topic.examples.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "## Examples");
        for example in &topic.examples {
            let _ = writeln!(out);
            if !example.title.is_empty() {
                let _ = writeln!(out, "{}", example.title);
                let _ = writeln!(out);
            }
            let _ = writeln!(out, "```wq");
            let _ = writeln!(out, "{}", example.code);
            let _ = writeln!(out, "```");
        }
    }

    if !topic.related.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "Related: {}", topic.related.join(", "));
    }

    out.trim_end().to_string()
}

pub fn builtin_topic(builtin: BuiltinEnum) -> DocTopic {
    let canonical = canonical_builtin(builtin);
    let builtin_doc = builtin_doc(canonical);
    let group = builtin_group(builtin)
        .map(BuiltinGroup::name)
        .unwrap_or("Builtin")
        .to_string();
    let alias_summary;
    let summary = if let Some(doc) = builtin_doc {
        if canonical == builtin {
            doc.summary.to_string()
        } else {
            alias_summary = format!("Alias of `{}`. {}", canonical.name(), doc.summary);
            alias_summary
        }
    } else if canonical == builtin {
        format!("Builtin in the {group} group.")
    } else {
        format!("Alias of `{}`.", canonical.name())
    };
    let details = builtin_doc
        .map(|doc| doc.details.to_string())
        .unwrap_or_else(|| "This page is generated from builtin metadata; add a hand-written doc entry when the behavior needs more explanation.".to_string());
    let examples = builtin_doc
        .map(|doc| doc.examples.to_vec())
        .unwrap_or_default();
    let mut related: Vec<String> = builtin_doc
        .map(|doc| doc.related.iter().map(|item| (*item).to_string()).collect())
        .unwrap_or_default();
    if canonical != builtin {
        related.push(canonical.name().to_string());
    }
    DocTopic {
        id: format!("builtin.{}", builtin.name()),
        title: format!("{} builtin", builtin.name()),
        kind: DocKind::Builtin,
        group,
        aliases: vec![builtin.name().to_string()],
        summary,
        details,
        examples,
        related,
        builtin: Some(builtin),
        canonical_builtin: Some(canonical),
    }
}

fn resolve_builtin(query: &str) -> Option<DocTopic> {
    Builtins::new().doc_for_name(query)
}

fn resolve_static(query: &str) -> Option<DocTopic> {
    let query_lower = query.to_ascii_lowercase();
    SYNTAX_DOCS
        .iter()
        .find(|doc| {
            doc.id == query
                || doc.id.eq_ignore_ascii_case(query)
                || doc.title.eq_ignore_ascii_case(query)
                || doc.aliases.iter().any(|alias| {
                    *alias == query || alias.to_ascii_lowercase() == query_lower
                })
        })
        .map(static_doc_topic)
}

fn static_topic(id: &str) -> Option<DocTopic> {
    SYNTAX_DOCS
        .iter()
        .find(|doc| doc.id == id)
        .map(static_doc_topic)
}

fn static_doc_topic(doc: &StaticDoc) -> DocTopic {
    DocTopic {
        id: doc.id.to_string(),
        title: doc.title.to_string(),
        kind: doc.kind,
        group: doc.group.to_string(),
        aliases: doc.aliases.iter().map(|alias| (*alias).to_string()).collect(),
        summary: doc.summary.to_string(),
        details: doc.details.to_string(),
        examples: doc.examples.to_vec(),
        related: doc.related.iter().map(|item| (*item).to_string()).collect(),
        builtin: None,
        canonical_builtin: None,
    }
}

fn builtin_doc(builtin: BuiltinEnum) -> Option<&'static BuiltinDoc> {
    BUILTIN_DOCS.iter().find(|doc| doc.builtin == builtin)
}

fn canonical_builtin(builtin: BuiltinEnum) -> BuiltinEnum {
    BUILTIN_ALIASES
        .iter()
        .find_map(|(alias, canonical)| (*alias == builtin).then_some(*canonical))
        .unwrap_or(builtin)
}

fn builtin_group(builtin: BuiltinEnum) -> Option<BuiltinGroup> {
    BUILTIN_GROUPS.get(builtin.id() as usize).copied()
}

fn kind_label(kind: DocKind) -> &'static str {
    match kind {
        DocKind::Builtin => "builtin",
        DocKind::Keyword => "keyword",
        DocKind::Syntax => "syntax",
        DocKind::Guide => "guide",
    }
}

fn is_depth_query(query: &str) -> bool {
    query
        .strip_prefix('@')
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;

    #[test]
    fn every_builtin_has_a_doc_topic() {
        for builtin in Builtins::ENUMS {
            let topic = builtin_topic(*builtin);
            assert_eq!(topic.builtin, Some(*builtin));
            assert!(
                render_markdown(&topic, DocRenderTarget::Cli).contains(builtin.usage()),
                "rendered doc for {} should use builtin usage metadata",
                builtin.name()
            );
            assert!(
                render_markdown(&topic, DocRenderTarget::Cli).contains(builtin.arity()),
                "rendered doc for {} should use builtin arity metadata",
                builtin.name()
            );
        }
    }

    #[test]
    fn resolves_keywords_and_depth_modifiers() {
        assert_eq!(
            resolve("@r").expect("@r doc").id,
            "at-return".to_string()
        );
        assert_eq!(
            resolve("@12").expect("@12 doc").id,
            "at-depth".to_string()
        );
        assert_eq!(
            resolve("words").expect("words doc").builtin,
            Some(BuiltinEnum::Words)
        );
    }

    #[test]
    fn executable_examples_stay_in_sync() {
        for topic in all_topics() {
            for example in &topic.examples {
                check_example(&topic, example);
            }
        }
    }

    fn check_example(topic: &DocTopic, example: &DocExample) {
        match example.expectation {
            ExampleExpectation::NoRun(_) => {}
            ExampleExpectation::Runs => {
                let mut session = Session::new();
                session
                    .eval_string(example.code)
                    .unwrap_or_else(|err| panic!("{} example failed: {err}", topic.id));
            }
            ExampleExpectation::ResultContains(expected) => {
                let mut session = Session::new();
                let value = session
                    .eval_string(example.code)
                    .unwrap_or_else(|err| panic!("{} example failed: {err}", topic.id));
                let actual = value.to_string();
                assert!(
                    actual.contains(expected),
                    "{} example result mismatch: expected {expected:?} in {actual:?}",
                    topic.id
                );
            }
            ExampleExpectation::ErrorContains(expected) => {
                let mut session = Session::new();
                let err = session
                    .eval_string(example.code)
                    .expect_err("example should fail");
                let actual = err.to_string();
                assert!(
                    actual.contains(expected),
                    "{} example error mismatch: expected {expected:?} in {actual:?}",
                    topic.id
                );
            }
            ExampleExpectation::StdoutContains(_) => {
                panic!("stdout expectations need an explicit capture harness")
            }
        }
    }
}
