use crate::{
    value::{Value, WqResult},
    wqerror::WqError,
};

impl Value {
    pub fn abs(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => n
                .checked_abs()
                .map(Value::Int)
                .ok_or(WqError::ArithmeticOverflowError("`abs`: overflow".into())),
            Value::Float(f) => Ok(Value::Float(f.abs())),
            _ => Err(WqError::DomainError(format!(
                "`abs`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn sgn(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Int(n.signum())),
            Value::Float(f) => Ok(Value::Float(f.signum())),
            _ => Err(WqError::DomainError(format!(
                "`sgn`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn sqrt(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).sqrt())),
            Value::Float(f) => Ok(Value::Float(f.sqrt())),
            _ => Err(WqError::DomainError(format!(
                "`sqrt`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn exp(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).exp())),
            Value::Float(f) => Ok(Value::Float(f.exp())),
            _ => Err(WqError::DomainError(format!(
                "`exp`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn ln(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).ln())),
            Value::Float(f) => Ok(Value::Float(f.ln())),
            _ => Err(WqError::DomainError(format!(
                "`ln`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn log(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |v1, v2| match (v1, v2) {
            (Value::Int(n1), Value::Int(n2)) => Ok(Value::Float((*n1 as f64).log(*n2 as f64))),
            (Value::Int(n1), Value::Float(n2)) => Ok(Value::Float((*n1 as f64).log(*n2))),
            (Value::Float(n1), Value::Int(n2)) => Ok(Value::Float(n1.log(*n2 as f64))),
            (Value::Float(f1), Value::Float(f2)) => Ok(Value::Float(f1.log(*f2))),
            _ => Err(WqError::DomainError(format!(
                "`log`: cannot operate on {} and {} of types {} and {}",
                v1,
                v2,
                v1.type_name_verbose(),
                v2.type_name_verbose()
            ))),
        })
    }

    pub fn floor(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Int(*n)),
            // cast to i64
            Value::Float(f) => Ok(Value::Int(f.floor() as i64)),
            _ => Err(WqError::DomainError(format!(
                "`floor`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn ceil(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Int(*n)),
            Value::Float(f) => Ok(Value::Int(f.ceil() as i64)),
            _ => Err(WqError::DomainError(format!(
                "`ceil`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn round(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Int(*n)),
            Value::Float(f) => Ok(Value::Int(f.round() as i64)),
            _ => Err(WqError::DomainError(format!(
                "`round`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn sin(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).sin())),
            Value::Float(f) => Ok(Value::Float(f.sin())),
            _ => Err(WqError::DomainError(format!(
                "`sin`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn cos(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).cos())),
            Value::Float(f) => Ok(Value::Float(f.cos())),
            _ => Err(WqError::DomainError(format!(
                "`cos`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn tan(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).tan())),
            Value::Float(f) => Ok(Value::Float(f.tan())),
            _ => Err(WqError::DomainError(format!(
                "`tan`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn sinh(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).sinh())),
            Value::Float(f) => Ok(Value::Float(f.sinh())),
            _ => Err(WqError::DomainError(format!(
                "`sinh`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn cosh(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).cosh())),
            Value::Float(f) => Ok(Value::Float(f.cosh())),
            _ => Err(WqError::DomainError(format!(
                "`cosh`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn tanh(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).tanh())),
            Value::Float(f) => Ok(Value::Float(f.tanh())),
            _ => Err(WqError::DomainError(format!(
                "`tanh`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn arcsin(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).asin())),
            Value::Float(f) => Ok(Value::Float(f.asin())),
            _ => Err(WqError::DomainError(format!(
                "`arcsin`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn arccos(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).acos())),
            Value::Float(f) => Ok(Value::Float(f.acos())),
            _ => Err(WqError::DomainError(format!(
                "`arccos`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn arctan(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).atan())),
            Value::Float(f) => Ok(Value::Float(f.atan())),
            _ => Err(WqError::DomainError(format!(
                "`arctan`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn arcsinh(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).asinh())),
            Value::Float(f) => Ok(Value::Float(f.asinh())),
            _ => Err(WqError::DomainError(format!(
                "`arcsinh`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn arccosh(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).acosh())),
            Value::Float(f) => Ok(Value::Float(f.acosh())),
            _ => Err(WqError::DomainError(format!(
                "`arccosh`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }

    pub fn arctanh(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Float((*n as f64).atanh())),
            Value::Float(f) => Ok(Value::Float(f.atanh())),
            _ => Err(WqError::DomainError(format!(
                "`arctanh`: cannot operate on {} of type {}",
                v,
                v.type_name_verbose()
            ))),
        })
    }
}
