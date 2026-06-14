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
    (BuiltinEnum::Not, BuiltinEnum::OpTilde),
    (BuiltinEnum::And, BuiltinEnum::OpBoolAnd),
    (BuiltinEnum::Or, BuiltinEnum::OpBoolOr),
    (BuiltinEnum::Xor, BuiltinEnum::OpXor),
    (BuiltinEnum::Band, BuiltinEnum::OpBitAnd),
    (BuiltinEnum::Bor, BuiltinEnum::OpBitOr),
    (BuiltinEnum::Shl, BuiltinEnum::OpShl),
    (BuiltinEnum::Shr, BuiltinEnum::OpShr),
];
