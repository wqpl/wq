use crate::{repl::stdio::ReplStdin, value::Value, vm::GlobalMap, wqerr::WqErr};

pub trait ReplEngine {
    fn eval_string(&mut self, input: &str) -> Result<Value, WqErr>;
    fn get_environment(&self) -> Option<&GlobalMap>;
    fn clear_environment(&mut self);
    fn env_vars(&self) -> &GlobalMap;
    fn set_stdin(&mut self, stdin: Box<dyn ReplStdin>);
    fn arm_wqdb_next(&mut self);
    fn dbg_set_source(&mut self, path: &str, full_text: &str);
    fn dbg_set_offset(&mut self, offset: usize);
    fn dbg_print_bt(&mut self);
    fn set_bt_mode(&mut self, flag: bool);
    fn set_wqdb(&mut self, flag: bool);
    fn set_debug_level(&mut self, level: u8);
    fn get_debug_level(&mut self) -> u8;
    fn is_wqdb_enabled(&self) -> bool;
    fn reset_session(&mut self);
}
