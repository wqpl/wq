use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const DECODE_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Decode UTF-8 bytes",
        code: "decode[(104;105);\"utf-8\"]",
        expectation: ExampleExpectation::ResultContains("\"hi\""),
    },
    DocExample {
        title: "Replace malformed input and strip a BOM",
        code: "decode[(239;187;191;104;128;105);\"utf-8\";`mode:`replace;`bom:`strip]",
        expectation: ExampleExpectation::ResultContains("\"h\u{FFFD}i\""),
    },
];

const ENCODE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Encode a string as UTF-8 bytes",
    code: "encode[\"hi\";\"utf-8\"]",
    expectation: ExampleExpectation::ResultContains("(104;105)"),
}];

const VALID_BYTES_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Check byte-shaped values",
    code: "(bytes? (65;66);bytes? (65;256))",
    expectation: ExampleExpectation::ResultContains("(T;F)"),
}];

pub(super) const DECODE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Decode,
    summary: "Decode bytes into a string with a named character encoding.",
    details: "`decode[bytes;codec]` decodes one complete byte buffer.
`bytes` may be one int from 0 through 255 or a list of ints in that range.
`codec` accepts encoding labels such as `\"utf-8\"` and `\"windows-1252\"`.
The named `mode` argument accepts the `strict` tag by default or the `replace` tag to substitute malformed sequences.
The named `bom` argument accepts the `preserve` tag by default or the `strip` tag to remove a matching leading byte-order mark.
Strict errors identify the codec and byte offset and distinguish malformed input from an incomplete final sequence.
`decode` is not a stateful incremental decoder, so callers must join split byte chunks before decoding.",
    examples: DECODE_EXAMPLES,
    related: &["encode", "bytes?", "read"],
};

pub(super) const ENCODE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Encode,
    summary: "Encode a string or char with a named character encoding.",
    details: "`encode[value;codec]` converts a string, char, or empty list to a list of byte ints.
The named `mode` argument uses the `strict` tag by default and reports an unrepresentable character with its character offset.
Use the `replace` tag to apply the codec's replacement representation.",
    examples: ENCODE_EXAMPLES,
    related: &["decode", "bytes?", "write"],
};

pub(super) const VALID_BYTES: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::ValidBytes,
    summary: "Return true when a value can be used as bytes.",
    details: "`bytes?[x]` checks whether `x` is one int from 0 through 255 or a list whose items are all ints in that range.
It does not decode or inspect strings; it only checks the byte shape accepted by `decode` and `write` and returned by `encode`.",
    examples: VALID_BYTES_EXAMPLES,
    related: &["encode", "decode", "write"],
};
