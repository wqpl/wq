pub mod access;
pub mod algebraic;
pub mod bc;
pub mod cas;
pub mod cell;
pub mod cmp;
pub mod convert;
pub mod display;
mod error;
pub mod func;
pub mod hash;
pub mod mat;
pub mod math;
pub mod meta;
pub mod op;
pub mod rng;
pub mod seq;
pub mod stream;
pub(crate) mod unpack;

use std::fmt;
use std::sync::{Arc, Mutex};

pub(crate) use convert::IntoWqValue;
pub use display::Excerpt;
pub(crate) use display::into_wq_string;
pub(crate) use error::*;
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_complex::Complex64;
use num_rational::Ratio;
pub(crate) use op::{eval_binary, eval_bool_op, eval_unary};
use ordered_float::OrderedFloat;

use crate::ast::{BinaryOperator, UnaryOperator};
use crate::value::cas::CasData;
use crate::value::func::{CallableExpr, ClosureData, FunctionData, LiftedCallableData};
use crate::value::rng::RngState;
use crate::value::stream::StreamHandle;
use crate::wqerror::WqError;

pub type WqResult<T> = Result<T, WqError>;

/// A value's stable, user-facing category.
///
/// Categories intentionally hide storage details such as integer width and
/// specialized list representations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ValueCategory {
    Int,
    Float,
    Complex,
    Fraction,
    Algebraic,
    Char,
    Tag,
    Bool,
    List,
    Cas,
    Dict,
    Function,
    Rng,
    Stream,
}

impl ValueCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Int => "int",
            Self::Float => "float",
            Self::Complex => "complex",
            Self::Fraction => "fraction",
            Self::Algebraic => "algebraic",
            Self::Char => "char",
            Self::Tag => "tag",
            Self::Bool => "bool",
            Self::List => "list",
            Self::Cas => "cas",
            Self::Dict => "dict",
            Self::Function => "function",
            Self::Rng => "rng",
            Self::Stream => "stream",
        }
    }
}

impl fmt::Display for ValueCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A value's representation-oriented kind for debugging and tooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ValueKind {
    Int,
    BigInt,
    Float,
    Complex,
    Fraction,
    Algebraic,
    Char,
    Tag,
    Bool,
    IntList,
    FloatList,
    BoolList,
    List,
    String,
    Cas,
    Dict,
    Function,
    Closure,
    BuiltinFunction,
    FunctionComposition,
    Rng,
    Stream,
}

impl ValueKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Int => "int",
            Self::BigInt => "bigint",
            Self::Float => "float",
            Self::Complex => "complex",
            Self::Fraction => "fraction",
            Self::Algebraic => "algebraic",
            Self::Char => "char",
            Self::Tag => "tag",
            Self::Bool => "bool",
            Self::IntList => "int-list",
            Self::FloatList => "float-list",
            Self::BoolList => "bool-list",
            Self::List => "list",
            Self::String => "string",
            Self::Cas => "cas",
            Self::Dict => "dict",
            Self::Function => "function",
            Self::Closure => "closure",
            Self::BuiltinFunction => "builtin-function",
            Self::FunctionComposition => "function-composition",
            Self::Rng => "rng",
            Self::Stream => "stream",
        }
    }
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    BigInt(Arc<BigInt>),
    Float(OrderedFloat<f64>),
    Complex(Complex64),
    Fraction(Arc<Ratio<BigInt>>),
    Algebraic(Arc<algebraic::AlgebraicData>),
    Char(char),
    Tag(Arc<str>),
    Bool(bool),
    IntList(Arc<Vec<i64>>),
    IntRange(Arc<seq::IntRangeData>),
    FloatList(Arc<Vec<OrderedFloat<f64>>>),
    BoolList(Arc<Vec<bool>>),
    List(Arc<Vec<Value>>),
    /// Heap-allocated string with copy-on-write mutation support.
    String(Arc<String>),
    /// Symbolic algebra expression.
    Cas(Arc<CasData>),
    Dict(Arc<IndexMap<Arc<str>, Value>>),
    CompiledFunction(Arc<FunctionData>),
    /// closure with captured cells (upvalues)
    Closure(Arc<ClosureData>),
    BuiltinFunction {
        name: Arc<str>,
        id: u16,
    },
    LiftedCallable(Arc<LiftedCallableData>),
    Rng(Arc<Mutex<RngState>>),
    Stream(Arc<Mutex<StreamHandle>>),
}

impl Value {
    /// Get the length of a value
    pub fn len(&self) -> usize {
        if let Some(seq) = seq::ValueSeq::from_value(self) {
            return seq.len();
        }

        match self {
            Value::Dict(map) => map.len(),

            _ => 1, // Atoms have length 1
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn empty_list() -> Self {
        Value::List(Arc::new(vec![]))
    }

    pub(crate) fn builtin_function(name: impl Into<Arc<str>>, id: u16) -> Self {
        Value::BuiltinFunction {
            name: name.into(),
            id,
        }
    }

    /// Convenience constructor for `Value::Float`.
    #[inline]
    pub(crate) fn float(f: impl Into<f64>) -> Self {
        Value::Float(OrderedFloat(f.into()))
    }

    pub(crate) fn rng(seed: i64) -> Self {
        Value::Rng(Arc::new(Mutex::new(RngState::from_seed(seed))))
    }

    /// Create a new stream value
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn stream(handle: StreamHandle) -> Self {
        Value::Stream(Arc::new(Mutex::new(handle)))
    }

    pub fn is_atom(&self) -> bool {
        matches!(
            self,
            Value::Int(_)
                | Value::BigInt(_)
                | Value::Float(_)
                | Value::Complex(_)
                | Value::Fraction(_)
                | Value::Algebraic(_)
                | Value::Char(_)
                | Value::Tag(_)
                | Value::Bool(_)
                | Value::Cas(_)
                | Value::CompiledFunction(_)
                | Value::Closure(_)
                | Value::BuiltinFunction { .. }
                | Value::LiftedCallable(_)
                | Value::Rng(_)
                | Value::Stream(_)
        )
    }

    pub fn is_list(&self) -> bool {
        matches!(
            self,
            Value::IntList(_)
                | Value::IntRange(_)
                | Value::FloatList(_)
                | Value::BoolList(_)
                | Value::List(_)
                | Value::String(_)
        )
    }

    pub fn is_unit(&self) -> bool {
        self.is_list() && self.is_empty()
    }

    pub(crate) fn is_string(&self) -> bool {
        matches!(self, Value::String(_) | Value::Char(_))
            || self.is_unit()
            || matches!(self, Value::List(items) if items.iter().all(|v| matches!(v, Value::Char(_))))
    }

    pub fn is_dict(&self) -> bool {
        matches!(self, Value::Dict(_))
    }

    pub(crate) fn is_container(&self) -> bool {
        self.is_list() || self.is_dict()
    }

    pub(crate) fn is_runtime_callable(&self) -> bool {
        matches!(
            self,
            Value::CompiledFunction(_)
                | Value::Closure(_)
                | Value::BuiltinFunction { .. }
                | Value::LiftedCallable(_)
                | Value::Rng(_)
        )
    }

    pub(crate) fn is_callable(&self) -> bool {
        self.is_runtime_callable() || self.is_cas_expr()
    }

    pub(crate) fn function_composition(op: BinaryOperator, left: Value, right: Value) -> Self {
        Value::LiftedCallable(Arc::new(LiftedCallableData {
            expr: CallableExpr::binary(op, left, right),
            dbg_provenance: None,
        }))
    }

    pub(crate) fn unary_function_composition(op: UnaryOperator, operand: Value) -> Self {
        Value::LiftedCallable(Arc::new(LiftedCallableData {
            expr: CallableExpr::unary(op, operand),
            dbg_provenance: None,
        }))
    }

    pub(crate) fn lift_callable_binary(
        op: BinaryOperator,
        left: &Value,
        right: &Value,
    ) -> Option<Self> {
        if !matches!(
            op,
            BinaryOperator::Add
                | BinaryOperator::Subtract
                | BinaryOperator::Multiply
                | BinaryOperator::Power
                | BinaryOperator::PowerDot
                | BinaryOperator::Divide
                | BinaryOperator::DivideDot
                | BinaryOperator::Modulo
                | BinaryOperator::Matmul
                | BinaryOperator::BitAnd
                | BinaryOperator::BitOr
                | BinaryOperator::BitXor
                | BinaryOperator::Shl
                | BinaryOperator::Shr
                | BinaryOperator::FloorDiv
        ) {
            return None;
        }

        if left.is_runtime_callable() || right.is_runtime_callable() {
            Some(Self::function_composition(op, left.clone(), right.clone()))
        } else {
            None
        }
    }

    pub(crate) fn lift_callable_unary(op: UnaryOperator, operand: &Value) -> Option<Self> {
        if !matches!(op, UnaryOperator::Negate | UnaryOperator::Not) {
            return None;
        }

        if operand.is_runtime_callable() {
            Some(Self::unary_function_composition(op, operand.clone()))
        } else {
            None
        }
    }

    /// Return the stable, user-facing category of this value.
    pub fn category(&self) -> ValueCategory {
        match self {
            Value::Int(_) | Value::BigInt(_) => ValueCategory::Int,
            Value::Float(_) => ValueCategory::Float,
            Value::Complex(_) => ValueCategory::Complex,
            Value::Fraction(_) => ValueCategory::Fraction,
            Value::Algebraic(_) => ValueCategory::Algebraic,
            Value::Char(_) => ValueCategory::Char,
            Value::Tag(_) => ValueCategory::Tag,
            Value::Bool(_) => ValueCategory::Bool,
            Value::IntRange(_)
            | Value::IntList(_)
            | Value::FloatList(_)
            | Value::BoolList(_)
            | Value::List(_)
            | Value::String(_) => ValueCategory::List,
            Value::Cas(_) => ValueCategory::Cas,
            Value::Dict(_) => ValueCategory::Dict,
            Value::CompiledFunction(_)
            | Value::Closure(_)
            | Value::BuiltinFunction { .. }
            | Value::LiftedCallable(_) => ValueCategory::Function,
            Value::Rng(_) => ValueCategory::Rng,
            Value::Stream(_) => ValueCategory::Stream,
        }
    }

    /// Return the representation-oriented kind used by debugging tools.
    pub fn debug_kind(&self) -> ValueKind {
        match self {
            Value::Int(_) => ValueKind::Int,
            Value::BigInt(_) => ValueKind::BigInt,
            Value::Float(_) => ValueKind::Float,
            Value::Complex(_) => ValueKind::Complex,
            Value::Fraction(_) => ValueKind::Fraction,
            Value::Algebraic(_) => ValueKind::Algebraic,
            Value::Char(_) => ValueKind::Char,
            Value::Tag(_) => ValueKind::Tag,
            Value::Bool(_) => ValueKind::Bool,
            Value::IntRange(_) | Value::IntList(_) => ValueKind::IntList,
            Value::FloatList(_) => ValueKind::FloatList,
            Value::BoolList(_) => ValueKind::BoolList,
            Value::List(_) => ValueKind::List,
            Value::String(_) => ValueKind::String,
            Value::Cas(_) => ValueKind::Cas,
            Value::Dict(_) => ValueKind::Dict,
            Value::CompiledFunction(_) => ValueKind::Function,
            Value::Closure(_) => ValueKind::Closure,
            Value::BuiltinFunction { .. } => ValueKind::BuiltinFunction,
            Value::LiftedCallable(_) => ValueKind::FunctionComposition,
            Value::Rng(_) => ValueKind::Rng,
            Value::Stream(_) => ValueKind::Stream,
        }
    }
}

#[cfg(test)]
mod tests {

    use std::hash::{Hash, Hasher};

    use indexmap::indexmap;

    use super::*;
    use crate::ast::BoolOperator;

    fn test_builtin(name: &str, id: u16) -> Value {
        Value::builtin_function(name, id)
    }

    fn test_function() -> Value {
        Value::CompiledFunction(Arc::new(FunctionData {
            params: None,
            named_params: None,
            locals: 0,
            isolated_module: false,
            instructions: Arc::from(Vec::new()),
            dbg_chunk: None,
            dbg_stmt_spans: None,
            dbg_source_base_offset: 0,
            dbg_pc_spans: None,
            dbg_stmt_marks: None,
            dbg_local_names: None,
            dbg_provenance: None,
        }))
    }

    fn test_closure() -> Value {
        Value::Closure(Arc::new(ClosureData {
            params: None,
            named_params: None,
            locals: 0,
            isolated_module: false,
            captured: cell::empty_cells(),
            instructions: Arc::from(Vec::new()),
            dbg_chunk: None,
            dbg_stmt_spans: None,
            dbg_source_base_offset: 0,
            dbg_pc_spans: None,
            dbg_stmt_marks: None,
            dbg_local_names: None,
            dbg_provenance: None,
        }))
    }

    fn hash_value(value: &Value) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn category_names_are_stable() {
        let cases = [
            (ValueCategory::Int, "int"),
            (ValueCategory::Float, "float"),
            (ValueCategory::Complex, "complex"),
            (ValueCategory::Fraction, "fraction"),
            (ValueCategory::Algebraic, "algebraic"),
            (ValueCategory::Char, "char"),
            (ValueCategory::Tag, "tag"),
            (ValueCategory::Bool, "bool"),
            (ValueCategory::List, "list"),
            (ValueCategory::Cas, "cas"),
            (ValueCategory::Dict, "dict"),
            (ValueCategory::Function, "function"),
            (ValueCategory::Rng, "rng"),
            (ValueCategory::Stream, "stream"),
        ];

        for (category, expected) in cases {
            assert_eq!(category.as_str(), expected);
            assert_eq!(category.to_string(), expected);
        }
    }

    #[test]
    fn debug_kind_names_are_stable() {
        let cases = [
            (ValueKind::Int, "int"),
            (ValueKind::BigInt, "bigint"),
            (ValueKind::Float, "float"),
            (ValueKind::Complex, "complex"),
            (ValueKind::Fraction, "fraction"),
            (ValueKind::Algebraic, "algebraic"),
            (ValueKind::Char, "char"),
            (ValueKind::Tag, "tag"),
            (ValueKind::Bool, "bool"),
            (ValueKind::IntList, "int-list"),
            (ValueKind::FloatList, "float-list"),
            (ValueKind::BoolList, "bool-list"),
            (ValueKind::List, "list"),
            (ValueKind::String, "string"),
            (ValueKind::Cas, "cas"),
            (ValueKind::Dict, "dict"),
            (ValueKind::Function, "function"),
            (ValueKind::Closure, "closure"),
            (ValueKind::BuiltinFunction, "builtin-function"),
            (ValueKind::FunctionComposition, "function-composition"),
            (ValueKind::Rng, "rng"),
            (ValueKind::Stream, "stream"),
        ];

        for (kind, expected) in cases {
            assert_eq!(kind.as_str(), expected);
            assert_eq!(kind.to_string(), expected);
        }
    }

    #[test]
    fn builtin_function_display_uses_formal_name() {
        assert_eq!(test_builtin("f", 1).to_string(), "f");
    }

    #[test]
    fn opaque_functions_display_as_comments() {
        assert_eq!(test_function().to_string(), "{...}");
        assert_eq!(test_closure().to_string(), "{...}");
        let lifted =
            Value::function_composition(BinaryOperator::Subtract, test_function(), Value::Int(5));
        assert_eq!(lifted.to_string(), "{...} - 5");
    }

    #[test]
    fn categories_hide_storage_details() {
        let int_values = [Value::Int(1), Value::BigInt(Arc::new(BigInt::from(1)))];
        for value in int_values {
            assert_eq!(value.category(), ValueCategory::Int);
        }

        let list_values = [
            Value::IntList(Arc::new(vec![1])),
            Value::IntRange(Arc::new(seq::IntRangeData::new(1, 1, 1))),
            Value::FloatList(Arc::new(vec![OrderedFloat(1.0)])),
            Value::BoolList(Arc::new(vec![true])),
            Value::List(Arc::new(vec![Value::Int(1)])),
            into_wq_string("one"),
        ];
        for value in list_values {
            assert_eq!(value.category(), ValueCategory::List);
        }

        let function_values = [
            test_function(),
            test_closure(),
            test_builtin("f", 1),
            Value::function_composition(BinaryOperator::Add, test_builtin("f", 1), Value::Int(1)),
        ];
        for value in function_values {
            assert_eq!(value.category(), ValueCategory::Function);
        }
    }

    #[test]
    fn debug_kinds_expose_relevant_storage_details() {
        let cases = [
            (Value::Int(1), ValueKind::Int),
            (Value::BigInt(Arc::new(BigInt::from(1))), ValueKind::BigInt),
            (Value::IntList(Arc::new(vec![1])), ValueKind::IntList),
            (
                Value::IntRange(Arc::new(seq::IntRangeData::new(1, 1, 1))),
                ValueKind::IntList,
            ),
            (
                Value::FloatList(Arc::new(vec![OrderedFloat(1.0)])),
                ValueKind::FloatList,
            ),
            (Value::BoolList(Arc::new(vec![true])), ValueKind::BoolList),
            (into_wq_string("one"), ValueKind::String),
            (test_function(), ValueKind::Function),
            (test_closure(), ValueKind::Closure),
            (test_builtin("f", 1), ValueKind::BuiltinFunction),
            (
                Value::function_composition(
                    BinaryOperator::Add,
                    test_builtin("f", 1),
                    Value::Int(1),
                ),
                ValueKind::FunctionComposition,
            ),
        ];

        for (value, expected) in cases {
            assert_eq!(value.debug_kind(), expected);
        }
    }

    #[test]
    fn callable_expr_splices_existing_compositions() {
        let f = test_builtin("f", 1);
        let g = test_builtin("g", 2);
        let h = test_builtin("h", 3);
        let fg = Value::function_composition(BinaryOperator::Add, f, g);
        let nested = Value::function_composition(BinaryOperator::Multiply, fg, h.clone());

        let Value::LiftedCallable(data) = nested else {
            unreachable!("constructor returns a function composition");
        };
        let CallableExpr::Binary { left, right, .. } = &data.expr else {
            unreachable!("top-level expression is binary");
        };

        assert!(matches!(left.as_ref(), CallableExpr::Binary { .. }));
        assert!(matches!(right.as_ref(), CallableExpr::Call(value) if value == &h));
    }

    #[test]
    fn callable_expr_structural_equality_and_hash() {
        let first = Value::function_composition(
            BinaryOperator::Add,
            test_builtin("f", 1),
            Value::function_composition(
                BinaryOperator::Multiply,
                Value::Int(2),
                test_builtin("g", 2),
            ),
        );
        let second = Value::function_composition(
            BinaryOperator::Add,
            test_builtin("f", 1),
            Value::function_composition(
                BinaryOperator::Multiply,
                Value::Int(2),
                test_builtin("g", 2),
            ),
        );

        assert_eq!(first, second);
        assert_eq!(hash_value(&first), hash_value(&second));
    }

    #[test]
    fn callable_expr_normalizes_constant_only_subtrees() {
        let folded = Value::function_composition(BinaryOperator::Add, Value::Int(1), Value::Int(2));
        let Value::LiftedCallable(data) = &folded else {
            unreachable!("constructor returns a function composition");
        };
        assert!(matches!(&data.expr, CallableExpr::Const(Value::Int(3))));

        let nested =
            Value::function_composition(BinaryOperator::Multiply, test_builtin("f", 1), folded);
        assert_eq!(nested.to_string(), "f * 3");

        let negated = Value::unary_function_composition(UnaryOperator::Negate, nested);
        assert_eq!(negated.to_string(), "-(f * 3)");
    }

    #[test]
    fn callable_expr_keeps_failed_constant_folds_deferred() {
        let divided =
            Value::function_composition(BinaryOperator::Divide, Value::Int(1), Value::Int(0));
        let Value::LiftedCallable(data) = divided else {
            unreachable!("constructor returns a function composition");
        };
        assert!(matches!(
            &data.expr,
            CallableExpr::Binary {
                op: BinaryOperator::Divide,
                left,
                right,
            } if matches!(left.as_ref(), CallableExpr::Const(Value::Int(1)))
                && matches!(right.as_ref(), CallableExpr::Const(Value::Int(0)))
        ));
    }

    #[test]
    fn callable_expr_strict_normalization_keeps_call_dependent_ops() {
        let f = test_builtin("f", 1);

        let add_zero = Value::function_composition(BinaryOperator::Add, f.clone(), Value::Int(0));
        let mul_zero =
            Value::function_composition(BinaryOperator::Multiply, f.clone(), Value::Int(0));
        let sub_self = Value::function_composition(BinaryOperator::Subtract, f.clone(), f);

        assert_eq!(add_zero.to_string(), "f + 0");
        assert_eq!(mul_zero.to_string(), "f * 0");
        assert_eq!(sub_self.to_string(), "f - f");
    }

    #[test]
    fn callable_lifting_allows_only_v2_operator_set() {
        let f = test_builtin("f", 1);

        let neg = Value::lift_callable_unary(UnaryOperator::Negate, &f)
            .expect("negating callable should lift");
        let Value::LiftedCallable(data) = neg else {
            unreachable!("lift returns a function composition");
        };
        assert!(matches!(
            &data.expr,
            CallableExpr::Unary {
                op: UnaryOperator::Negate,
                ..
            }
        ));

        assert!(Value::lift_callable_unary(UnaryOperator::Not, &f).is_some());
        assert!(Value::lift_callable_unary(UnaryOperator::Count, &f).is_none());
        assert!(Value::lift_callable_binary(BinaryOperator::Add, &f, &Value::Int(1)).is_some());
        assert!(Value::lift_callable_binary(BinaryOperator::Equal, &f, &Value::Int(1)).is_none());
        assert!(Value::lift_callable_binary(BinaryOperator::Lt, &f, &Value::Int(1)).is_none());
        assert!(Value::lift_callable_binary(BinaryOperator::Cat, &f, &Value::Int(1)).is_none());
    }

    #[test]
    fn lifted_callable_display_shows_expression_tree() {
        let f = test_builtin("f", 1);
        let add_one = Value::function_composition(BinaryOperator::Add, f.clone(), Value::Int(1));
        let times_two =
            Value::function_composition(BinaryOperator::Multiply, add_one.clone(), Value::Int(2));
        let minus_three =
            Value::function_composition(BinaryOperator::Subtract, times_two, Value::Int(3));
        let negated = Value::unary_function_composition(UnaryOperator::Negate, add_one);

        assert_eq!(minus_three.to_string(), "((f + 1) * 2) - 3");
        assert_eq!(negated.to_string(), "-(f + 1)");
    }

    #[test]
    fn test_arithmetic() {
        let a = Value::Int(5);
        let b = Value::Int(3);

        assert_eq!(a.add(&b), Ok(Value::Int(8)));
        assert_eq!(a.subtract(&b), Ok(Value::Int(2)));
        assert_eq!(a.multiply(&b), Ok(Value::Int(15)));
        assert_eq!(a.divide(&b), Ok(Value::float(5.0 / 3.0)));
        assert_eq!(a.modulo(&b), Ok(Value::Int(2)));
    }

    #[test]
    fn test_list_operations() {
        let list = Value::List(Arc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));

        assert_eq!(list.len(), 3);
        assert_eq!(list.index(&Value::Int(0)), Some(Value::Int(1)));
        assert_eq!(list.index(&Value::Int(-1)), Some(Value::Int(3)));

        assert_eq!(
            Value::Bool(true).cat(Value::Bool(false)),
            Value::BoolList(Arc::new(vec![true, false]))
        );

        let empty_bools = Value::BoolList(Arc::new(vec![]));
        assert!(empty_bools.is_unit());
        assert_eq!(
            empty_bools.try_flatten_to_rust_string(),
            Some(String::new())
        );
    }

    #[test]
    fn atom_checks_follow_storage_shape() {
        assert!(Value::Int(1).is_atom());
        assert!(Value::Float(OrderedFloat(1.5)).is_atom());
        assert!(Value::Bool(true).is_atom());

        assert!(!Value::IntList(Arc::new(vec![1])).is_atom());
        assert!(!Value::IntRange(Arc::new(seq::IntRangeData::new(0, 1, 1))).is_atom());
        assert!(!Value::FloatList(Arc::new(vec![OrderedFloat(1.5)])).is_atom());
        assert!(!Value::BoolList(Arc::new(vec![true])).is_atom());
        assert!(!Value::List(Arc::new(vec![Value::Int(1)])).is_atom());
        assert!(!into_wq_string("x").is_atom());

        let map = indexmap! {
            Arc::from("x") => Value::Int(1),
        };
        assert!(!Value::Dict(Arc::new(map)).is_atom());
    }

    #[test]
    fn test_set_index() {
        let mut list = Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)]));
        assert_eq!(
            list.assign_by_index(&Value::Int(1), Value::Int(5)),
            Some(())
        );
        assert_eq!(list.index(&Value::Int(1)), Some(Value::Int(5)));
        assert_eq!(list.assign_by_index(&Value::Int(5), Value::Int(0)), None);

        let mut map = IndexMap::new();
        map.insert("a".into(), Value::Int(1));
        let mut dict = Value::Dict(Arc::new(map));
        assert_eq!(
            dict.assign_by_index(&Value::Tag("a".into()), Value::Int(2)),
            Some(())
        );
        assert_eq!(dict.index(&Value::Tag("a".into())), Some(Value::Int(2)));
        assert_eq!(
            dict.assign_by_index(&Value::Tag("b".into()), Value::Int(3)),
            Some(())
        );
    }

    #[test]
    fn test_vectorized_comparisons() {
        let list = Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)]));
        assert_eq!(
            list.gt(&Value::Int(1)),
            Ok(Value::List(Arc::new(vec![
                Value::Bool(false),
                Value::Bool(true)
            ])))
        );

        let atom = Value::Int(1);
        let vec = Value::List(Arc::new(vec![Value::Int(1), Value::Int(1)]));
        assert_eq!(
            atom.eq_bc(&vec),
            Ok(Value::List(Arc::new(vec![
                Value::Bool(true),
                Value::Bool(true)
            ])))
        );

        let a = Value::List(Arc::new(vec![
            Value::List(Arc::new(vec![Value::Int(1), Value::Int(3)])),
            Value::Int(2),
        ]));
        let b = Value::List(Arc::new(vec![
            Value::List(Arc::new(vec![Value::Int(1), Value::Int(4)])),
            Value::Int(2),
        ]));
        assert_eq!(
            a.eq_bc(&b),
            Ok(Value::List(Arc::new(vec![
                Value::List(Arc::new(vec![Value::Bool(true), Value::Bool(false)])),
                Value::Bool(true),
            ])))
        );

        let c = Value::List(Arc::new(vec![Value::Int(1)]));
        let d = Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)]));
        assert!(c.eq_bc(&d).is_err());

        let str_a = Value::List(Arc::new(vec![Value::Char('a'), Value::Char('b')]));
        let str_b = Value::List(Arc::new(vec![Value::Char('a'), Value::Char('c')]));
        assert_eq!(
            str_a.eq_bc(&str_b),
            Ok(Value::List(Arc::new(vec![
                Value::Bool(true),
                Value::Bool(false)
            ])))
        );
    }

    // #[test]
    // fn test_bool_equals() {
    //     assert_eq!(
    //         Value::Bool(true).eq(&Value::Bool(true)),
    //         Ok(Value::Bool(true))
    //     );
    //     assert_eq!(
    //         Value::Bool(true).eq(&Value::Bool(false)),
    //         Ok(Value::Bool(false))
    //     );
    //     assert_eq!(
    //         Value::Bool(false).eq(&Value::Bool(false)),
    //         Ok(Value::Bool(true))
    //     );
    // }

    #[test]
    fn test_vectorized_logical_ops() {
        let a = Value::Bool(true);
        let b = Value::List(Arc::new(vec![Value::Bool(true), Value::Bool(false)]));
        assert_eq!(
            a.bool_and(&b),
            Ok(Value::List(Arc::new(vec![
                Value::Bool(true),
                Value::Bool(false)
            ])))
        );

        assert_eq!(
            eval_bool_op(BoolOperator::Or, &b, &Value::Bool(false)),
            Ok(Value::List(Arc::new(vec![
                Value::Bool(true),
                Value::Bool(false)
            ])))
        );

        let c = Value::List(Arc::new(vec![Value::Bool(true)]));
        let d = Value::List(Arc::new(vec![Value::Bool(false), Value::Bool(true)]));
        assert!(eval_binary(&BinaryOperator::BitXor, &c, &d).is_err());

        assert_eq!(
            eval_unary(&crate::ast::UnaryOperator::Not, &d),
            Ok(Value::List(Arc::new(vec![
                Value::Bool(true),
                Value::Bool(false)
            ])))
        );
    }

    #[test]
    fn test_vectorized_modulo() {
        let a = Value::List(Arc::new(vec![Value::Int(5), Value::Int(10)]));
        let b = Value::Int(3);
        assert_eq!(
            a.modulo(&b),
            Ok(Value::List(Arc::new(vec![Value::Int(2), Value::Int(1)])))
        );

        let c = Value::List(Arc::new(vec![Value::Int(5)]));
        let d = Value::List(Arc::new(vec![Value::Int(2), Value::Int(3)]));
        assert!(c.modulo(&d).is_err());
    }

    #[test]
    fn test_bitwise_ops() {
        let a = Value::Int(6);
        let b = Value::Int(3);
        assert_eq!(a.band(&b), Ok(Value::Int(2)));
        assert_eq!(a.bor(&b), Ok(Value::Int(7)));
        assert_eq!(a.bxor(&b), Ok(Value::Int(5)));
        assert_eq!(a.not(), Ok(Value::Int(!6)));
        assert_eq!(Value::Int(1).shl(&Value::Int(3)), Ok(Value::Int(8)));
        assert_eq!(Value::Int(8).shr(&Value::Int(2)), Ok(Value::Int(2)));
        let arr = Value::IntList(Arc::new(vec![1, 2, 3]));
        let res = arr.bor(&Value::Int(1));
        assert_eq!(res, Ok(Value::IntList(Arc::new(vec![1 | 1, 2 | 1, 3 | 1]))));
    }

    #[test]
    fn test_complex_arithmetic() {
        let z1 = Value::from_complex64(num_complex::Complex64::new(1.0, 2.0));
        let z2 = Value::from_complex64(num_complex::Complex64::new(3.0, 4.0));

        assert_eq!(
            z1.add(&z2),
            Ok(Value::from_complex64(num_complex::Complex64::new(4.0, 6.0)))
        );
        assert_eq!(
            z1.multiply(&z2),
            Ok(Value::from_complex64(num_complex::Complex64::new(
                -5.0, 10.0
            )))
        );
        let pow = z1.power(&Value::Int(2)).unwrap();
        let pow = pow.as_complex64().unwrap();
        assert!((pow.re + 3.0).abs() < 1e-12);
        assert!((pow.im - 4.0).abs() < 1e-12);
    }

    #[test]
    fn test_complex_broadcasting_stops_at_re_im_dict() {
        let xs = Value::List(Arc::new(vec![Value::from_complex64(
            num_complex::Complex64::new(1.0, 2.0),
        )]));
        assert_eq!(
            xs.multiply(&Value::Int(3)),
            Ok(Value::List(Arc::new(vec![Value::from_complex64(
                num_complex::Complex64::new(3.0, 6.0)
            )])))
        );
    }

    #[test]
    fn test_complex_display_uses_a_plus_bi_form() {
        let i_text = "i";
        assert_eq!(
            Value::from_complex64(num_complex::Complex64::new(1.0, 2.0)).to_string(),
            format!("1+2{i_text}")
        );
        assert_eq!(
            Value::from_complex64(num_complex::Complex64::new(0.0, -1.0)).to_string(),
            format!("-1{i_text}")
        );
        assert_eq!(
            Value::from_complex64(num_complex::Complex64::new(3.5, -2.0)).to_string(),
            format!("3.5-2{i_text}")
        );
    }

    #[test]
    fn test_dict_multi_index() {
        let mut map = IndexMap::new();
        map.insert("a".into(), Value::Int(1));
        map.insert("b".into(), Value::Int(2));
        let dict = Value::Dict(Arc::new(map));
        let keys = Value::List(Arc::new(vec![
            Value::Tag("b".into()),
            Value::Tag("a".into()),
        ]));
        assert_eq!(
            dict.index(&keys),
            Some(Value::List(Arc::new(vec![Value::Int(2), Value::Int(1)])))
        );
    }

    #[test]
    fn test_dict_integer_index_and_assign() {
        let mut map = IndexMap::new();
        map.insert("a".into(), Value::Int(1));
        map.insert("b".into(), Value::Int(2));
        map.insert("c".into(), Value::Int(3));
        let mut dict = Value::Dict(Arc::new(map));

        assert_eq!(dict.index(&Value::Int(1)), Some(Value::Int(2)));
        assert_eq!(dict.index(&Value::Int(-1)), Some(Value::Int(3)));
        assert_eq!(
            dict.index(&Value::IntList(Arc::new(vec![0, 2]))),
            Some(Value::List(Arc::new(vec![Value::Int(1), Value::Int(3)])))
        );
        assert_eq!(
            dict.index(&Value::List(Arc::new(vec![
                Value::Int(1),
                Value::Tag("a".into()),
            ]))),
            Some(Value::List(Arc::new(vec![Value::Int(2), Value::Int(1)])))
        );

        assert_eq!(
            dict.assign_by_index(&Value::Int(1), Value::Int(99)),
            Some(())
        );
        assert_eq!(dict.index(&Value::Int(1)), Some(Value::Int(99)));
        assert_eq!(dict.assign_by_index(&Value::Int(10), Value::Int(0)), None);
    }

    #[test]
    fn test_intlist_multi_index() {
        let arr = Value::IntList(Arc::new(vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]));
        let idxs = Value::IntList(Arc::new(vec![2, 4]));
        assert_eq!(arr.index(&idxs), Some(Value::IntList(Arc::new(vec![2, 4]))));
    }

    #[test]
    fn test_intlist_list_arith_and_cmp() {
        let arr = Value::IntList(Arc::new(vec![1, 2, 3]));
        let list = Value::List(Arc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
        assert_eq!(
            arr.add(&list),
            Ok(Value::List(Arc::new(vec![
                Value::Int(2),
                Value::Int(4),
                Value::Int(6)
            ])))
        );
        assert_eq!(
            list.add(&arr),
            Ok(Value::List(Arc::new(vec![
                Value::Int(2),
                Value::Int(4),
                Value::Int(6)
            ])))
        );
    }

    // String variant tests

    #[test]
    fn string_construction_via_into_wq_str() {
        let v = into_wq_string("hello");
        assert!(matches!(v, Value::String(_)));
        assert_eq!(v.to_string(), "\"hello\"");
    }

    #[test]
    fn string_len() {
        assert_eq!(into_wq_string("").len(), 0);
        assert_eq!(into_wq_string("abc").len(), 3);
        assert_eq!(into_wq_string("🦀🚀").len(), 2);
    }

    #[test]
    fn string_is_not_atom() {
        assert!(!into_wq_string("hello").is_atom());
        assert!(!into_wq_string("").is_atom());
    }

    #[test]
    fn string_category() {
        assert_eq!(into_wq_string("hello").category(), ValueCategory::List);
        assert_eq!(into_wq_string("").category(), ValueCategory::List);
    }

    #[test]
    fn string_is_string_like() {
        assert!(into_wq_string("hello").is_string());
        assert!(into_wq_string("").is_string());
    }

    #[test]
    fn string_try_to_rust_string() {
        assert_eq!(
            into_wq_string("hello").try_to_rust_string().unwrap(),
            "hello".to_string()
        );
        assert_eq!(
            into_wq_string("").try_to_rust_string().unwrap(),
            "".to_string()
        );
    }

    #[test]
    fn string_display_quotes_and_escapes() {
        assert_eq!(into_wq_string("hello").to_string(), "\"hello\"");
        assert_eq!(into_wq_string("a\"b").to_string(), "\"a\\\"b\"");
        assert_eq!(into_wq_string("a").to_string(), ",\"a\"");
        assert_eq!(into_wq_string("").to_string(), "()");
    }

    #[test]
    fn char_display_uses_single_scalar_quotes() {
        assert_eq!(Value::Char('a').to_string(), "\"a\"");
        assert_eq!(Value::Char('\n').to_string(), "\"\\n\"");
    }

    #[test]
    fn nested_singleton_string_display_is_unambiguous() {
        let one_item_string = into_wq_string("a");
        let outer = Value::List(Arc::new(vec![one_item_string]));
        assert_eq!(outer.to_string(), ",(,\"a\")");
    }

    #[test]
    fn string_partial_eq() {
        let a = into_wq_string("hello");
        let b = into_wq_string("hello");
        let c = into_wq_string("world");
        assert_eq!(a, b);
        assert_ne!(a, c);
        // String and char-list are cross-equal (same user-facing value)
        let list = Value::List(Arc::new("hello".chars().map(Value::Char).collect()));
        assert_eq!(a, list);
    }

    #[test]
    fn string_hash_consistent() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let a = into_wq_string("hello");
        let b = into_wq_string("hello");
        let mut ha = DefaultHasher::new();
        let mut hb = DefaultHasher::new();
        a.hash(&mut ha);
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    #[test]
    fn string_and_list_char_hash_same() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let s = into_wq_string("hello");
        let list = Value::List(Arc::new("hello".chars().map(Value::Char).collect()));
        let mut hs = DefaultHasher::new();
        let mut hl = DefaultHasher::new();
        s.hash(&mut hs);
        list.hash(&mut hl);
        assert_eq!(hs.finish(), hl.finish());
    }

    #[test]
    fn string_into_wq_value_trait() {
        let v: Value = "hello".into_wq_value();
        assert!(matches!(v, Value::String(_)));
        assert_eq!(v.to_string(), "\"hello\"");

        let v2: Value = String::from("world").into_wq_value();
        assert!(matches!(v2, Value::String(_)));
    }

    #[test]
    fn string_backward_compat_list_char_still_works() {
        // An old-style char-list still works as a string via fallback paths
        let old_style = Value::List(Arc::new("hi".chars().map(Value::Char).collect()));
        assert!(old_style.is_string());
        assert_eq!(old_style.try_to_rust_string().unwrap(), "hi".to_string());
        assert_eq!(old_style.to_string(), "\"hi\"");
    }

    #[test]
    fn string_cat_two_strings() {
        let a = into_wq_string("hello");
        let b = into_wq_string("world");
        let result = a.cat(b);
        assert!(matches!(result, Value::String(_)));
        assert_eq!(result.to_string(), "\"helloworld\"");
    }

    #[test]
    fn cat_reuses_unique_packed_storage() {
        let ints = Arc::new(Vec::with_capacity(4));
        let int_ptr = Arc::as_ptr(&ints);
        let ints = Value::IntList(ints).cat(Value::Int(1));
        let Value::IntList(ints) = ints else {
            unreachable!("expected packed int list");
        };
        assert_eq!(Arc::as_ptr(&ints), int_ptr);

        let string = Arc::new(String::with_capacity(4));
        let string_ptr = Arc::as_ptr(&string);
        let string = Value::String(string).cat(Value::Char('a'));
        let Value::String(string) = string else {
            unreachable!("expected string");
        };
        assert_eq!(Arc::as_ptr(&string), string_ptr);
    }

    #[test]
    fn cat_copies_shared_packed_storage() {
        let ints = Arc::new(vec![1, 2]);
        let alias = ints.clone();
        let ints = Value::IntList(ints).cat(Value::Int(3));
        let Value::IntList(ints) = ints else {
            unreachable!("expected packed int list");
        };

        assert_ne!(Arc::as_ptr(&ints), Arc::as_ptr(&alias));
        assert_eq!(alias.as_slice(), [1, 2]);
        assert_eq!(ints.as_slice(), [1, 2, 3]);
    }

    #[test]
    fn string_cat_with_char() {
        let s = into_wq_string("hello");
        let c = Value::Char('!');
        let result = s.cat(c);
        assert!(matches!(result, Value::String(_)));
        assert_eq!(result.to_string(), "\"hello!\"");
    }

    #[test]
    fn char_cat_with_string() {
        let c = Value::Char('A');
        let s = into_wq_string("bc");
        let result = c.cat(s);
        assert!(matches!(result, Value::String(_)));
        assert_eq!(result.to_string(), "\"Abc\"");
    }

    #[test]
    fn string_cat_with_list_char() {
        let s = into_wq_string("hello");
        let list = Value::List(Arc::new(" world".chars().map(Value::Char).collect()));
        let result = s.cat(list);
        assert!(matches!(result, Value::String(_)));
        assert_eq!(result.to_string(), "\"hello world\"");
    }

    // Complex variant tests

    #[test]
    fn complex_construction() {
        let z = Value::from_complex64(num_complex::Complex64::new(3.0, 4.0));
        assert!(matches!(z, Value::Complex(_)));
        assert!(z.is_complex());
    }

    #[test]
    fn complex_is_atom() {
        let z = Value::from_complex64(num_complex::Complex64::new(1.0, 2.0));
        assert!(z.is_atom(), "Complex is an atom (no transparent broadcast)");
    }

    #[test]
    fn complex_category() {
        let z = Value::from_complex64(num_complex::Complex64::new(1.0, 0.0));
        assert_eq!(z.category(), ValueCategory::Complex);
    }

    #[test]
    fn complex_display() {
        let z = Value::from_complex64(num_complex::Complex64::new(3.0, 4.0));
        assert!(z.to_string().contains('3') && z.to_string().contains('4'));
        // Pure imaginary
        let zi = Value::from_complex64(num_complex::Complex64::new(0.0, 1.0));
        assert!(zi.to_string().contains('i'));
    }

    #[test]
    fn complex_as_complex64() {
        let z = Value::from_complex64(num_complex::Complex64::new(3.0, 4.0));
        let extracted = z.as_complex64().unwrap();
        assert_eq!(extracted.re, 3.0);
        assert_eq!(extracted.im, 4.0);
    }

    #[test]
    fn complex_partial_eq() {
        let a = Value::from_complex64(num_complex::Complex64::new(1.0, 2.0));
        let b = Value::from_complex64(num_complex::Complex64::new(1.0, 2.0));
        let c = Value::from_complex64(num_complex::Complex64::new(3.0, 4.0));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn complex_arithmetic() {
        let a = Value::from_complex64(num_complex::Complex64::new(1.0, 2.0));
        let b = Value::from_complex64(num_complex::Complex64::new(3.0, 4.0));
        let sum = a.add(&b).unwrap();
        let expected = Value::from_complex64(num_complex::Complex64::new(4.0, 6.0));
        assert_eq!(sum, expected);
    }

    // Fraction variant tests

    #[test]
    fn fraction_construction() {
        let f =
            Value::from_fraction_parts(num_bigint::BigInt::from(1), num_bigint::BigInt::from(2));
        assert!(matches!(f, Value::Fraction(_)));
        assert!(f.is_fraction());
    }

    #[test]
    fn fraction_is_atom() {
        let f =
            Value::from_fraction_parts(num_bigint::BigInt::from(1), num_bigint::BigInt::from(2));
        assert!(f.is_atom(), "Fraction should be an atom");
    }

    #[test]
    fn fraction_category() {
        let f =
            Value::from_fraction_parts(num_bigint::BigInt::from(1), num_bigint::BigInt::from(3));
        assert_eq!(f.category(), ValueCategory::Fraction);
    }

    #[test]
    fn fraction_display() {
        let f =
            Value::from_fraction_parts(num_bigint::BigInt::from(-1), num_bigint::BigInt::from(2));
        assert_eq!(f.to_string(), "-1/2");
    }

    #[test]
    fn fraction_normalizes_sign() {
        let f =
            Value::from_fraction_parts(num_bigint::BigInt::from(1), num_bigint::BigInt::from(-2));
        assert_eq!(f.to_string(), "-1/2");
    }

    #[test]
    fn fraction_partial_eq() {
        let a =
            Value::from_fraction_parts(num_bigint::BigInt::from(1), num_bigint::BigInt::from(2));
        let b =
            Value::from_fraction_parts(num_bigint::BigInt::from(1), num_bigint::BigInt::from(2));
        let c =
            Value::from_fraction_parts(num_bigint::BigInt::from(1), num_bigint::BigInt::from(3));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn fraction_as_f64() {
        let f =
            Value::from_fraction_parts(num_bigint::BigInt::from(1), num_bigint::BigInt::from(2));
        assert_eq!(f.as_f64(), Some(0.5));
    }

    #[test]
    fn fraction_neg() {
        let f =
            Value::from_fraction_parts(num_bigint::BigInt::from(-1), num_bigint::BigInt::from(2));
        let negated = f.neg().unwrap();
        assert_eq!(
            negated,
            Value::from_fraction_parts(num_bigint::BigInt::from(1), num_bigint::BigInt::from(2),)
        );
    }

    #[test]
    fn fraction_arithmetic_preserves_fraction() {
        let a =
            Value::from_fraction_parts(num_bigint::BigInt::from(1), num_bigint::BigInt::from(3));
        let b =
            Value::from_fraction_parts(num_bigint::BigInt::from(1), num_bigint::BigInt::from(3));
        let sum = a.add(&b).unwrap();
        assert!(sum.is_fraction());
        assert_eq!(
            sum,
            Value::from_fraction_parts(num_bigint::BigInt::from(2), num_bigint::BigInt::from(3),)
        );
    }
}
