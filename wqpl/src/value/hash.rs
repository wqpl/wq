use std::hash::Hash as _;
use std::sync::Arc;

use num_traits::ToPrimitive;
use ordered_float::OrderedFloat;

use crate::value::Value;
use crate::value::cas::CasKind;
use crate::value::func::CallableExpr;
use crate::value::seq::ValueSeq;

fn hash_callable_expr<H: std::hash::Hasher>(expr: &CallableExpr, state: &mut H) {
    match expr {
        CallableExpr::Const(value) => {
            0u8.hash(state);
            value.hash(state);
        }
        CallableExpr::Call(value) => {
            1u8.hash(state);
            value.hash(state);
        }
        CallableExpr::Unary { op, operand } => {
            2u8.hash(state);
            op.hash(state);
            hash_callable_expr(operand, state);
        }
        CallableExpr::Binary { op, left, right } => {
            3u8.hash(state);
            op.hash(state);
            hash_callable_expr(left, state);
            hash_callable_expr(right, state);
        }
    }
}

impl std::hash::Hash for Value {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // - Int and BigInt cross-equality
        // - Sequence representation cross-equality
        if let Some(seq) = ValueSeq::from_value(self) {
            6u8.hash(state);
            seq.len().hash(state);
            for item in seq.values() {
                item.hash(state);
            }
            return;
        }

        match self {
            Value::Int(n) => {
                0u8.hash(state);
                n.hash(state);
            }
            Value::BigInt(n) => {
                if let Some(i) = n.to_i64() {
                    0u8.hash(state);
                    i.hash(state);
                } else {
                    1u8.hash(state);
                    n.hash(state);
                }
            }
            Value::Float(f) => {
                2u8.hash(state);
                f.hash(state);
            }
            Value::Char(c) => {
                3u8.hash(state);
                c.hash(state);
            }
            Value::Tag(s) => {
                4u8.hash(state);
                s.hash(state);
            }
            Value::Bool(b) => {
                5u8.hash(state);
                b.hash(state);
            }
            Value::IntList(_)
            | Value::IntRange(_)
            | Value::FloatList(_)
            | Value::BoolList(_)
            | Value::List(_)
            | Value::String(_) => {
                unreachable!("sequence values are handled before atom hash dispatch")
            }
            Value::Dict(m) => {
                7u8.hash(state);
                m.len().hash(state);
                for (k, v) in m.iter() {
                    k.hash(state);
                    v.hash(state);
                }
            }
            Value::Complex(z) => {
                14u8.hash(state);
                OrderedFloat(z.re).hash(state);
                OrderedFloat(z.im).hash(state);
            }
            Value::Fraction(fd) => {
                15u8.hash(state);
                fd.numer().hash(state);
                fd.denom().hash(state);
            }

            Value::CompiledFunction(fd) => {
                8u8.hash(state);
                Arc::as_ptr(fd).hash(state);
            }
            Value::Closure(cd) => {
                9u8.hash(state);
                Arc::as_ptr(&cd.instructions).hash(state);
                cd.captured.len().hash(state);
                for cell in cd.captured.iter() {
                    Arc::as_ptr(cell).hash(state);
                }
            }
            Value::BuiltinFunction { id, .. } => {
                10u8.hash(state);
                id.hash(state);
            }
            Value::LiftedCallable(data) => {
                11u8.hash(state);
                hash_callable_expr(&data.expr, state);
            }
            Value::Cas(cd) => {
                18u8.hash(state);
                match &cd.kind {
                    CasKind::Var(name) => {
                        0u8.hash(state);
                        name.hash(state);
                    }
                    CasKind::Const(name) => {
                        4u8.hash(state);
                        name.hash(state);
                    }
                    CasKind::Op(op, args) => {
                        1u8.hash(state);
                        op.hash(state);
                        for arg in args.iter() {
                            arg.hash(state);
                        }
                    }
                    CasKind::Function(name, args) => {
                        2u8.hash(state);
                        name.hash(state);
                        for arg in args.iter() {
                            arg.hash(state);
                        }
                    }
                    CasKind::Apply(name, args) => {
                        6u8.hash(state);
                        name.hash(state);
                        for arg in args.iter() {
                            arg.hash(state);
                        }
                    }
                    CasKind::NamedArg(name, value) => {
                        7u8.hash(state);
                        name.hash(state);
                        value.hash(state);
                    }
                    CasKind::Limit {
                        expr,
                        var,
                        point,
                        direction,
                    } => {
                        5u8.hash(state);
                        expr.hash(state);
                        var.hash(state);
                        point.hash(state);
                        direction.hash(state);
                    }
                    CasKind::Root { poly, lo, hi } => {
                        8u8.hash(state);
                        poly.hash(state);
                        lo.to_bits().hash(state);
                        hi.to_bits().hash(state);
                    }
                    CasKind::Eq(lhs, rhs) => {
                        3u8.hash(state);
                        lhs.hash(state);
                        rhs.hash(state);
                    }
                    CasKind::Predicate(predicate) => {
                        9u8.hash(state);
                        use crate::value::cas::CasPredicate;
                        let discriminant = match predicate {
                            CasPredicate::Zero(_) => 0u8,
                            CasPredicate::NonZero(_) => 1,
                            CasPredicate::Positive(_) => 2,
                            CasPredicate::Negative(_) => 3,
                            CasPredicate::NonNegative(_) => 4,
                            CasPredicate::Real(_) => 5,
                            CasPredicate::Integer(_) => 6,
                        };
                        discriminant.hash(state);
                        predicate.expr().hash(state);
                    }
                }
            }
            Value::Stream(s) => {
                17u8.hash(state);
                Arc::as_ptr(s).hash(state);
            }
            Value::Rng(rng) => {
                21u8.hash(state);
                Arc::as_ptr(rng).hash(state);
            }
            Value::Algebraic(a) => {
                20u8.hash(state);
                a.field().hash(state);
                a.coeffs.len().hash(state);
                for c in a.coeffs.iter() {
                    c.hash(state);
                }
            }
        }
    }
}

impl Eq for Value {}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        if self.is_unit() && other.is_unit() {
            return true;
        }
        if let (Some(left), Some(right)) = (ValueSeq::from_value(self), ValueSeq::from_value(other))
        {
            return left.eq_values(&right);
        }

        use Value::*;
        match (self, other) {
            (Int(a), Int(b)) => a == b,
            (BigInt(a), BigInt(b)) => a == b,
            (Int(a), BigInt(b)) => num_bigint::BigInt::from(*a) == **b,
            (BigInt(a), Int(b)) => **a == num_bigint::BigInt::from(*b),
            (Float(a), Float(b)) => a == b,
            (Char(a), Char(b)) => a == b,
            (Tag(a), Tag(b)) => a == b,
            (Bool(a), Bool(b)) => a == b,

            (List(a), List(b)) => a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x == y),
            (IntList(a), IntList(b)) => a == b,
            (IntList(a), List(b)) | (List(b), IntList(a)) => {
                if a.len() != b.len() {
                    return false;
                }
                a.iter()
                    .zip(b.iter())
                    .all(|(x, y)| matches!(y, Int(n) if *n == *x))
            }
            (Dict(a), Dict(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .zip(b.iter())
                        .all(|((ak, av), (bk, bv))| ak == bk && av == bv)
            }

            (CompiledFunction(a), CompiledFunction(b)) => Arc::ptr_eq(a, b),
            (Closure(a), Closure(b)) => {
                if !Arc::ptr_eq(&a.instructions, &b.instructions)
                    || a.captured.len() != b.captured.len()
                {
                    return false;
                }
                a.captured
                    .iter()
                    .zip(b.captured.iter())
                    .all(|(lhs, rhs)| Arc::ptr_eq(lhs, rhs))
            }

            (Complex(a), Complex(b)) => {
                OrderedFloat(a.re) == OrderedFloat(b.re) && OrderedFloat(a.im) == OrderedFloat(b.im)
            }
            (Fraction(a), Fraction(b)) => a == b,

            (Cas(a), Cas(b)) => match (&a.kind, &b.kind) {
                (CasKind::Var(na), CasKind::Var(nb)) => na == nb,
                (CasKind::Const(na), CasKind::Const(nb)) => na == nb,
                (CasKind::Op(opa, arga), CasKind::Op(opb, argb)) => {
                    opa == opb
                        && arga.len() == argb.len()
                        && arga.iter().zip(argb.iter()).all(|(x, y)| x == y)
                }
                (CasKind::Function(na, arga), CasKind::Function(nb, argb)) => {
                    na == nb
                        && arga.len() == argb.len()
                        && arga.iter().zip(argb.iter()).all(|(x, y)| x == y)
                }
                (CasKind::Apply(na, arga), CasKind::Apply(nb, argb)) => {
                    na == nb
                        && arga.len() == argb.len()
                        && arga.iter().zip(argb.iter()).all(|(x, y)| x == y)
                }
                (CasKind::NamedArg(na, va), CasKind::NamedArg(nb, vb)) => na == nb && va == vb,
                (
                    CasKind::Limit {
                        expr: ea,
                        var: va,
                        point: pa,
                        direction: da,
                    },
                    CasKind::Limit {
                        expr: eb,
                        var: vb,
                        point: pb,
                        direction: db,
                    },
                ) => ea == eb && va == vb && pa == pb && da == db,
                (
                    CasKind::Root {
                        poly: polya,
                        lo: loa,
                        hi: hia,
                    },
                    CasKind::Root {
                        poly: polyb,
                        lo: lob,
                        hi: hib,
                    },
                ) => {
                    polya == polyb
                        && loa.to_bits() == lob.to_bits()
                        && hia.to_bits() == hib.to_bits()
                }
                (CasKind::Eq(lhsa, rhsa), CasKind::Eq(lhsb, rhsb)) => lhsa == lhsb && rhsa == rhsb,
                (CasKind::Predicate(a), CasKind::Predicate(b)) => a == b,
                _ => false,
            },

            (String(a), String(b)) => a == b,
            (String(a), List(b)) | (List(b), String(a)) => {
                a.chars().count() == b.len()
                    && a.chars()
                        .zip(b.iter())
                        .all(|(c, v)| matches!(v, Char(ch) if *ch == c))
            }

            (BuiltinFunction { id: a, .. }, BuiltinFunction { id: b, .. }) => a == b,

            (LiftedCallable(a), LiftedCallable(b)) => a.expr == b.expr,

            (Stream(a), Stream(b)) => Arc::ptr_eq(a, b),

            (Rng(a), Rng(b)) => Arc::ptr_eq(a, b),

            (Algebraic(a), Algebraic(b)) => {
                a.field() == b.field()
                    && a.coeffs.len() == b.coeffs.len()
                    && a.coeffs.iter().zip(b.coeffs.iter()).all(|(x, y)| x == y)
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod hash_tests {

    use std::hash::{Hash, Hasher};

    use indexmap::IndexMap;
    use num_bigint::BigInt;
    use num_complex::Complex64;

    use super::*;
    use crate::value::cas::CasFunction;

    fn hash_value(v: &Value) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        v.hash(&mut hasher);
        hasher.finish()
    }

    fn assert_equal_values_hash_equal(lhs: Value, rhs: Value) {
        assert_eq!(lhs, rhs);
        assert_eq!(hash_value(&lhs), hash_value(&rhs));
    }

    #[test]
    fn equal_values_hash_equal_across_representations() {
        assert_equal_values_hash_equal(Value::Int(42), Value::BigInt(Arc::new(BigInt::from(42))));
        assert_equal_values_hash_equal(
            Value::IntList(Arc::new(vec![1, 2, 3])),
            Value::List(Arc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)])),
        );
        assert_equal_values_hash_equal(
            Value::IntRange(Arc::new(crate::value::seq::IntRangeData::new(1, 1, 3))),
            Value::IntList(Arc::new(vec![1, 2, 3])),
        );
        assert_equal_values_hash_equal(
            Value::String(Arc::new("abc".to_string())),
            Value::List(Arc::new(vec![
                Value::Char('a'),
                Value::Char('b'),
                Value::Char('c'),
            ])),
        );
        assert_equal_values_hash_equal(Value::float(0.0), Value::float(-0.0));
    }

    #[test]
    fn complex_hash_matches_float_component_equality() {
        assert_equal_values_hash_equal(
            Value::from_complex64(Complex64::new(0.0, 0.0)),
            Value::from_complex64(Complex64::new(-0.0, -0.0)),
        );
        assert_equal_values_hash_equal(
            Value::from_complex64(Complex64::new(f64::NAN, 0.0)),
            Value::from_complex64(Complex64::new(f64::from_bits(0x7ff8_0000_0000_0001), -0.0)),
        );
    }

    #[test]
    fn empty_container_hashes() {
        let empty_list = Value::List(Arc::new(vec![]));
        let empty_intlist = Value::IntList(Arc::new(vec![]));
        let empty_dict = Value::Dict(Arc::new(IndexMap::new()));
        let empty_string = Value::String(Arc::new(String::new()));

        assert_eq!(hash_value(&empty_list), hash_value(&empty_intlist));
        assert_eq!(hash_value(&empty_string), hash_value(&empty_intlist));
        assert_eq!(hash_value(&empty_string), hash_value(&empty_list));

        assert_ne!(hash_value(&empty_dict), hash_value(&empty_intlist));
        assert_ne!(hash_value(&empty_string), hash_value(&empty_dict));
        assert_ne!(hash_value(&empty_list), hash_value(&empty_dict));
    }

    #[test]
    fn empty_containers_cross_equal() {
        let empty_string = Value::String(Arc::new(String::new()));
        let empty_list = Value::List(Arc::new(vec![]));
        let empty_intlist = Value::IntList(Arc::new(vec![]));
        let empty_dict = Value::Dict(Arc::new(IndexMap::new()));

        assert_eq!(empty_list, empty_intlist);
        assert_eq!(empty_string, empty_intlist);
        assert_eq!(empty_string, empty_list);

        assert_ne!(empty_dict, empty_intlist);
        assert_ne!(empty_string, empty_dict);
        assert_ne!(empty_list, empty_dict);
    }

    #[test]
    fn dict_order_affects_eq_and_hash() {
        let mut map_a = IndexMap::new();
        map_a.insert(Arc::from("a"), Value::Int(1));
        map_a.insert(Arc::from("b"), Value::Int(2));
        let dict_a = Value::Dict(Arc::new(map_a));

        let mut map_b = IndexMap::new();
        map_b.insert(Arc::from("b"), Value::Int(2));
        map_b.insert(Arc::from("a"), Value::Int(1));
        let dict_b = Value::Dict(Arc::new(map_b));

        assert_ne!(dict_a, dict_b);
        assert_ne!(hash_value(&dict_a), hash_value(&dict_b));
    }

    #[test]
    fn cas_application_hash_distinguishes_builtin_function() {
        let app = Value::from_cas_apply("sin", vec![Value::from_cas_var("x")]);
        let builtin = Value::from_cas_function(CasFunction::Sin, vec![Value::from_cas_var("x")]);

        assert_ne!(app, builtin);
        assert_ne!(hash_value(&app), hash_value(&builtin));
    }

    #[test]
    fn display_no_cycle_normal_output() {
        let list = Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)]));
        let s = list.to_string();
        assert_eq!(s, "(1;2)");
    }

    #[test]
    fn deep_nesting_works() {
        let mut list = Value::List(Arc::new(vec![Value::Int(0)]));
        for _ in 0..100 {
            list = Value::List(Arc::new(vec![list.clone(), Value::Int(1)]));
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        list.hash(&mut hasher);
        let _ = hasher.finish();
        let _ = list.to_string();
        // Comparison
        let list2 = list.clone();
        assert_eq!(list, list2);
        assert_ne!(list, Value::empty_list());
    }
}
