mod core;
mod higher_order;
mod list;
mod set;
mod string;

use super::super::model::BuiltinDoc;

pub(super) const BUILTIN_DOCS: &[BuiltinDoc] = &[
    core::ECHO,
    core::LEN,
    higher_order::MAP,
    list::SPLIT,
    set::HAS_Q,
    string::WORDS,
    string::STR,
];
