mod core;
mod higher_order;
mod list;
mod set;
mod string;

use super::super::model::BuiltinDoc;

pub(super) const BUILTIN_DOCS: &[BuiltinDoc] = &[
    core::BFN,
    core::CHR,
    core::ORD,
    core::INT,
    core::FLOAT,
    core::BIN,
    core::OCT,
    core::HEX,
    core::HASH,
    core::RAISE,
    core::ECHO,
    core::PRINT,
    core::INPUT,
    core::EXEC,
    core::LEN,
    higher_order::MAP,
    list::SPLIT,
    set::HAS_Q,
    string::WORDS,
    string::STR,
];
