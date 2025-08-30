use crate::{value::Value, wqerror::WqError};

use super::stdio::ReplStdin;

pub trait ReplEngine {
    fn eval_string(&mut self, input: &str) -> Result<Value, WqError>;
    // fn set_debug(&mut self, flag: bool);
    // fn is_debug(&self) -> bool;
    fn get_environment(&self) -> Option<&std::collections::HashMap<String, Value>>;
    fn clear_environment(&mut self);
    fn env_vars(&self) -> &std::collections::HashMap<String, Value>;
    fn set_stdin(&mut self, stdin: Box<dyn ReplStdin>);
    fn arm_wqdb_next(&mut self);
    fn dbg_set_source(&mut self, path: &str, full_text: &str);
    fn dbg_set_offset(&mut self, offset: usize);
    fn dbg_print_bt(&mut self);
    fn set_bt_mode(&mut self, flag: bool);
    fn set_wqdb(&mut self, flag: bool);
    fn set_debug_level(&mut self, level: u8);
    fn get_debug_level(&mut self) -> u8;
    fn is_wqdb_on(&self) -> bool;
    fn reset_session(&mut self);
}
