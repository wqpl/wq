use crate::value::{Value, WqResult};

impl Value {
    pub fn wq_where(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(0) => Ok(Value::Bool(false)),
            Value::Float(0.0) => Ok(Value::Bool(false)),
            Value::Bool(false) => Ok(Value::Bool(false)),
            _ => Ok(Value::Bool(true)),
        })
    }
}
