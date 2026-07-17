use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const STR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Convert a value to a string",
    code: "str (1;2)",
    expectation: ExampleExpectation::ResultContains("(1;2)"),
}];

const GRAPHEMES_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Count grapheme clusters",
    code: "graphemes \"café\"",
    expectation: ExampleExpectation::ResultContains("4"),
}];

const WS_Q_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Test for whitespace",
    code: "ws? @u\" \"",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const WORDS_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Split on Unicode word boundaries",
    code: "words \"red, green\"",
    expectation: ExampleExpectation::ResultContains("(\"red\";\",\";\"green\")"),
}];

const TRIM_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Trim both ends",
    code: "trim \"  hi  \"",
    expectation: ExampleExpectation::ResultContains("\"hi\""),
}];

const L_TRIM_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Trim the left end",
    code: "ltrim \"  hi  \"",
    expectation: ExampleExpectation::ResultContains("\"hi  \""),
}];

const R_TRIM_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Trim the right end",
    code: "rtrim \"  hi  \"",
    expectation: ExampleExpectation::ResultContains("\"  hi\""),
}];

const FMT_DETAILS: &str = "`fmt[template;values...]` and `@f\"...\"` share one formatting system.
In a `fmt` template, `{}` consumes the next value and writes its normal display form.
`{[spec]}` consumes the next value and applies a spec.
Spec contents are `[fill][align][sign][#][0][width][.precision][type]`.

- `fill` is one char used with `align`; `align` is `<`, `>`, `^`, or `=`. `=` pads after a sign or int base prefix.
- `sign` is `+`, `-`, or a space. `#` adds `0x`, `0X`, `0b`, `0B`, `0o`, or `0O` for int bases, and selects pretty debug with `?`.
- `0` is shorthand for sign-aware zero padding when no explicit alignment was set.
- `width` and `.precision` are digits. In `fmt`, dynamic `{}` width or precision consumes an extra value before the formatted value; in `@f`, dynamic `{expr}` width or precision evaluates that expression.
- `type` is `b`, `B`, `o`, `O`, `x`, `X`, `e`, `E`, `,`, `%`, or `?`. Use `,` for thousands separators, `%` for percentages, and `?` for debug output.

Use `{{` and `}}` for literal braces.";

const FMT_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Format values with a specifier",
        code: "fmt[\"{}={[#x]}\";\"n\";255]",
        expectation: ExampleExpectation::ResultContains("\"n=0xff\""),
    },
    DocExample {
        title: "Combine supported spec pieces",
        code: "fmt[\"hex={[#08x]} pct={[.1%]} dbg={[?]}\";123;0.125;T]",
        expectation: ExampleExpectation::ResultContains(
            "\"hex=0x00007b pct=12.5% dbg=Bool(true)\"",
        ),
    },
    DocExample {
        title: "Use dynamic width and precision",
        code: "fmt[\"w={[{}]} p={[.{}]}\";4;12;2;3.14159]",
        expectation: ExampleExpectation::ResultContains("\"w=  12 p=3.14\""),
    },
    DocExample {
        title: "Escape literal braces",
        code: "fmt[\"{{{}}}\";1+2]",
        expectation: ExampleExpectation::ResultContains("\"{3}\""),
    },
];

pub(super) const STR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Str,
    summary: "Convert a value to a string.",
    details: "`str` leaves strings unchanged. Other values are converted through their display representation, making it useful when a rendered value is needed as data rather than terminal output.",
    examples: STR_EXAMPLES,
    related: &["fmt", "@f", "echo"],
};

pub(super) const GRAPHEMES: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Graphemes,
    summary: "Count grapheme clusters in a string.",
    details: "`graphemes` converts its argument to a string and counts Unicode grapheme clusters rather than bytes or Unicode scalars.",
    examples: GRAPHEMES_EXAMPLES,
    related: &["words", "len"],
};

pub(super) const WS_Q: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::WsQ,
    summary: "Return true when a character is whitespace.",
    details: "`ws?` accepts a char and uses Unicode whitespace classification. Create chars with `@u\"...\"`; ordinary quoted literals are strings at every length.",
    examples: WS_Q_EXAMPLES,
    related: &["trim", "words"],
};

pub(super) const WORDS: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Words,
    summary: "Split a string on Unicode word boundaries.",
    details: "`words` uses Unicode word-boundary segmentation and filters out empty or whitespace-only spans. It is not just whitespace splitting: punctuation such as `,` may be returned as its own token.",
    examples: WORDS_EXAMPLES,
    related: &["splitw", "trim", "graphemes"],
};

pub(super) const TRIM: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Trim,
    summary: "Trim whitespace from both ends of a string.",
    details: "`trim` converts its argument to a string and removes leading and trailing Unicode whitespace.",
    examples: TRIM_EXAMPLES,
    related: &["ltrim", "rtrim", "ws?"],
};

pub(super) const L_TRIM: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::LTrim,
    summary: "Trim whitespace from the start of a string.",
    details: "`ltrim` converts its argument to a string and removes leading Unicode whitespace, leaving trailing whitespace unchanged.",
    examples: L_TRIM_EXAMPLES,
    related: &["trim", "rtrim", "ws?"],
};

pub(super) const R_TRIM: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::RTrim,
    summary: "Trim whitespace from the end of a string.",
    details: "`rtrim` converts its argument to a string and removes trailing Unicode whitespace, leaving leading whitespace unchanged.",
    examples: R_TRIM_EXAMPLES,
    related: &["trim", "ltrim", "ws?"],
};

pub(super) const FMT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Fmt,
    summary: "Build a string from a template and values.",
    details: FMT_DETAILS,
    examples: FMT_EXAMPLES,
    related: &["@f", "str"],
};
