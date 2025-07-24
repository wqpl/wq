use crate::parser::{BinaryOperator, UnaryOperator};
use crate::value::valuei::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    LoadConst(Value),
    LoadVar(String),
    StoreVar(String),
    BinaryOp(BinaryOperator),
    UnaryOp(UnaryOperator),
    CallBuiltin(String, usize),
    /// Builtin call resolved to an ID at compile time for faster dispatch
    CallBuiltinId(u8, usize),
    CallUser(String, usize),
    CallAnon(usize),
    MakeList(usize),
    MakeDict(usize),
    Index,
    IndexAssign,
    Jump(usize),
    JumpIfFalse(usize),
    Pop,
    Return,
}
