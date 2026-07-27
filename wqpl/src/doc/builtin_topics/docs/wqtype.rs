use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const TYPE_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Inspect several public categories",
        code: "(type[42];type[1/.3];type[@u\"x\"];type[\"wq\"])",
        expectation: ExampleExpectation::ResultContains("(\"int\";\"fraction\";\"char\";\"list\")"),
    },
    DocExample {
        title: "Inspect lists",
        code: "(type[til 3];type[\"wq\"])",
        expectation: ExampleExpectation::ResultContains("(\"list\";\"list\")"),
    },
    DocExample {
        title: "Inspect functions",
        code: "(type[{x+1}];type[map])",
        expectation: ExampleExpectation::ResultContains("(\"function\";\"function\")"),
    },
];

const TAG_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Convert a string to a tag",
    code: "tag \"ready?\"",
    expectation: ExampleExpectation::ResultContains("`ready?"),
}];

const BOOL_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Convert 0 and 1 to bools",
    code: "(bool 1;bool 0)",
    expectation: ExampleExpectation::ResultContains("(T;F)"),
}];

const CHAR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Convert a string containing one Unicode scalar",
    code: "char \"x\"",
    expectation: ExampleExpectation::ResultContains("\"x\""),
}];

const ATOM_Q_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Distinguish atoms from containers",
    code: "(atom? 7;atom? (7;8))",
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
    summary: "Return the public value category for a value.",
    details: "`type[x]` returns the stable public category of `x` as a string.
The result is one of `\"int\"`, `\"float\"`, `\"complex\"`, `\"fraction\"`, `\"algebraic\"`, `\"char\"`, `\"tag\"`, `\"bool\"`, `\"list\"`, `\"cas\"`, `\"dict\"`, `\"function\"`, `\"rng\"`, or `\"stream\"`.
Use predicates such as `atom?` and length comparisons such as `#x~0` for structural questions.",
    examples: TYPE_EXAMPLES,
    related: &["atom?", "len", "shape"],
};

pub(super) const TAG: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Tag,
    summary: "Convert a string to a tag.",
    details: "`tag[x]` converts a string to a tag name and leaves tags unchanged.
Tag names follow identifier character rules: they start with a Unicode identifier character or `_`, and remaining characters can also include `?`.
Invalid names raise a domain error.",
    examples: TAG_EXAMPLES,
    related: &["type", "dict", "str"],
};

pub(super) const BOOL: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Bool,
    summary: "Convert exactly 0 or 1 to a bool.",
    details: "`bool[x]` converts int `0` to `F` and `1` to `T`, and accepts bools unchanged. Other values are rejected.",
    examples: BOOL_EXAMPLES,
    related: &["int", "atom?"],
};

pub(super) const CHAR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Char,
    summary: "Convert a value to one Unicode scalar.",
    details: "`char[x]` leaves char atoms unchanged. String input must contain exactly one Unicode scalar. Other values are first displayed as a string, and that string must also contain exactly one Unicode scalar.",
    examples: CHAR_EXAMPLES,
    related: &["chr", "ord", "str"],
};

pub(super) const ATOM_Q: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::AtomQ,
    summary: "Return true when a value is not a traversable container.",
    details: "`atom?[x]` is false for containers such as lists, strings and dicts. It is true for atoms such as ints, floats, bools, chars, tags, and functions. The empty list is not an atom.",
    examples: ATOM_Q_EXAMPLES,
    related: &["len", "type", "shape"],
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
    details: "`dict[x]` expects a list whose items are two-item pairs with tag keys. Later duplicate keys replace earlier values.",
    examples: DICT_EXAMPLES,
    related: &["list", "tag", "keys"],
};
