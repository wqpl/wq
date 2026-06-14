use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const STR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Convert a value to text",
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
    code: "ws? \" \"",
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

const FMT_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Format values with a specifier",
        code: "fmt[\"{}={!#x}\";\"n\";255]",
        expectation: ExampleExpectation::ResultContains("\"n=0xff\""),
    },
    DocExample {
        title: "Combine supported spec pieces",
        code: "fmt[\"hex={!#08x} pct={!.1%} dbg={!?}\";123;0.125;T]",
        expectation: ExampleExpectation::ResultContains(
            "\"hex=0x00007b pct=12.5% dbg=Bool(true)\"",
        ),
    },
];

pub(super) const STR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Str,
    summary: "Convert a value to a string.",
    details: "`str` leaves string-like values unchanged. Other values are converted through their display representation, making it useful when rendered text is needed as data rather than terminal output.",
    examples: STR_EXAMPLES,
    related: &["fmt", "@f", "echo"],
};

pub(super) const GRAPHEMES: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Graphemes,
    summary: "Count user-perceived characters in text.",
    details: "`graphemes` converts its argument to text and counts Unicode grapheme clusters rather than bytes or scalar values.",
    examples: GRAPHEMES_EXAMPLES,
    related: &["words", "len"],
};

pub(super) const WS_Q: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::WsQ,
    summary: "Return true when a character is whitespace.",
    details: "`ws?` accepts a char and uses Unicode whitespace classification. A one-character string literal is accepted by the parser as a char.",
    examples: WS_Q_EXAMPLES,
    related: &["trim", "words"],
};

pub(super) const WORDS: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Words,
    summary: "Split text on Unicode word boundaries.",
    details: "`words` uses Unicode word-boundary segmentation and filters out empty or whitespace-only spans. It is not just whitespace splitting: punctuation such as `,` may be returned as its own token.",
    examples: WORDS_EXAMPLES,
    related: &["splitw", "trim", "graphemes"],
};

pub(super) const TRIM: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Trim,
    summary: "Trim whitespace from both ends of text.",
    details: "`trim` converts its argument to text and removes leading and trailing Unicode whitespace.",
    examples: TRIM_EXAMPLES,
    related: &["ltrim", "rtrim", "ws?"],
};

pub(super) const L_TRIM: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::LTrim,
    summary: "Trim whitespace from the start of text.",
    details: "`ltrim` converts its argument to text and removes leading Unicode whitespace, leaving trailing whitespace unchanged.",
    examples: L_TRIM_EXAMPLES,
    related: &["trim", "rtrim", "ws?"],
};

pub(super) const R_TRIM: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::RTrim,
    summary: "Trim whitespace from the end of text.",
    details: "`rtrim` converts its argument to text and removes trailing Unicode whitespace, leaving leading whitespace unchanged.",
    examples: R_TRIM_EXAMPLES,
    related: &["trim", "ltrim", "ws?"],
};

pub(super) const FMT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Fmt,
    summary: "Build a string from a template and values.",
    details: "`fmt` replaces `{}` with the next value and `{!...}` with the next value formatted by a specifier. Supported spec shape is `{![fill][align][sign][#][0][width][.precision][type]}`: align is `<`, `>`, `^`, or `=`; sign is `+`, `-`, or space; width and precision are digits or dynamic `{}` values; type is `b`, `B`, `o`, `O`, `x`, `X`, `e`, `E`, `,`, `%`, or `?`. `#` adds integer prefixes or pretty debug output, `0` enables sign-aware zero padding, and doubled braces emit literal braces.",
    examples: FMT_EXAMPLES,
    related: &["@f", "str"],
};
