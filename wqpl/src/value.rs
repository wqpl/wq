pub mod access;
pub mod algebraic;
pub mod bc;
pub mod cas;
pub mod cell;
pub mod cmp;
pub mod convert;
pub mod display;
pub mod func;
pub mod hash;
pub mod mat;
pub mod math;
pub mod meta;
pub mod op;
pub mod stream;

use std::borrow::Cow;
use std::sync::{Arc, Mutex};

pub(crate) use convert::IntoWqValue;
pub use display::Excerpt;
pub(crate) use display::into_wq_string;
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_complex::Complex64;
use num_rational::Ratio;
use num_traits::ToPrimitive;
pub(crate) use op::{eval_binary, eval_unary};
use ordered_float::OrderedFloat;

use crate::value::cas::CasData;
use crate::value::func::{ClosureData, FunctionData};
use crate::value::stream::StreamHandle;
use crate::wqerror::{WqError, WqErrorType};

pub type WqResult<T> = Result<T, WqError>;

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
    List(Arc<Vec<Value>>),
    /// Heap-allocated string with copy-on-write mutation support.
    String(Arc<String>),
    /// Symbolic algebra expression.
    Cas(Arc<CasData>),
    Dict(Arc<IndexMap<Arc<str>, Value>>),
    CompiledFunction(Arc<FunctionData>),
    /// closure with captured cells (upvalues)
    Closure(Arc<ClosureData>),
    BuiltinFunction(Arc<str>),
    Stream(Arc<Mutex<StreamHandle>>),
}

impl Value {
    /// Get the length of a value
    pub fn len(&self) -> usize {
        match self {
            Value::List(items) => items.len(),
            Value::IntList(items) => items.len(),
            Value::String(s) => s.chars().count(),
            Value::Dict(map) => map.len(),

            _ => 1, // Atoms have length 1
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn unit() -> Self {
        Value::IntList(Arc::new(vec![]))
    }

    pub fn is_unit(&self) -> bool {
        self.is_empty()
    }

    /// Convenience constructor for `Value::Float`.
    #[inline]
    pub(crate) fn float(f: impl Into<f64>) -> Self {
        Value::Float(OrderedFloat(f.into()))
    }

    /// Create a new stream value
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn stream(handle: StreamHandle) -> Self {
        Value::Stream(Arc::new(Mutex::new(handle)))
    }

    pub fn is_atom(&self) -> bool {
        !matches!(
            self,
            Value::IntList(_) | Value::List(_) | Value::Dict(_) | Value::String(_)
        )
    }

    pub(crate) fn is_string_like(&self) -> bool {
        matches!(self, Value::String(_) | Value::Char(_))
            || self.is_unit()
            || matches!(self, Value::List(items) if items.iter().all(|v| matches!(v, Value::Char(_))))
    }

    pub(crate) fn can_convert_to_vec_u8(&self) -> bool {
        match self {
            Value::Int(n) => u8::try_from(*n).is_ok(),
            Value::BigInt(n) => n.to_u8().is_some(),
            Value::IntList(items) => items.iter().all(|&n| u8::try_from(n).is_ok()),
            Value::List(items) => items.iter().all(|v| match v {
                Value::Int(n) => u8::try_from(*n).is_ok(),
                Value::BigInt(n) => n.to_u8().is_some(),
                _ => false,
            }),
            _ => false,
        }
    }

    pub(crate) fn as_rust_char_slice<'a>(&'a self) -> Option<Cow<'a, [Value]>> {
        match self {
            Value::String(s) => Some(Cow::Owned(s.chars().map(Value::Char).collect())),
            Value::List(items) if items.iter().all(|v| matches!(v, Value::Char(_))) => {
                Some(Cow::Borrowed(items))
            }
            Value::Char(c) => Some(Cow::Owned(vec![Value::Char(*c)])),
            v if v.is_unit() => Some(Cow::Owned(vec![])),
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn try_to_rust_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            // Value::Int(1) => Some(true),
            // Value::Int(0) => Some(false),
            // Value::BigInt(bi) if **bi == BigInt::from(1) => Some(true),
            // Value::BigInt(bi) if **bi == BigInt::from(0) => Some(false),
            _ => None,
        }
    }

    /// Convert a char, string, or char-only list into a Rust String.
    ///
    /// Do not call from `Display::fmt`.
    pub(crate) fn to_rust_string_with_note(&self) -> WqResult<String> {
        const EXP: &str = "expected char or string";
        match self {
            Value::String(s) => Ok(s.to_string()),
            Value::Char(c) => Ok(c.to_string()),
            Value::List(items) => {
                let mut s = String::with_capacity(items.len());
                for (i, v) in items.iter().enumerate() {
                    if let Value::Char(c) = v {
                        s.push(*c);
                    } else {
                        return Err(WqError::new(WqErrorType::Domain)
                            .msg(EXP)
                            .offending_elem(v, i));
                    }
                }
                Ok(s)
            }
            _ if self.is_unit() => Ok(String::new()),
            _ => Err(WqError::new(WqErrorType::Domain).msg(EXP).got1(self)),
        }
    }

    /// Try to flatten a value into a Rust [`String`].
    ///
    /// - `Char` → single-character string
    /// - `String` → cloned string
    /// - `List` where every element is string-like → concatenated string
    /// - empty `IntList` → empty string
    /// - everything else → `None`
    pub(crate) fn try_flatten_to_string(&self) -> Option<String> {
        match self {
            Value::Char(c) => Some(c.to_string()),
            Value::String(s) => Some(s.to_string()),
            Value::List(items) => {
                if items.is_empty() {
                    return Some(String::new());
                }
                if !items.iter().all(|v| v.is_string_like()) {
                    return None;
                }
                let mut out = String::new();
                for v in items.iter() {
                    out.push_str(&v.to_rust_string_with_note().ok()?);
                }
                Some(out)
            }
            Value::IntList(items) if items.is_empty() => Some(String::new()),
            _ => None,
        }
    }

    pub(crate) fn try_to_vec_u8(&self) -> WqResult<Vec<u8>> {
        const EXP: &str = "expected list<int in 0..=255>";

        match self {
            Value::IntList(l) => l
                .iter()
                .enumerate()
                .map(|(i, &n)| {
                    u8::try_from(n).map_err(|_| {
                        WqError::new(WqErrorType::Domain)
                            .msg(EXP)
                            .offending_elem(&Value::Int(n), i)
                    })
                })
                .collect(),
            Value::List(items) => items
                .iter()
                .enumerate()
                .map(|(i, v)| match v {
                    Value::Int(n) => u8::try_from(*n).map_err(|_| {
                        WqError::new(WqErrorType::Domain)
                            .msg(EXP)
                            .offending_elem(v, i)
                    }),
                    Value::BigInt(n) => n.to_u8().ok_or_else(|| {
                        WqError::new(WqErrorType::Domain)
                            .msg(EXP)
                            .offending_elem(v, i)
                    }),
                    _ => Err(WqError::new(WqErrorType::Domain)
                        .msg(EXP)
                        .offending_elem(v, i)),
                })
                .collect(),
            Value::Int(n) => u8::try_from(*n)
                .map(|b| vec![b])
                .map_err(|_| WqError::new(WqErrorType::Domain).msg(EXP).got1(self)),
            Value::BigInt(n) => n
                .to_u8()
                .map(|b| vec![b])
                .ok_or_else(|| WqError::new(WqErrorType::Domain).msg(EXP).got1(self)),
            _ => Err(WqError::new(WqErrorType::Domain).msg(EXP).got1(self)),
        }
    }

    /// Get the type name of a value
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::BigInt(_) => "bigint",
            Value::Float(_) => "float",
            Value::Complex(_) => "complex",
            Value::Fraction(_) => "fraction",
            Value::Algebraic(_) => "algebraic",
            Value::Char(_) => "char",
            Value::Tag(_) => "tag",
            Value::Bool(_) => "bool",

            Value::IntList(_) if self.is_unit() => "intlist (unit)",
            Value::IntList(_) => "intlist",

            Value::List(_) if self.is_unit() => "list (unit)",
            Value::List(_) if self.is_string_like() => "list (string)",
            Value::List(_) => "list",

            Value::String(_) if self.is_unit() => "string (unit)",
            Value::String(_) => "string",

            Value::Cas(_) => "cas",
            Value::Dict(_) if self.is_unit() => "dict (unit)",
            Value::Dict(_) => "dict",

            Value::CompiledFunction { .. } => "fn",
            Value::Closure { .. } => "closure",
            Value::BuiltinFunction(_) => "bfn",

            Value::Stream(_) => "stream",
        }
    }
}

pub(crate) fn expected_numeric1(v: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .msg("expected int, bigint, float or fraction")
        .got1(v)
}

pub(crate) fn expected_numeric2(lhs: &Value, rhs: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .msg("expected int, bigint, float or fraction")
        .got2(lhs, rhs)
}

pub(crate) fn expected_integer1(v: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .msg("expected int or bigint")
        .got1(v)
}

pub(crate) fn expected_integer2(lhs: &Value, rhs: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .msg("expected int or bigint")
        .got2(lhs, rhs)
}

pub(crate) fn expected_bool1(v: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .msg("expected bool")
        .got1(v)
}

pub(crate) fn expected_bool2(lhs: &Value, rhs: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .msg("expected bool")
        .got2(lhs, rhs)
}

#[cfg(test)]
mod tests {

    use super::*;

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

        let scalar = Value::Int(1);
        let vec = Value::List(Arc::new(vec![Value::Int(1), Value::Int(1)]));
        assert_eq!(
            scalar.eq_bc(&vec),
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
            a.and_bool(&b),
            Ok(Value::List(Arc::new(vec![
                Value::Bool(true),
                Value::Bool(false)
            ])))
        );

        assert_eq!(
            b.or_bool(&Value::Bool(false)),
            Ok(Value::List(Arc::new(vec![
                Value::Bool(true),
                Value::Bool(false)
            ])))
        );

        let c = Value::List(Arc::new(vec![Value::Bool(true)]));
        let d = Value::List(Arc::new(vec![Value::Bool(false), Value::Bool(true)]));
        assert!(c.xor_bool(&d).is_err());

        assert_eq!(
            d.not_bool(),
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
        assert_eq!(a.bnot(), Ok(Value::Int(!6)));
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

    // ── String variant tests ──

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
        assert_eq!(into_wq_string("🦀🚀").len(), 2); // character count, not byte len
    }

    #[test]
    fn string_is_empty_and_unit() {
        let empty = into_wq_string("");
        assert!(empty.is_empty());
        assert!(empty.is_unit());

        let non_empty = into_wq_string("x");
        assert!(!non_empty.is_empty());
        assert!(!non_empty.is_unit());
    }

    #[test]
    fn string_is_atom() {
        assert!(!into_wq_string("hello").is_atom());
        assert!(!into_wq_string("").is_atom());
    }

    #[test]
    fn string_type_name() {
        assert_eq!(into_wq_string("hello").type_name(), "string");
        assert_eq!(into_wq_string("").type_name(), "string (unit)");
    }

    #[test]
    fn string_is_string_like() {
        assert!(into_wq_string("hello").is_string_like());
        assert!(into_wq_string("").is_string_like());
    }

    #[test]
    fn string_try_to_rust_string() {
        assert_eq!(
            into_wq_string("hello").to_rust_string_with_note().unwrap(),
            "hello".to_string()
        );
        assert_eq!(
            into_wq_string("").to_rust_string_with_note().unwrap(),
            "".to_string()
        );
    }

    #[test]
    fn string_display_quotes_and_escapes() {
        assert_eq!(into_wq_string("hello").to_string(), "\"hello\"");
        assert_eq!(into_wq_string("").to_string(), "\"\"");
        assert_eq!(into_wq_string("a\"b").to_string(), "\"a\\\"b\"");
    }

    #[test]
    fn string_partial_eq() {
        let a = into_wq_string("hello");
        let b = into_wq_string("hello");
        let c = into_wq_string("world");
        assert_eq!(a, b);
        assert_ne!(a, c);
        // String and List<Char> are cross-equal (same user-facing value)
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
        // Old-style List<Char> still works as a string via fallback paths
        let old_style = Value::List(Arc::new("hi".chars().map(Value::Char).collect()));
        assert!(old_style.is_string_like());
        assert_eq!(
            old_style.to_rust_string_with_note().unwrap(),
            "hi".to_string()
        );
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

    // ── Complex variant tests ──

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
    fn complex_type_name() {
        let z = Value::from_complex64(num_complex::Complex64::new(1.0, 0.0));
        assert_eq!(z.type_name(), "complex");
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

    // ── Fraction variant tests ──

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
    fn fraction_type_name() {
        let f =
            Value::from_fraction_parts(num_bigint::BigInt::from(1), num_bigint::BigInt::from(3));
        assert_eq!(f.type_name(), "fraction");
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
