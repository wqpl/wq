use std::sync::Arc;

use num_bigint::BigInt;
use num_traits::{One, ToPrimitive};

use super::limit::LimitDirection;
use super::{format_cas_equation, format_cas_value};
use crate::value::Value;
use crate::value::cas::{CasConst, CasData, CasFunction, CasKind, CasOp, CasPredicate, CasSymbol};

impl Value {
    pub fn is_cas(&self) -> bool {
        matches!(self, Value::Cas(_))
    }

    pub(crate) fn is_cas_expr(&self) -> bool {
        matches!(self, Value::Cas(cd) if !matches!(cd.kind, CasKind::Eq(..) | CasKind::Predicate(..)))
    }

    pub(crate) fn is_cas_equation(&self) -> bool {
        matches!(self, Value::Cas(cd) if matches!(cd.kind, CasKind::Eq(..)))
    }

    pub(crate) fn from_cas_var(name: impl Into<String>) -> Value {
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Var(CasSymbol::new(name)),
        }))
    }

    /// Raw structural CAS operator builder.
    ///
    /// Use canonical constructors from `crate::cas` such as `cas_add`,
    /// `cas_mul`, `cas_div`, and `cas_pow` when creating a new semantic
    /// expression. This helper is only for preserving or rebuilding an
    /// already-normalized shape.
    pub(crate) fn from_cas_op(op: CasOp, args: Vec<Value>) -> Value {
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Op(op, Arc::from(args)),
        }))
    }

    pub(crate) fn from_cas_const(konst: CasConst) -> Value {
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Const(konst),
        }))
    }

    /// Raw structural CAS function builder.
    ///
    /// Prefer `cas_call_expr` or a function-specific canonical helper when
    /// constructing new semantic expressions.
    pub(crate) fn from_cas_function(function: CasFunction, args: Vec<Value>) -> Value {
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Function(function, Arc::from(args)),
        }))
    }

    /// Raw structural CAS application builder.
    ///
    /// Prefer canonical CAS constructors for new semantic expressions.
    pub(crate) fn from_cas_apply(name: impl Into<String>, args: Vec<Value>) -> Value {
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Apply(CasSymbol::new(name), Arc::from(args)),
        }))
    }

    pub(crate) fn from_cas_named_arg(name: impl Into<String>, value: Value) -> Value {
        Value::Cas(Arc::new(CasData {
            kind: CasKind::NamedArg(CasSymbol::new(name), value),
        }))
    }

    pub(crate) fn from_cas_eq(lhs: Value, rhs: Value) -> Value {
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Eq(lhs, rhs),
        }))
    }

    pub(crate) fn from_cas_nonzero(expr: Value) -> Value {
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Predicate(CasPredicate::NonZero(expr)),
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

    pub(crate) fn from_cas_root(poly: Value, lo: f64, hi: f64) -> Value {
        Value::Cas(Arc::new(CasData {
            kind: CasKind::Root { poly, lo, hi },
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

    pub(crate) fn cas_named_arg_parts(&self) -> Option<(&CasSymbol, &Value)> {
        match self {
            Value::Cas(cd) => match &cd.kind {
                CasKind::NamedArg(name, value) => Some((name, value)),
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

    pub(crate) fn cas_predicate(&self) -> Option<&CasPredicate> {
        match self {
            Value::Cas(cd) => match &cd.kind {
                CasKind::Predicate(predicate) => Some(predicate),
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

    pub(crate) fn cas_root_parts(&self) -> Option<(&Value, f64, f64)> {
        match self {
            Value::Cas(cd) => match &cd.kind {
                CasKind::Root { poly, lo, hi } => Some((poly, *lo, *hi)),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn format_cas(&self) -> Option<String> {
        if self.is_cas_expr() {
            Some(format_cas_value(self))
        } else if let Some((lhs, rhs)) = self.cas_eq_parts() {
            Some(format_cas_equation(lhs, rhs))
        } else if let Some(CasPredicate::NonZero(expr)) = self.cas_predicate() {
            Some(format!("nonzero[{}]", format_cas_value(expr)))
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
