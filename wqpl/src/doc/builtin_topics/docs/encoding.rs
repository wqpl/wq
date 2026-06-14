use crate::builtins::BuiltinEnum;

use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};

const DECODE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Decode UTF-8 bytes",
    code: "decode[(104;105);\"utf-8\"]",
    expectation: ExampleExpectation::ResultContains("\"hi\""),
}];

const ENCODE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Encode text as UTF-8 bytes",
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
    summary: "Decode bytes into text with a named character encoding.",
    details: "`decode[bytes;codec]` converts byte-shaped data into a string. `bytes` may be one int or bigint in `0..=255`, an int list, or a list of ints/bigints in that range. `codec` is an encoding label understood by `encoding_rs`, such as `\"utf-8\"` or `\"windows-1252\"`. The default mode is `\"s\"` for strict, which errors on invalid input; mode `\"r\"` replaces invalid byte sequences.",
    examples: DECODE_EXAMPLES,
    related: &["encode", "bytes?", "str"],
};

pub(super) const ENCODE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Encode,
    summary: "Encode text into bytes with a named character encoding.",
    details: "`encode[text;codec]` converts string-like data to an int list of bytes. `text` may be a string, char, unit, or a list of chars. `codec` is an encoding label understood by `encoding_rs`. The default mode is `\"s\"` for strict, which errors when text cannot be represented in the target encoding; mode `\"r\"` uses replacement behavior.",
    examples: ENCODE_EXAMPLES,
    related: &["decode", "bytes?", "ord"],
};

pub(super) const VALID_BYTES: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::ValidBytes,
    summary: "Return true when a value can be used as bytes.",
    details: "`bytes?[x]` checks whether `x` can be converted to a byte vector: a single int or bigint in `0..=255`, an int list whose items are all bytes, or a list of ints/bigints whose items are all bytes. It does not decode or inspect text; it only checks the numeric byte shape accepted by `encode`, `decode`, and `fwrite`.",
    examples: VALID_BYTES_EXAMPLES,
    related: &["encode", "decode", "fwrite"],
};
