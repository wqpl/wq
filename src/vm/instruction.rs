use crate::parser::{BinaryOperator, UnaryOperator};
use crate::value::valuei::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    LoadConst(Value),
    /// Load a global variable or builtin by name
    LoadVar(String),
    /// Store into a global variable by name
    StoreVar(String),
    /// Load a local variable from an index-based slot
    LoadLocal(u16),
    /// Store into a local variable slot
    StoreLocal(u16),
    BinaryOp(BinaryOperator),
    UnaryOp(UnaryOperator),
    CallBuiltin(String, usize),
    /// Builtin call resolved to an ID at compile time for faster dispatch
    CallBuiltinId(u8, usize),
    /// Call a function stored in a local slot
    CallLocal(u16, usize),
    CallUser(String, usize),
    CallAnon(usize),
    MakeList(usize),
    MakeDict(usize),
    Index,
    IndexAssign,
    IndexAssignLocal(u16),
    Jump(usize),
    JumpIfFalse(usize),
    Pop,
    Assert,
    Return,
}
