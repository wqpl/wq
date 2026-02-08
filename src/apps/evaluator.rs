pub mod default;

use crate::{
    debug_flags::DebugFlags, stdio::WqStdin, value::Value, vm::GlobalMap, wqerror::WqError,
};

pub trait Evaluator {
    fn eval_string(&mut self, input: &str) -> Result<Value, WqError>;
    fn get_environment(&self) -> Option<&GlobalMap>;
    fn clear_environment(&mut self);
    fn env_vars(&self) -> &GlobalMap;
    fn set_stdin(&mut self, stdin: Box<dyn WqStdin>);
    fn arm_wqdb_next(&mut self);
    fn dbg_set_source(&mut self, path: &str, full_text: &str);
    fn dbg_set_offset(&mut self, offset: usize);
    fn dbg_print_bt(&mut self);
    fn set_bt_mode(&mut self, flag: bool);
    fn set_wqdb(&mut self, flag: bool);
    fn set_debug_flags(&mut self, flags: DebugFlags);
    fn get_debug_flags(&mut self) -> DebugFlags;
    fn is_wqdb_enabled(&self) -> bool;
    fn reset_session(&mut self);
}
