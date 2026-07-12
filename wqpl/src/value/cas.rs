use std::fmt;
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

impl fmt::Display for CasOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol())
    }
}

/// Symbolic variable name used by CAS expressions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct CasSymbol(Box<str>);

impl CasSymbol {
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self(name.into().into_boxed_str())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CasSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Symbolic constants with stable internal identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CasConst {
    Pi,
    E,
    Infinity,
    NegInfinity,
    Undefined,
}

impl CasConst {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "pi" => Some(Self::Pi),
            "e" => Some(Self::E),
            "inf" | "oo" => Some(Self::Infinity),
            "-inf" | "-oo" | "_oo" => Some(Self::NegInfinity),
            "undef" => Some(Self::Undefined),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Pi => "pi",
            Self::E => "e",
            Self::Infinity => "inf",
            Self::NegInfinity => "-inf",
            Self::Undefined => "undef",
        }
    }
}

impl fmt::Display for CasConst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Built-in CAS functions with known semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CasFunction {
    Abs,
    Sgn,
    Sin,
    Cos,
    Tan,
    Sec,
    Csc,
    Cot,
    Erf,
    Erfc,
    Gamma,
    LnGamma,
    Si,
    Ci,
    Ei,
    En,
    EllPk,
    EllPe,
    EllIk,
    EllIe,
    Heaviside,
    Delta,
    Exp,
    Ln,
    Log,
    Log2,
    Log10,
    Sqrt,
    ArcSin,
    ArcCos,
    ArcTan,
    ArcTan2,
    Sinh,
    Cosh,
    Tanh,
    ArcSinh,
    ArcCosh,
    ArcTanh,
    Floor,
    Ceil,
    Round,
    Integrate,
}

impl CasFunction {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "abs" => Some(Self::Abs),
            "sgn" => Some(Self::Sgn),
            "sin" => Some(Self::Sin),
            "cos" => Some(Self::Cos),
            "tan" => Some(Self::Tan),
            "sec" => Some(Self::Sec),
            "csc" => Some(Self::Csc),
            "cot" => Some(Self::Cot),
            "erf" => Some(Self::Erf),
            "erfc" => Some(Self::Erfc),
            "gamma" => Some(Self::Gamma),
            "lngamma" => Some(Self::LnGamma),
            "si" => Some(Self::Si),
            "ci" => Some(Self::Ci),
            "ei" => Some(Self::Ei),
            "en" => Some(Self::En),
            "ellpk" => Some(Self::EllPk),
            "ellpe" => Some(Self::EllPe),
            "ellik" => Some(Self::EllIk),
            "ellie" => Some(Self::EllIe),
            "heaviside" => Some(Self::Heaviside),
            "delta" => Some(Self::Delta),
            "exp" => Some(Self::Exp),
            "ln" => Some(Self::Ln),
            "log" => Some(Self::Log),
            "log2" => Some(Self::Log2),
            "log10" => Some(Self::Log10),
            "sqrt" => Some(Self::Sqrt),
            "arcsin" => Some(Self::ArcSin),
            "arccos" => Some(Self::ArcCos),
            "arctan" => Some(Self::ArcTan),
            "arctan2" => Some(Self::ArcTan2),
            "sinh" => Some(Self::Sinh),
            "cosh" => Some(Self::Cosh),
            "tanh" => Some(Self::Tanh),
            "arcsinh" => Some(Self::ArcSinh),
            "arccosh" => Some(Self::ArcCosh),
            "arctanh" => Some(Self::ArcTanh),
            "floor" => Some(Self::Floor),
            "ceil" => Some(Self::Ceil),
            "round" => Some(Self::Round),
            "integrate" => Some(Self::Integrate),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Abs => "abs",
            Self::Sgn => "sgn",
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Tan => "tan",
            Self::Sec => "sec",
            Self::Csc => "csc",
            Self::Cot => "cot",
            Self::Erf => "erf",
            Self::Erfc => "erfc",
            Self::Gamma => "gamma",
            Self::LnGamma => "lngamma",
            Self::Si => "si",
            Self::Ci => "ci",
            Self::Ei => "ei",
            Self::En => "en",
            Self::EllPk => "ellpk",
            Self::EllPe => "ellpe",
            Self::EllIk => "ellik",
            Self::EllIe => "ellie",
            Self::Heaviside => "heaviside",
            Self::Delta => "delta",
            Self::Exp => "exp",
            Self::Ln => "ln",
            Self::Log => "log",
            Self::Log2 => "log2",
            Self::Log10 => "log10",
            Self::Sqrt => "sqrt",
            Self::ArcSin => "arcsin",
            Self::ArcCos => "arccos",
            Self::ArcTan => "arctan",
            Self::ArcTan2 => "arctan2",
            Self::Sinh => "sinh",
            Self::Cosh => "cosh",
            Self::Tanh => "tanh",
            Self::ArcSinh => "arcsinh",
            Self::ArcCosh => "arccosh",
            Self::ArcTanh => "arctanh",
            Self::Floor => "floor",
            Self::Ceil => "ceil",
            Self::Round => "round",
            Self::Integrate => "integrate",
        }
    }
}

impl fmt::Display for CasFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Symbolic algebra expression kind.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CasKind {
    /// Symbolic variable (e.g. `x`).
    Var(CasSymbol),
    /// Symbolic constant (e.g. `pi`).
    Const(CasConst),
    /// Symbolic operator (e.g. `+`, `*`, `^`).
    Op(CasOp, Arc<[Value]>),
    /// Symbolic function call (e.g. `sin`, `ln`).
    Function(CasFunction, Arc<[Value]>),
    /// Uninterpreted symbolic application (e.g. `f[x]`).
    Apply(CasSymbol, Arc<[Value]>),
    /// Named symbolic call argument (e.g. `` `d:x``).
    NamedArg(CasSymbol, Value),
    /// Unevaluated limit special form.
    Limit {
        expr: Value,
        var: Value,
        point: Value,
        direction: Option<crate::cas::limit::LimitDirection>,
    },
    /// Opaque real root of an exact polynomial on a finite isolating interval.
    Root { poly: Value, lo: f64, hi: f64 },
    /// Equation (lhs = rhs).
    Eq(Value, Value),
    /// Symbolic condition used by CAS assumption contexts.
    Predicate(CasPredicate),
}

/// Atomic symbolic facts accepted by CAS assumption contexts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CasPredicate {
    /// The expression equals zero.
    Zero(Value),
    /// The expression is defined and unequal to zero.
    NonZero(Value),
    /// The expression is real and greater than zero.
    Positive(Value),
    /// The expression is real and less than zero.
    Negative(Value),
    /// The expression is real and greater than or equal to zero.
    NonNegative(Value),
    /// The expression is real.
    Real(Value),
    /// The expression is an integer.
    Integer(Value),
}

impl CasPredicate {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Zero(_) => "zero",
            Self::NonZero(_) => "nonzero",
            Self::Positive(_) => "positive",
            Self::Negative(_) => "negative",
            Self::NonNegative(_) => "nonnegative",
            Self::Real(_) => "real",
            Self::Integer(_) => "integer",
        }
    }

    pub(crate) fn expr(&self) -> &Value {
        match self {
            Self::Zero(expr)
            | Self::NonZero(expr)
            | Self::Positive(expr)
            | Self::Negative(expr)
            | Self::NonNegative(expr)
            | Self::Real(expr)
            | Self::Integer(expr) => expr,
        }
    }
}

/// Heap-allocated symbolic algebra value.
#[derive(Debug, Clone)]
pub struct CasData {
    pub(crate) kind: CasKind,
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
        let sum = Value::from_cas_op(CasOp::Add, vec![Value::Int(1), Value::Int(2)]);
        assert!(sum.is_cas_expr());
        let (op, args) = sum.cas_op_parts().unwrap();
        assert_eq!(op, CasOp::Add);
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn cas_call_construction() {
        let sin = Value::from_cas_function(CasFunction::Sin, vec![Value::from_cas_var("x")]);
        assert!(sin.is_cas_expr());
        let (function, args) = sin.cas_function_parts().unwrap();
        assert_eq!(function, CasFunction::Sin);
        assert_eq!(args.len(), 1);
    }

    #[test]
    fn cas_apply_construction() {
        let app = Value::from_cas_apply("f", vec![Value::from_cas_var("x")]);
        assert!(app.is_cas_expr());
        let (name, args) = app.cas_apply_parts().unwrap();
        assert_eq!(name.as_str(), "f");
        assert_eq!(args.len(), 1);
        assert!(app.cas_function_parts().is_none());
    }

    #[test]
    fn cas_apply_formats_and_compares_structurally() {
        let lhs = Value::from_cas_apply("f", vec![Value::from_cas_var("x")]);
        let rhs = Value::from_cas_apply("f", vec![Value::from_cas_var("x")]);
        let other_head = Value::from_cas_apply("g", vec![Value::from_cas_var("x")]);
        let builtin = Value::from_cas_function(CasFunction::Sin, vec![Value::from_cas_var("x")]);

        assert_eq!(lhs.to_string(), "f[x]");
        assert_eq!(lhs, rhs);
        assert_ne!(lhs, other_head);
        assert_ne!(lhs, builtin);
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
    fn cas_predicate_is_not_an_expression() {
        let predicate = Value::from_cas_nonzero(Value::from_cas_var("x"));
        assert!(predicate.is_cas());
        assert!(!predicate.is_cas_expr());
        assert!(!predicate.is_cas_equation());
        assert_eq!(predicate.to_string(), "nonzero[x]");
        assert!(crate::cas::cas_add(vec![predicate, Value::Int(1)]).is_err());
    }

    #[test]
    fn cas_is_atom() {
        assert!(Value::from_cas_var("x").is_atom());
        assert!(Value::from_cas_op(CasOp::Add, vec![]).is_atom());
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
    fn cas_typed_op_accessors() {
        let sum = Value::from_cas_op(CasOp::Add, vec![Value::Int(1), Value::Int(2)]);
        let (op, args) = sum.cas_op_parts().unwrap();
        assert_eq!(op, CasOp::Add);
        assert_eq!(args.len(), 2);
        assert_eq!(sum.cas_op_args(CasOp::Add), Some(args));
    }

    #[test]
    fn canonical_cas_constructors_collapse_empty_variadic_ops() {
        assert_eq!(crate::cas::cas_add(vec![]), Ok(Value::Int(0)));
        assert_eq!(crate::cas::cas_mul(vec![]), Ok(Value::Int(1)));
    }

    #[test]
    fn cas_simplifier_rejects_malformed_fixed_arity_ops() {
        let x = Value::from_cas_var("x");
        assert!(crate::cas::simplify_cas_value(&Value::from_cas_op(CasOp::Add, vec![])).is_err());
        assert!(
            crate::cas::simplify_cas_value(&Value::from_cas_op(
                CasOp::Multiply,
                vec![Value::Int(1)]
            ))
            .is_err()
        );
        assert!(
            crate::cas::simplify_cas_value(&Value::from_cas_op(CasOp::Power, vec![x])).is_err()
        );
        assert!(
            crate::cas::simplify_cas_value(&Value::from_cas_op(
                CasOp::Divide,
                vec![Value::Int(1), Value::Int(2), Value::Int(3)]
            ))
            .is_err()
        );
    }

    #[test]
    fn cas_function_name_roundtrip() {
        for (name, function) in [
            ("sin", CasFunction::Sin),
            ("ln", CasFunction::Ln),
            ("sqrt", CasFunction::Sqrt),
            ("arctan2", CasFunction::ArcTan2),
            ("integrate", CasFunction::Integrate),
        ] {
            assert_eq!(CasFunction::from_name(name), Some(function));
            assert_eq!(function.name(), name);
        }
        assert_eq!(CasFunction::from_name("f"), None);
    }

    #[test]
    fn cas_const_name_roundtrip() {
        for (name, konst) in [
            ("pi", CasConst::Pi),
            ("e", CasConst::E),
            ("inf", CasConst::Infinity),
            ("-inf", CasConst::NegInfinity),
            ("undef", CasConst::Undefined),
        ] {
            assert_eq!(CasConst::from_name(name), Some(konst));
            assert_eq!(CasConst::from_name(konst.name()), Some(konst));
        }
        assert_eq!(CasConst::from_name("oo"), Some(CasConst::Infinity));
        assert_eq!(CasConst::from_name("-oo"), Some(CasConst::NegInfinity));
        assert_eq!(CasConst::from_name("_oo"), Some(CasConst::NegInfinity));
        assert_eq!(CasConst::from_name("\u{221e}"), None);
        assert_eq!(CasConst::from_name("-\u{221e}"), None);
        assert_eq!(CasConst::from_name("x"), None);
    }
}
