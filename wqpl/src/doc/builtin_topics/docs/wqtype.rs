use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const TYPE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Inspect a value category",
    code: "type (1;2)",
    expectation: ExampleExpectation::ResultContains("\"list\""),
}];

const TAG_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Convert text to a tag",
    code: "tag \"ready?\"",
    expectation: ExampleExpectation::ResultContains("`ready?"),
}];

const BOOL_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Convert 0 and 1 to bools",
    code: "(bool 1;bool 0)",
    expectation: ExampleExpectation::ResultContains("(T;F)"),
}];

const CHAR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Convert one character of text",
    code: "char \"x\"",
    expectation: ExampleExpectation::ResultContains("\"x\""),
}];

const ATOM_Q_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Distinguish atoms from containers",
    code: "(atom? 7;atom? (7;8))",
    expectation: ExampleExpectation::ResultContains("(T;F)"),
}];

const UNIT_Q_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Test for unit",
    code: "(unit? ();unit? (1;2))",
    expectation: ExampleExpectation::ResultContains("(T;F)"),
}];

const LIST_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Wrap an atom in a list",
    code: "list 7",
    expectation: ExampleExpectation::ResultContains(",7"),
}];

const DICT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Build a dict from pairs",
    code: "type dict ((`a;1);(`b;2))",
    expectation: ExampleExpectation::ResultContains("\"dict\""),
}];

pub(super) const TYPE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Type,
    summary: "Return the runtime type name for a value.",
    details: "`type[x]` returns a string naming the broad value category used by builtin dispatch. Big integers report as `\"int\"`, and strings and lists of ints report as `\"list\"`, because those values participate in list-like behavior. Use predicates such as `atom?` and `unit?` when the question is structural rather than nominal.",
    examples: TYPE_EXAMPLES,
    related: &["atom?", "unit?", "shape"],
};

pub(super) const TAG: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Tag,
    summary: "Convert string-like data to a tag.",
    details: "`tag[x]` leaves tags unchanged and converts string-like input to a tag name. Tag names must be non-empty and contain only alphanumeric characters, `_`, or `?`; invalid names raise a domain error.",
    examples: TAG_EXAMPLES,
    related: &["type", "dict", "str"],
};

pub(super) const BOOL: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Bool,
    summary: "Convert exactly 0 or 1 to a bool.",
    details: "`bool[x]` accepts bools unchanged, and converts integer or bigint `0` to `F` and `1` to `T`. Other values are rejected; this is not a general truthiness conversion.",
    examples: BOOL_EXAMPLES,
    related: &["int", "atom?"],
};

pub(super) const CHAR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Char,
    summary: "Convert a value to one character.",
    details: "`char[x]` leaves chars unchanged. String-like input must contain exactly one Unicode code point. Other values are first displayed as text, and that text must also be exactly one character.",
    examples: CHAR_EXAMPLES,
    related: &["chr", "ord", "str"],
};

pub(super) const ATOM_Q: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::AtomQ,
    summary: "Return true when a value is not a traversable container.",
    details: "`atom?[x]` is false for lists, strings, and dicts. It is true for atoms such as numbers, bools, chars, tags, fractions, complex values, CAS values, functions, and streams. Unit is empty list-like data, so it is not an atom.",
    examples: ATOM_Q_EXAMPLES,
    related: &["unit?", "type", "shape"],
};

pub(super) const UNIT_Q: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::UnitQ,
    summary: "Return true when a value is unit.",
    details: "`unit?[x]` is true for empty container values. It is false for non-empty containers or strings and for atoms. `U` is an alias.",
    examples: UNIT_Q_EXAMPLES,
    related: &["U", "atom?", "len"],
};

pub(super) const LIST: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::List,
    summary: "Convert a value to a list.",
    details: "`list[x]` leaves lists unchanged. Dicts become a list of two-item `(key;value)` pairs with tag keys. Every other value, including strings, is wrapped as a one-item list.",
    examples: LIST_EXAMPLES,
    related: &["dict", ",", "flatten"],
};

pub(super) const DICT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Dict,
    summary: "Convert a list of pairs to a dict.",
    details: "`dict[x]` expects a list whose items are two-item pairs. Pair keys may be tags, strings, or chars; a two-int pair such as `(1;2)` becomes key `\"1\"` with value `2`. Later duplicate keys replace earlier values.",
    examples: DICT_EXAMPLES,
    related: &["list", "tag", "keys"],
};
