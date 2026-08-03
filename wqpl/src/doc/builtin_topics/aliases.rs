use crate::builtins::BuiltinEnum;

pub(super) const BUILTIN_ALIASES: &[(BuiltinEnum, BuiltinEnum)] = &[
    (BuiltinEnum::R, BuiltinEnum::Reshape),
    (BuiltinEnum::TP, BuiltinEnum::Transpose),
    (BuiltinEnum::M, BuiltinEnum::Map),
    (BuiltinEnum::Reduce, BuiltinEnum::Fold),
    (BuiltinEnum::D, BuiltinEnum::Diff),
    (BuiltinEnum::I, BuiltinEnum::Integrate),
    (BuiltinEnum::Not, BuiltinEnum::OpTilde),
];
