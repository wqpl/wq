use std::sync::Arc;

use crate::value::Value;

/// Core symbolic operators with stable internal names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CasOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Power,
}

impl CasOp {
    pub(crate) fn from_symbol(symbol: &str) -> Option<Self> {
        match symbol {
            "+" => Some(Self::Add),
            "-" => Some(Self::Subtract),
            "*" => Some(Self::Multiply),
            "/" => Some(Self::Divide),
            "^" => Some(Self::Power),
            _ => None,
        }
    }

    pub(crate) fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::Power => "^",
        }
    }
}

/// Symbolic algebra expression kind.
#[derive(Debug, Clone, PartialEq)]
pub enum CasKind {
    /// Symbolic variable (e.g. `x`).
    Var(Box<str>),
    /// Symbolic constant (e.g. `pi`).
    Const(Box<str>),
    /// Symbolic operator (e.g. `+`, `*`, `^`).
    Op(Box<str>, Arc<[Value]>),
    /// Symbolic function call (e.g. `sin`, `ln`).
    Call(Box<str>, Arc<[Value]>),
    /// Equation (lhs = rhs).
    Eq(Value, Value),
}

/// Heap-allocated symbolic algebra value.
#[derive(Debug, Clone)]
pub struct CasData {
    pub kind: CasKind,
}

#[cfg(test)]
mod cas_tests {
    use super::*;

    #[test]
    fn cas_var_construction() {
        let x = Value::from_cas_var("x");
        assert!(matches!(x, Value::Cas(_)));
        assert!(x.is_cas());
        assert!(x.is_cas_expr());
        assert!(!x.is_cas_equation());
    }

    #[test]
    fn cas_op_construction() {
        let sum = Value::from_cas_op("+", vec![Value::Int(1), Value::Int(2)]);
        assert!(sum.is_cas_expr());
        let (op, args) = sum.cas_op_parts().unwrap();
        assert_eq!(op, "+");
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn cas_call_construction() {
        let sin = Value::from_cas_call("sin", vec![Value::from_cas_var("x")]);
        assert!(sin.is_cas_expr());
        let (name, args) = sin.cas_call_parts().unwrap();
        assert_eq!(name, "sin");
        assert_eq!(args.len(), 1);
    }

    #[test]
    fn cas_eq_construction() {
        let eq = Value::from_cas_eq(Value::from_cas_var("x"), Value::Int(1));
        assert!(eq.is_cas_equation());
        let (lhs, rhs) = eq.cas_eq_parts().unwrap();
        assert_eq!(*lhs, Value::from_cas_var("x"));
        assert_eq!(*rhs, Value::Int(1));
    }

    #[test]
    fn cas_is_atom() {
        assert!(Value::from_cas_var("x").is_atom());
        assert!(Value::from_cas_op("+", vec![]).is_atom());
    }

    #[test]
    fn cas_type_name() {
        assert_eq!(Value::from_cas_var("x").type_name(), "cas");
    }

    #[test]
    fn cas_partial_eq() {
        let a = Value::from_cas_var("x");
        let b = Value::from_cas_var("x");
        let c = Value::from_cas_var("y");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn cas_op_symbol_roundtrip() {
        for (symbol, op) in [
            ("+", CasOp::Add),
            ("-", CasOp::Subtract),
            ("*", CasOp::Multiply),
            ("/", CasOp::Divide),
            ("^", CasOp::Power),
        ] {
            assert_eq!(CasOp::from_symbol(symbol), Some(op));
            assert_eq!(op.symbol(), symbol);
        }
        assert_eq!(CasOp::from_symbol("mod"), None);
    }

    #[test]
    fn cas_typed_op_accessors() {
        let sum = Value::from_cas_known_op(CasOp::Add, vec![Value::Int(1), Value::Int(2)]);
        let (op, args) = sum.cas_known_op_parts().unwrap();
        assert_eq!(op, CasOp::Add);
        assert_eq!(args.len(), 2);
        assert_eq!(sum.cas_op_args(CasOp::Add), Some(args));
    }

    #[test]
    fn unknown_raw_op_is_preserved() {
        let raw = Value::from_cas_op("mod", vec![Value::Int(5), Value::Int(2)]);
        let (op, args) = raw.cas_op_parts().unwrap();
        assert_eq!(op, "mod");
        assert_eq!(args.len(), 2);
        assert_eq!(raw.cas_known_op_parts(), None);
        assert_eq!(raw.cas_op_args(CasOp::Add), None);
    }
}
