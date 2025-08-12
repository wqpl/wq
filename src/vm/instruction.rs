use crate::parser::{BinaryOperator, UnaryOperator};
use crate::value::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    LoadConst(Value),
    /// Load a global variable or builtin by name
    LoadVar(String),
    /// Store into a global variable by name
    StoreVar(String),
    /// Store into a global variable and keep value on stack
    StoreVarKeep(String),
    /// Load a local variable from an index-based slot
    LoadLocal(u16),
    /// Store into a local variable slot
    StoreLocal(u16),
    /// Store into a local slot and keep value on stack
    StoreLocalKeep(u16),
    BinaryOp(BinaryOperator),
    UnaryOp(UnaryOperator),
    CallBuiltin(String, usize),
    /// Builtin call resolved to an ID at compile time for faster dispatch
    CallBuiltinId(u8, usize),
    /// Call a function stored in a local slot
    CallLocal(u16, usize),
    CallUser(String, usize),
    CallAnon(usize),
    /// Call if the object is a function, otherwise index
    CallOrIndex(usize),
    MakeList(usize),
    MakeDict(usize),
    Index,
    IndexAssign,
    IndexAssignLocal(u16),
    /// Like IndexAssign, but does not push the assigned value
    IndexAssignDrop,
    /// Like IndexAssignLocal, but does not push the assigned value
    IndexAssignLocalDrop(u16),
    Jump(usize),
    JumpIfFalse(usize),
    /// Jump if left >= right (pops two operands)
    JumpIfGE(usize),
    /// Jump if local slot value <= 0
    JumpIfLEZLocal(u16, usize),
    Pop,
    Assert,
    Return,
    Try(usize),
}
