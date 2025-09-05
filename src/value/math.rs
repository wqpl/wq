use crate::{
    value::{Value, WqResult},
    wqerror::WqError,
};

use super::op::{type_err1, type_err2};

impl Value {
    pub fn abs(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => n
                .checked_abs()
                .map(Value::Int)
                .ok_or(WqError::ArithmeticOverflowError("`abs`: overflow".into())),
            Value::Float(f) => Ok(Value::Float(f.abs())),
            _ => Err(type_err1("abs", v)),
        })
    }

    pub fn sgn(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Int(n.signum())),
            Value::Float(f) => Ok(Value::Float(f.signum())),
            _ => Err(type_err1("sgn", v)),
        })
    }

    pub fn sqrt(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).sqrt())),
            Value::Float(f) => Ok(Value::Float(f.sqrt())),
            _ => Err(type_err1("sqrt", v)),
        })
    }

    pub fn exp(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).exp())),
            Value::Float(f) => Ok(Value::Float(f.exp())),
            _ => Err(type_err1("exp", v)),
        })
    }

    pub fn ln(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).ln())),
            Value::Float(f) => Ok(Value::Float(f.ln())),
            _ => Err(type_err1("ln", v)),
        })
    }

    pub fn log2(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).log2())),
            Value::Float(f) => Ok(Value::Float(f.log2())),
            _ => Err(type_err1("log2", v)),
        })
    }

    pub fn log10(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).log10())),
            Value::Float(f) => Ok(Value::Float(f.log10())),
            _ => Err(type_err1("log10", v)),
        })
    }

    pub fn log(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |v1, v2| match (v1, v2) {
            (Value::Int(n1), Value::Int(n2)) => Ok(Value::Float((*n1 as f64).log(*n2 as f64))),
            (Value::Int(n1), Value::Float(n2)) => Ok(Value::Float((*n1 as f64).log(*n2))),
            (Value::Float(n1), Value::Int(n2)) => Ok(Value::Float(n1.log(*n2 as f64))),
            (Value::Float(f1), Value::Float(f2)) => Ok(Value::Float(f1.log(*f2))),
            _ => Err(type_err2("log", v1, v2)),
        })
    }

    pub fn floor(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Int(*n)),
            // cast to i64
            Value::Float(f) => Ok(Value::Int(f.floor() as i64)),
            _ => Err(type_err1("floor", v)),
        })
    }

    pub fn ceil(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Int(*n)),
            Value::Float(f) => Ok(Value::Int(f.ceil() as i64)),
            _ => Err(type_err1("ceil", v)),
        })
    }

    pub fn round(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Int(*n)),
            Value::Float(f) => Ok(Value::Int(f.round() as i64)),
            _ => Err(type_err1("round", v)),
        })
    }

    pub fn sin(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).sin())),
            Value::Float(f) => Ok(Value::Float(f.sin())),
            _ => Err(type_err1("sin", v)),
        })
    }

    pub fn cos(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).cos())),
            Value::Float(f) => Ok(Value::Float(f.cos())),
            _ => Err(type_err1("cos", v)),
        })
    }

    pub fn tan(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).tan())),
            Value::Float(f) => Ok(Value::Float(f.tan())),
            _ => Err(type_err1("tan", v)),
        })
    }

    pub fn sinh(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).sinh())),
            Value::Float(f) => Ok(Value::Float(f.sinh())),
            _ => Err(type_err1("sinh", v)),
        })
    }

    pub fn cosh(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).cosh())),
            Value::Float(f) => Ok(Value::Float(f.cosh())),
            _ => Err(type_err1("cosh", v)),
        })
    }

    pub fn tanh(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).tanh())),
            Value::Float(f) => Ok(Value::Float(f.tanh())),
            _ => Err(type_err1("tanh", v)),
        })
    }

    pub fn arcsin(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).asin())),
            Value::Float(f) => Ok(Value::Float(f.asin())),
            _ => Err(type_err1("arcsin", v)),
        })
    }

    pub fn arccos(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).acos())),
            Value::Float(f) => Ok(Value::Float(f.acos())),
            _ => Err(type_err1("arccos", v)),
        })
    }

    pub fn arctan(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).atan())),
            Value::Float(f) => Ok(Value::Float(f.atan())),
            _ => Err(type_err1("arctan", v)),
        })
    }

    pub fn arcsinh(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).asinh())),
            Value::Float(f) => Ok(Value::Float(f.asinh())),
            _ => Err(type_err1("arcsinh", v)),
        })
    }

    pub fn arccosh(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).acosh())),
            Value::Float(f) => Ok(Value::Float(f.acosh())),
            _ => Err(type_err1("arccosh", v)),
        })
    }

    pub fn arctanh(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).atanh())),
            Value::Float(f) => Ok(Value::Float(f.atanh())),
            _ => Err(type_err1("arctanh", v)),
        })
    }
}
