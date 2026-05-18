use std::sync::Arc;

use num_bigint::BigInt;
use num_traits::{One, ToPrimitive};

use super::format_expr;
use super::limit::{self, LimitDirection};
use crate::value::Value;
use crate::value::cas::{CasData, CasKind};

impl Value {
    pub fn is_cas(&self) -> bool {
        matches!(self, Value::Cas(_))
    }

    pub(crate) fn is_cas_expr(&self) -> bool {
        matches!(self, Value::Cas(cd) if !matches!(cd.kind, CasKind::Eq(..)))
    }

    pub(crate) fn is_cas_equation(&self) -> bool {
        matches!(self, Value::Cas(cd) if matches!(cd.kind, CasKind::Eq(..)))
    }

    pub(crate) fn from_cas_var(name: impl Into<String>) -> Value {
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Var(name.into().into()),
        }))
    }

    pub(crate) fn from_cas_op(op: impl Into<String>, args: Vec<Value>) -> Value {
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Op(op.into().into(), Arc::from(args)),
        }))
    }

    pub(crate) fn from_cas_const(name: impl Into<String>) -> Value {
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Const(name.into().into()),
        }))
    }

    pub(crate) fn from_cas_call(name: impl Into<String>, args: Vec<Value>) -> Value {
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Call(name.into().into(), Arc::from(args)),
        }))
    }

    pub(crate) fn from_cas_eq(lhs: Value, rhs: Value) -> Value {
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Eq(lhs, rhs),
        }))
    }

    pub(crate) fn from_cas_limit(
        expr: Value,
        var: Value,
        point: Value,
        direction: Option<LimitDirection>,
    ) -> Value {
        let mut args = vec![expr, var, point];
        if let Some(dir) = direction {
            args.push(match dir {
                LimitDirection::Right => Value::from_cas_const("+"),
                LimitDirection::Left => Value::from_cas_const("-"),
            });
        }
        Value::from_cas_call("limit", args)
    }

    pub(crate) fn cas_var_name(&self) -> Option<&str> {
        match self {
            Value::Cas(cd) => match &cd.kind {
                CasKind::Var(name) => Some(&**name),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn cas_const_name(&self) -> Option<&str> {
        match self {
            Value::Cas(cd) => match &cd.kind {
                CasKind::Const(name) => Some(&**name),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn cas_op_parts(&self) -> Option<(&str, &[Value])> {
        match self {
            Value::Cas(cd) => match &cd.kind {
                CasKind::Op(op, args) => Some((op, args)),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn cas_call_parts(&self) -> Option<(&str, &[Value])> {
        match self {
            Value::Cas(cd) => match &cd.kind {
                CasKind::Call(name, args) => Some((name, args)),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn cas_eq_parts(&self) -> Option<(&Value, &Value)> {
        match self {
            Value::Cas(cd) => match &cd.kind {
                CasKind::Eq(lhs, rhs) => Some((lhs, rhs)),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn cas_limit_parts(&self) -> Option<(Value, Value, Value, Option<LimitDirection>)> {
        let (name, args) = self.cas_call_parts()?;
        if name != "limit" || args.len() < 3 || args.len() > 4 {
            return None;
        }
        let expr = args[0].clone();
        let var = args[1].clone();
        let point = args[2].clone();
        let direction = if args.len() == 4 {
            limit::parse_limit_direction(&args[3])
        } else {
            None
        };
        Some((expr, var, point, direction))
    }

    pub(crate) fn format_cas(&self) -> Option<String> {
        if self.is_cas_expr() {
            Some(format_expr(self, 0))
        } else if let Some((lhs, rhs)) = self.cas_eq_parts() {
            Some(format!("{} = {}", format_expr(lhs, 0), format_expr(rhs, 0)))
        } else {
            None
        }
    }

    pub(crate) fn exact_int(&self) -> Option<BigInt> {
        let (numer, denom) = self.rational_parts()?;
        if denom.is_one() { Some(numer) } else { None }
    }

    pub(crate) fn exact_int_is(&self, expected: i64) -> bool {
        match self {
            Value::Int(i) => *i == expected,
            Value::BigInt(b) => b.to_i64().is_some_and(|i| i == expected),
            _ => false,
        }
    }

    pub(crate) fn exact_half(&self) -> bool {
        self.rational_parts()
            .is_some_and(|(numer, denom)| numer == BigInt::one() && denom == BigInt::from(2))
    }

    pub(crate) fn exact_neg_half(&self) -> bool {
        self.rational_parts()
            .is_some_and(|(numer, denom)| numer == BigInt::from(-1) && denom == BigInt::from(2))
    }
}
