use crate::{
    evaluator::Evaluator,
    value::valuei::{Value, WqError},
    vm::VmEvaluator,
};

pub trait ReplEngine {
    fn eval_string(&mut self, input: &str) -> Result<Value, WqError>;
    fn set_debug(&mut self, flag: bool);
    fn is_debug(&self) -> bool;
    fn get_environment(&self) -> Option<&std::collections::HashMap<String, Value>>;
    fn clear_environment(&mut self);
    fn env_vars(&self) -> &std::collections::HashMap<String, Value>;
}

impl ReplEngine for Evaluator {
    fn eval_string(&mut self, input: &str) -> Result<Value, WqError> {
        Evaluator::eval_string(self, input)
    }
    fn set_debug(&mut self, flag: bool) {
        Evaluator::set_debug(self, flag)
    }
    fn is_debug(&self) -> bool {
        Evaluator::is_debug(self)
    }
    fn get_environment(&self) -> Option<&std::collections::HashMap<String, Value>> {
        Evaluator::get_environment(self)
    }
    fn clear_environment(&mut self) {
        self.environment_mut().clear();
    }
    fn env_vars(&self) -> &std::collections::HashMap<String, Value> {
        self.environment().variables()
    }
}

impl ReplEngine for VmEvaluator {
    fn eval_string(&mut self, input: &str) -> Result<Value, WqError> {
        VmEvaluator::eval_string(self, input)
    }
    fn set_debug(&mut self, flag: bool) {
        VmEvaluator::set_debug(self, flag)
    }
    fn is_debug(&self) -> bool {
        VmEvaluator::is_debug(self)
    }
    fn get_environment(&self) -> Option<&std::collections::HashMap<String, Value>> {
        VmEvaluator::get_environment(self)
    }
    fn clear_environment(&mut self) {
        self.environment_mut().clear();
    }
    fn env_vars(&self) -> &std::collections::HashMap<String, Value> {
        self.environment()
    }
}
