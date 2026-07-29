use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const STR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Convert a value to a string",
    code: "str (1;2)",
    expectation: ExampleExpectation::ResultContains("(1;2)"),
}];

const GRAPHEMES_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Split extended grapheme clusters",
    code: "graphemes \"éx\"",
    expectation: ExampleExpectation::ResultContains("(\"é\";\"x\")"),
}];

const UNICODE_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Look up a character name",
        code: "unicode[\"☃\";`name]",
        expectation: ExampleExpectation::ResultContains("\"SNOWMAN\""),
    },
    DocExample {
        title: "Look up a named sequence",
        code: "unicode[\"KEYCAP DIGIT ONE\";`from_name]",
        expectation: ExampleExpectation::ResultContains("1"),
    },
    DocExample {
        title: "Test an identifier property",
        code: "unicode[\"λ\";`xid_start]",
        expectation: ExampleExpectation::ResultContains("T"),
    },
];

const NORMALIZE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Compose a decomposed string",
    code: "normalize \"é\"",
    expectation: ExampleExpectation::ResultContains("é"),
}];

const CASE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Apply a full uppercase mapping",
    code: "case[\"ß\";`upper]",
    expectation: ExampleExpectation::ResultContains("\"SS\""),
}];

const WHITESPACE_Q_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Test for whitespace",
    code: "whitespace? \" \"",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const TERM_WIDTH_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Measure terminal columns",
    code: "termwidth \"猫\"",
    expectation: ExampleExpectation::ResultContains("2"),
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
    summary: "Split a char or string into extended grapheme clusters.",
    details: "`graphemes` returns Unicode extended grapheme clusters in order. A one-scalar cluster is a char. A multi-scalar cluster is a string.",
    examples: GRAPHEMES_EXAMPLES,
    related: &["unicode", "len"],
};

pub(super) const UNICODE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Unicode,
    summary: "Query Unicode version, names, named sequences, and identifier properties.",
    details: "`unicode[]` returns the Unicode version used by the Unicode builtins. `` unicode[c;`name] `` returns the primary Unicode character name. For an approved Unicode named sequence, `` unicode[s;`name] `` returns its sequence name. `` unicode[name;`from_name] `` performs loose reverse lookup across primary names, formal aliases, and approved named sequences. `` unicode[c;`xid_start] `` and `` unicode[c;`xid_continue] `` expose the raw Unicode identifier properties. A lookup with no result returns `()`.",
    examples: UNICODE_EXAMPLES,
    related: &["normalize", "case", "graphemes"],
};

pub(super) const NORMALIZE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Normalize,
    summary: "Normalize a char or string.",
    details: "`normalize[x]` applies NFC. Pass `` `nfc ``, `` `nfd ``, `` `nfkc ``, or `` `nfkd `` as the second argument to select another Unicode normalization form. A char result stays a char when the normalized result contains one scalar; otherwise it becomes a string. A string input always returns a string.",
    examples: NORMALIZE_EXAMPLES,
    related: &["unicode", "case", "graphemes"],
};

pub(super) const CASE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Case,
    summary: "Apply full Unicode case conversion or folding.",
    details: "`` case[x;`lower] `` and `` case[x;`upper] `` use full, context-aware root-locale mappings. `` case[x;`fold] `` performs locale-independent Unicode case folding. A char result stays a char only when the mapping contains one scalar. A string input always returns a string.",
    examples: CASE_EXAMPLES,
    related: &["unicode", "normalize"],
};

pub(super) const WHITESPACE_Q: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::WhitespaceQ,
    summary: "Return true when a character is whitespace.",
    details: "`whitespace?` accepts a char and tests the Unicode White_Space property. The trim family, whitespace splitting, and source lexer use the same property.",
    examples: WHITESPACE_Q_EXAMPLES,
    related: &["trim", "unicode"],
};

pub(super) const TRIM: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Trim,
    summary: "Trim whitespace from both ends of a string.",
    details: "`trim` converts its argument to a string and removes leading and trailing Unicode whitespace.",
    examples: TRIM_EXAMPLES,
    related: &["ltrim", "rtrim", "whitespace?"],
};

pub(super) const L_TRIM: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::LTrim,
    summary: "Trim whitespace from the start of a string.",
    details: "`ltrim` converts its argument to a string and removes leading Unicode whitespace, leaving trailing whitespace unchanged.",
    examples: L_TRIM_EXAMPLES,
    related: &["trim", "rtrim", "whitespace?"],
};

pub(super) const R_TRIM: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::RTrim,
    summary: "Trim whitespace from the end of a string.",
    details: "`rtrim` converts its argument to a string and removes trailing Unicode whitespace, leaving leading whitespace unchanged.",
    examples: R_TRIM_EXAMPLES,
    related: &["trim", "ltrim", "whitespace?"],
};

pub(super) const TERM_WIDTH: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Termwidth,
    summary: "Measure terminal display width.",
    details: "`termwidth` returns the default Unicode terminal width of a char or string. It has one stable width mode. Control characters are rejected because their width depends on terminal state.",
    examples: TERM_WIDTH_EXAMPLES,
    related: &["graphemes", "len"],
};

pub(super) const FMT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Fmt,
    summary: "Build a string from a template and values.",
    details: FMT_DETAILS,
    examples: FMT_EXAMPLES,
    related: &["@f", "str"],
};
