use crate::builtins::BuiltinEnum;

pub(super) const BUILTIN_ALIASES: &[(BuiltinEnum, BuiltinEnum)] = &[
    (BuiltinEnum::E, BuiltinEnum::Echo),
    (BuiltinEnum::V, BuiltinEnum::Reverse),
    (BuiltinEnum::R, BuiltinEnum::Reshape),
    (BuiltinEnum::TP, BuiltinEnum::Transpose),
    (BuiltinEnum::Z, BuiltinEnum::Where),
    (BuiltinEnum::A, BuiltinEnum::Apply),
    (BuiltinEnum::M, BuiltinEnum::Map),
    (BuiltinEnum::Reduce, BuiltinEnum::Fold),
    (BuiltinEnum::D, BuiltinEnum::Diff),
    (BuiltinEnum::I, BuiltinEnum::Integrate),
    (BuiltinEnum::U, BuiltinEnum::UnitQ),
];
