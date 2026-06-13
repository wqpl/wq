use crate::builtins::BuiltinEnum;

use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};

const BFN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Check whether a builtin is available",
    code: "bfn[]|has?[\"echo\"]",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const CHR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Convert code points to text",
    code: "chr (65;66;67)",
    expectation: ExampleExpectation::ResultContains("\"ABC\""),
}];

const ORD_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Convert text to code points",
    code: "ord \"ABC\"",
    expectation: ExampleExpectation::ResultContains("(65;66;67)"),
}];

const INT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Parse text in a base",
    code: "int[\"ff\";16]",
    expectation: ExampleExpectation::ResultContains("255"),
}];

const FLOAT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Parse a decimal string",
    code: "float \"3.25\"",
    expectation: ExampleExpectation::ResultContains("3.25"),
}];

const BIN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Format an int in base 2",
    code: "bin[10;false]",
    expectation: ExampleExpectation::ResultContains("\"1010\""),
}];

const OCT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Format an int in base 8",
    code: "oct 64",
    expectation: ExampleExpectation::ResultContains("\"0o100\""),
}];

const HEX_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Format an int in base 16",
    code: "hex[255;false]",
    expectation: ExampleExpectation::ResultContains("\"ff\""),
}];

const HASH_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Compare hashes for equal values",
    code: "hash 42=hash 42",
    expectation: ExampleExpectation::ResultContains("T"),
}];

const RAISE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Raise a runtime error",
    code: "raise \"stop here\"",
    expectation: ExampleExpectation::ErrorContains("stop here"),
}];

const ECHO_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Print values with a separator",
    code: "echo[\"red\";\"blue\";`sep:\", \"]",
    expectation: ExampleExpectation::NoRun("writes to stdout"),
}];

const PRINT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Write without a trailing newline",
    code: "print \"hi\"",
    expectation: ExampleExpectation::NoRun("writes to stdout"),
}];

const INPUT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Prompt for one line of input",
    code: "input \"name> \"",
    expectation: ExampleExpectation::NoRun("waits for stdin"),
}];

const EXEC_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Run a host command",
    code: "exec[\"printf\";\"hi\"]",
    expectation: ExampleExpectation::NoRun("spawns a host process"),
}];

const LEN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Count list items",
    code: "len (10;20;30)",
    expectation: ExampleExpectation::ResultContains("3"),
}];

pub(super) const BFN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Bfn,
    summary: "Return the names of enabled builtins.",
    details: "`bfn[]` returns a sorted list of builtin names available in the current builtin preset. It is useful when code needs to inspect the runtime surface it is running with.",
    examples: BFN_EXAMPLES,
    related: &["help", "symbols"],
};

pub(super) const CHR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Chr,
    summary: "Convert integer code points to characters.",
    details: "`chr` accepts an int, bigint, or lists of them. Lists of integer code points are packed into strings, and invalid Unicode scalar values raise a domain error.",
    examples: CHR_EXAMPLES,
    related: &["ord", "str"],
};

pub(super) const ORD: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Ord,
    summary: "Convert characters or strings to Unicode code points.",
    details: "`ord` is the inverse of `chr` for valid Unicode scalar values. A char returns one int; a string returns an int list of code points.",
    examples: ORD_EXAMPLES,
    related: &["chr", "graphemes"],
};

pub(super) const INT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Int,
    summary: "Convert a value to an integer.",
    details: "`int` leaves integer values unchanged and parses string-like input. When a base is supplied, it must be in `2..=36`; matching `0b`, `0o`, and `0x` prefixes are accepted, and underscores in digits are ignored.",
    examples: INT_EXAMPLES,
    related: &["float", "bin", "oct", "hex"],
};

pub(super) const FLOAT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Float,
    summary: "Convert a value to a float.",
    details: "`float` converts numeric values directly and parses string-like input with Rust-style floating-point syntax. Empty text converts to unit.",
    examples: FLOAT_EXAMPLES,
    related: &["int", "fraction"],
};

pub(super) const BIN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Bin,
    summary: "Format integers in binary.",
    details: "`bin` returns a string representation of an int or bigint. The optional boolean argument controls whether the `0b` prefix is included.",
    examples: BIN_EXAMPLES,
    related: &["int", "oct", "hex"],
};

pub(super) const OCT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Oct,
    summary: "Format integers in octal.",
    details: "`oct` returns a string representation of an int or bigint. The optional boolean argument controls whether the `0o` prefix is included.",
    examples: OCT_EXAMPLES,
    related: &["int", "bin", "hex"],
};

pub(super) const HEX: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Hex,
    summary: "Format integers in hexadecimal.",
    details: "`hex` returns a lowercase string representation of an int or bigint. The optional boolean argument controls whether the `0x` prefix is included.",
    examples: HEX_EXAMPLES,
    related: &["int", "bin", "oct"],
};

pub(super) const HASH: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Hash,
    summary: "Return a hash value for a wq value.",
    details: "`hash` follows wq value equality, so equal values hash the same within the current implementation. Treat it as a runtime hash, not as a stable external digest format.",
    examples: HASH_EXAMPLES,
    related: &["=", "type"],
};

pub(super) const RAISE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Raise,
    summary: "Raise a runtime error.",
    details: "`raise` converts its message to text and stops evaluation with a raise error. It is commonly used for explicit validation failures inside functions.",
    examples: RAISE_EXAMPLES,
    related: &["@t", "@r"],
};

pub(super) const ECHO: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Echo,
    summary: "Print values to stdout and return unit.",
    details: "Use `echo` for line-oriented output. String-like values are printed as text, other values use their display form, and the optional `sep` named argument joins multiple values on one line.",
    examples: ECHO_EXAMPLES,
    related: &["print", "str", "pipes"],
};

pub(super) const PRINT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Print,
    summary: "Print values to stdout without adding newlines.",
    details: "`print` is the no-newline companion to `echo`. It flattens string-like values to text and otherwise prints each value's display form.",
    examples: PRINT_EXAMPLES,
    related: &["echo", "str"],
};

pub(super) const INPUT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Input,
    summary: "Read one line from stdin.",
    details: "`input` optionally prints a prompt, reads one line, and returns it as a string. End-of-file and interruption return unit.",
    examples: INPUT_EXAMPLES,
    related: &["echo", "print"],
};

pub(super) const EXEC: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Exec,
    summary: "Run a host process and capture its output.",
    details: "`exec` converts its positional arguments to command parts. Without named options it returns stdout as a list of lines; with options such as `stdin`, `cwd`, `env`, `timeout`, or `check`, it returns a dict containing `stdout`, `stderr`, `code`, and `success`.",
    examples: EXEC_EXAMPLES,
    related: &["input", "open", "freadt"],
};

pub(super) const LEN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Len,
    summary: "Return the length of a value.",
    details: "For lists and strings, `len` returns the number of top-level items. Atoms have length 1 and unit has length 0.",
    examples: LEN_EXAMPLES,
    related: &["shape", "#"],
};
