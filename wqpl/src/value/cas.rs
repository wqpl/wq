use std::sync::Arc;

use crate::value::Value;

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
}
