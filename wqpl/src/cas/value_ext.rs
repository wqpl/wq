use std::sync::Arc;

use num_bigint::BigInt;
use num_traits::{One, ToPrimitive};

use super::format_expr;
use super::limit::LimitDirection;
use crate::value::Value;
use crate::value::cas::{CasConst, CasData, CasFunction, CasKind, CasOp, CasSymbol};

pub(crate) trait IntoCasOp {
    fn into_cas_op(self) -> CasOp;
}

impl IntoCasOp for CasOp {
    fn into_cas_op(self) -> CasOp {
        self
    }
}

impl IntoCasOp for &str {
    fn into_cas_op(self) -> CasOp {
        CasOp::from_symbol(self).unwrap_or_else(|| {
            unreachable!("internal CAS op constructor received unknown operator '{self}'")
        })
    }
}

impl IntoCasOp for String {
    fn into_cas_op(self) -> CasOp {
        self.as_str().into_cas_op()
    }
}

pub(crate) trait IntoCasConst {
    fn into_cas_const(self) -> CasConst;
}

impl IntoCasConst for CasConst {
    fn into_cas_const(self) -> CasConst {
        self
    }
}

impl IntoCasConst for &str {
    fn into_cas_const(self) -> CasConst {
        CasConst::from_name(self).unwrap_or_else(|| {
            unreachable!("internal CAS const constructor received unknown constant '{self}'")
        })
    }
}

impl IntoCasConst for String {
    fn into_cas_const(self) -> CasConst {
        self.as_str().into_cas_const()
    }
}

pub(crate) trait IntoCasFunction {
    fn into_cas_function(self) -> CasFunction;
}

impl IntoCasFunction for CasFunction {
    fn into_cas_function(self) -> CasFunction {
        self
    }
}

impl IntoCasFunction for &str {
    fn into_cas_function(self) -> CasFunction {
        CasFunction::from_name(self).unwrap_or_else(|| {
            unreachable!("internal CAS function constructor received unknown function '{self}'")
        })
    }
}

impl IntoCasFunction for String {
    fn into_cas_function(self) -> CasFunction {
        self.as_str().into_cas_function()
    }
}

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
            kind: CasKind::Var(CasSymbol::new(name)),
        }))
    }

    pub(crate) fn from_cas_op(op: impl IntoCasOp, args: Vec<Value>) -> Value {
        let op = op.into_cas_op();
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Op(op, Arc::from(args)),
        }))
    }

    pub(crate) fn from_cas_known_op(op: CasOp, args: Vec<Value>) -> Value {
        Self::from_cas_op(op, args)
    }

    pub(crate) fn from_cas_const(konst: impl IntoCasConst) -> Value {
        let konst = konst.into_cas_const();
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Const(konst),
        }))
    }

    pub(crate) fn from_cas_function(function: impl IntoCasFunction, args: Vec<Value>) -> Value {
        let function = function.into_cas_function();
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Function(function, Arc::from(args)),
        }))
    }

    pub(crate) fn from_cas_call(function: impl IntoCasFunction, args: Vec<Value>) -> Value {
        Self::from_cas_function(function, args)
    }

    pub(crate) fn from_cas_apply(name: impl Into<String>, args: Vec<Value>) -> Value {
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Apply(CasSymbol::new(name), Arc::from(args)),
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
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Limit {
                expr,
                var,
                point,
                direction,
            },
        }))
    }

    pub(crate) fn cas_var_name(&self) -> Option<&str> {
        match self {
            Value::Cas(cd) => match &cd.kind {
                CasKind::Var(name) => Some(name.as_str()),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn cas_const(&self) -> Option<CasConst> {
        match self {
            Value::Cas(cd) => match &cd.kind {
                CasKind::Const(konst) => Some(*konst),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn cas_const_name(&self) -> Option<&'static str> {
        self.cas_const().map(CasConst::name)
    }

    pub(crate) fn cas_op_parts(&self) -> Option<(CasOp, &[Value])> {
        match self {
            Value::Cas(cd) => match &cd.kind {
                CasKind::Op(op, args) => Some((*op, args)),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn cas_known_op_parts(&self) -> Option<(CasOp, &[Value])> {
        self.cas_op_parts()
    }

    pub(crate) fn cas_op_args(&self, expected: CasOp) -> Option<&[Value]> {
        let (op, args) = self.cas_known_op_parts()?;
        if op == expected { Some(args) } else { None }
    }

    pub(crate) fn cas_function_parts(&self) -> Option<(CasFunction, &[Value])> {
        match self {
            Value::Cas(cd) => match &cd.kind {
                CasKind::Function(function, args) => Some((*function, args)),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn cas_apply_parts(&self) -> Option<(&CasSymbol, &[Value])> {
        match self {
            Value::Cas(cd) => match &cd.kind {
                CasKind::Apply(name, args) => Some((name, args)),
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

    pub(crate) fn cas_limit_parts(
        &self,
    ) -> Option<(&Value, &Value, &Value, Option<LimitDirection>)> {
        match self {
            Value::Cas(cd) => match &cd.kind {
                CasKind::Limit {
                    expr,
                    var,
                    point,
                    direction,
                } => Some((expr, var, point, *direction)),
                _ => None,
            },
            _ => None,
        }
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
