use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const BFN_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Check whether a builtin is available",
        code: "bfn[]|has?[\"echo\"]",
        expectation: ExampleExpectation::ResultContains("T"),
    },
    DocExample {
        title: "Count enabled builtins",
        code: "len bfn[]>0",
        expectation: ExampleExpectation::ResultContains("T"),
    },
];

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

const INT_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Parse text in a base",
        code: "int[\"ff\";16]",
        expectation: ExampleExpectation::ResultContains("255"),
    },
    DocExample {
        title: "Convert bools to 0 and 1",
        code: "(int F;int T)",
        expectation: ExampleExpectation::ResultContains("(0;1)"),
    },
];

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

const ASSERT_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Check a condition",
        code: "assert[2<3]",
        expectation: ExampleExpectation::ResultContains("T"),
    },
    DocExample {
        title: "Describe a failed condition",
        code: "assert[F;\"configuration is not ready\";`context:`startup]",
        expectation: ExampleExpectation::ErrorContains("configuration is not ready"),
    },
];

const ASSERT_EQ_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Check equal values",
        code: "assert_eq[(1;2);(1;2)]",
        expectation: ExampleExpectation::ResultContains("(1;2)"),
    },
    DocExample {
        title: "Compare unequal values",
        code: "assert_eq[3;4;\"unexpected result\"]",
        expectation: ExampleExpectation::ErrorContains("unexpected result"),
    },
];

const RAISE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Raise a runtime error",
    code: "raise \"stop here\"",
    expectation: ExampleExpectation::ErrorContains("stop here"),
}];

const ARGV_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Read forwarded script arguments",
    code: "argv[]",
    expectation: ExampleExpectation::NoRun("depends on the host invocation"),
}];

const ARGPARSE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Parse a flag and positional argument",
    code: r#"spec:(`name:"demo";`args:((`name:`quiet;`kind:`flag;`short:@u"q");(`name:`file;`kind:`positional;`required:T)));argparse[spec;("-q";"input.wq")][`kind]"#,
    expectation: ExampleExpectation::ResultContains("`ok"),
}];

const CLIARGS_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Parse the current script invocation",
    code: r#"cliargs (`name:"demo";`args:,(`name:`file;`kind:`positional;`required:T))"#,
    expectation: ExampleExpectation::NoRun("depends on the host invocation and may halt"),
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

#[cfg(not(target_arch = "wasm32"))]
const EXEC_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Run a host command",
        code: "exec[\"printf\";\"hi\"]",
        expectation: ExampleExpectation::NoRun("spawns a host process"),
    },
    DocExample {
        title: "Send text to stdin",
        code: "exec[\"cat\";`stdin:\"hello\"]",
        expectation: ExampleExpectation::NoRun("spawns a host process"),
    },
    DocExample {
        title: "Run from a working directory",
        code: "exec[\"pwd\";`cwd:\"/tmp\"]",
        expectation: ExampleExpectation::NoRun("depends on the local filesystem"),
    },
    DocExample {
        title: "Set environment and timeout",
        code: "exec[\"sh\";\"-c\";\"printf %s $WQ_MODE\";`env:(`WQ_MODE:\"demo\");`timeout:5]",
        expectation: ExampleExpectation::NoRun("spawns a host process"),
    },
    DocExample {
        title: "Inspect a non-zero exit status",
        code: "exec[\"sh\";\"-c\";\"exit 7\";`check:false]",
        expectation: ExampleExpectation::NoRun("spawns a host process"),
    },
];

const LEN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Count list items",
    code: "len (10;20;30)",
    expectation: ExampleExpectation::ResultContains("3"),
}];

pub(super) const BFN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Bfn,
    summary: "Return the names of enabled builtins.",
    details: "`bfn[]` returns a sorted list of builtin names available in the current builtin preset. It returns strings, so code can search the list with `has?`, `in?`, `find`, or ordinary indexing. The result reflects the active preset selected by the host, such as the CLI `--builtins` flag or the REPL `\\bfn <preset>` command. Use the `builtins` guide for preset and REPL command details.",
    examples: BFN_EXAMPLES,
    related: &["builtins", r"\bfn", "help", "symbols"],
};

pub(super) const CHR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Chr,
    summary: "Convert integer code points to characters.",
    details: "`chr` accepts an int, bigint, or lists of them. Lists of integer code points are packed into strings, and invalid Unicode code points raise a domain error.",
    examples: CHR_EXAMPLES,
    related: &["ord", "str"],
};

pub(super) const ORD: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Ord,
    summary: "Convert characters or strings to Unicode code points.",
    details: "`ord` is the inverse of `chr` for valid Unicode code points. A char returns one int; a string returns a list of code points.",
    examples: ORD_EXAMPLES,
    related: &["chr", "graphemes"],
};

pub(super) const INT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Int,
    summary: "Convert a value to an integer.",
    details: "`int` leaves integer values unchanged, converts `F` to `0` and `T` to `1`, and parses text input. When a base is supplied, it must be in `2..=36`; matching `0b`, `0o`, and `0x` prefixes are accepted, and underscores in digits are ignored.",
    examples: INT_EXAMPLES,
    related: &["bool", "float", "bin", "oct", "hex"],
};

pub(super) const FLOAT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Float,
    summary: "Convert a value to a float.",
    details: "`float` converts numeric values directly and parses text input with Rust-style floating-point syntax. Empty text converts to unit.",
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

pub(super) const ASSERT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Assert,
    summary: "Require a boolean condition to be true.",
    details: "`assert[condition]` returns `T` when its boolean condition is true and raises an assert error otherwise. The optional message replaces the default failure message. The optional named `context` value is preserved in the error's structured `data` dict. Assertion data includes `check:`truth and the failed `condition`, so `@t` callers can inspect it without parsing display text.",
    examples: ASSERT_EXAMPLES,
    related: &["assert_eq", "@t", "raise"],
};

pub(super) const ASSERT_EQ: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::AssertEq,
    summary: "Require two whole values to be equal.",
    details: "`assert_eq[actual;expected]` uses whole-value `=` semantics and returns `actual` when the values are equal. On failure it raises an assert error whose structured `data` dict includes `check:`equal, `actual`, `expected`, and the optional named `context` value. The optional positional message replaces the default failure message.",
    examples: ASSERT_EQ_EXAMPLES,
    related: &["assert", "=", "@t"],
};

pub(super) const RAISE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Raise,
    summary: "Raise a runtime error.",
    details: "`raise` converts its message to text and stops evaluation with a raise error. It is commonly used for explicit validation failures inside functions.",
    examples: RAISE_EXAMPLES,
    related: &["@t", "@r"],
};

pub(super) const ARGV: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Argv,
    summary: "Return the arguments forwarded to the current wq invocation.",
    details: "`argv[]` returns a list of strings containing only the arguments forwarded by the host. The native CLI requires an explicit separator, as in `wq script.wq -- one --flag`. It excludes the `wq` executable and script path. Loaded scripts share the same arguments. New embedded and interactive sessions default to an empty list, and embedders can set arguments through the session API.",
    examples: ARGV_EXAMPLES,
    related: &["argparse", "cliargs"],
};

pub(super) const ARGPARSE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Argparse,
    summary: "Parse a list of command-line argument strings from a declarative spec.",
    details:
"`argparse[spec;args]` parses explicit arguments without printing or halting.

The spec is a dict with:

- `name`: required program-name string.
- `version`: optional version string.
- `about`: optional description string.
- `args`: list of argument descriptor dicts.

Every descriptor requires a tag `name` and a tag `kind`. Supported kinds are `flag`, `count`, `option`, and `positional`. Option and positional descriptors may use a one-argument `parse` callable, `value_name`, `required`, `multiple`, and `choices`. All descriptors may use `default`, `help`, `hidden`, `conflicts`, and `requires`. Non-positional descriptors may use `short` and `long`; flags may also use `negatable`.

If `long` is omitted for a non-positional argument, the argument name is used with underscores changed to hyphens. `flag` defaults to `F`, `count` to `0`, multiple arguments to an empty list, and other absent arguments to unit.

The result is a dict with `kind` and `status`. Successful results have kind `ok`, status `0`, and a `value` dict containing `args` and `command`. Help and version requests have status `0` and plain `text`. User input failures have kind `error`, status `2`, plain `text`, and a structured `error` dict. Invalid specifications raise a domain error because they are program defects.",
    examples: ARGPARSE_EXAMPLES,
    related: &["argv", "cliargs", "dicts", "tag"],
};

pub(super) const CLIARGS: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Cliargs,
    summary: "Parse the current invocation and handle conventional CLI exits.",
    details: "`cliargs[spec]` parses `argv[]` with the same specification accepted by `argparse`. A successful parse returns the value envelope containing `args` and `command`. Help and version requests print to stdout and halt evaluation with status `0`. User input errors print to stderr and halt with status `2`. These controlled halts are not runtime errors and cannot be caught by `@t`. Invalid specifications still raise ordinary domain errors.",
    examples: CLIARGS_EXAMPLES,
    related: &["argv", "argparse", "@t"],
};

pub(super) const ECHO: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Echo,
    summary: "Print values to stdout and return unit.",
    details: "Use `echo` for line-oriented output. Strings are printed as text, other values use their display form, and the optional `sep` named argument joins multiple values on one line.",
    examples: ECHO_EXAMPLES,
    related: &["print", "str", "pipes"],
};

pub(super) const PRINT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Print,
    summary: "Print values to stdout without adding newlines.",
    details: "`print` is the no-newline companion to `echo`. It prints strings as text and otherwise prints each value's display form.",
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

#[cfg(not(target_arch = "wasm32"))]
pub(super) const EXEC: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Exec,
    summary: "Run a host process and capture its output.",
    details:
"`exec` converts its positional arguments to command parts and runs them without a shell.

Return shape:

- Without named options, it returns stdout as a list of lines.
- With any named option, it returns a dict containing `stdout`, `stderr`, `code`, and `success`.
  - `stdout` and `stderr` are lists of lines.
  - `code` is the process exit code.
  - `success` is true when the process exited successfully.

Named options:

- `stdin`: string written to the child process's standard input.
- `cwd`: string path used as the child process's working directory; it must exist and be a directory.
- `env`: dict of environment variables to add or override, with tag keys and string values.
- `timeout`: non-negative integer number of seconds; when it expires, `exec` kills the child and raises an exec error.
- `check`: bool that defaults to true; with `check:true`, a non-zero exit raises an exec error, while `check:false` returns the structured result so code can inspect `code`, `success`, and captured output.

When checking is enabled, failures include the exit code plus stderr and stdout excerpts when available.",
    examples: EXEC_EXAMPLES,
    related: &["input", "open", "freadt"],
};

pub(super) const LEN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Len,
    summary: "Return the length of a value.",
    details: "For containers, `len` returns the number of top-level items; for strings, this is the number of characters. Atoms have length 1 and unit has length 0.",
    examples: LEN_EXAMPLES,
    related: &["shape", "#"],
};
