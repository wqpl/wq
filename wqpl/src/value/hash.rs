use std::hash::Hasher;
use std::sync::Arc;

use num_traits::ToPrimitive;

use crate::value::Value;
use crate::value::cas::CasKind;

impl std::hash::Hash for Value {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // - Int and BigInt cross-equality
        // - IntList and List cross-equality
        // - All unit values cross-equality ()=(`)=""=S()
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
            // IntList, List, String, Dict, and Set share a tag when empty
            // so that all unit values hash the same.
            v if v.is_unit() => {
                6u8.hash(state);
                0usize.hash(state);
            }
            Value::IntList(v) => {
                6u8.hash(state);
                v.len().hash(state);
                for item in v.iter() {
                    0u8.hash(state);
                    item.hash(state);
                }
            }
            Value::List(v) => {
                6u8.hash(state);
                v.len().hash(state);
                for item in v.iter() {
                    item.hash(state);
                }
            }
            Value::Dict(m) => {
                7u8.hash(state);
                m.len().hash(state);
                let mut entries: Vec<_> = m.iter().collect();
                entries.sort_by_key(|(k, _)| *k);
                for (k, v) in entries {
                    k.hash(state);
                    v.hash(state);
                }
            }
            Value::String(s) => {
                // Hash identically to List<Char> for cross-equality consistency.
                6u8.hash(state);
                s.chars().count().hash(state);
                for c in s.chars() {
                    Value::Char(c).hash(state);
                }
            }
            Value::Complex(z) => {
                14u8.hash(state);
                z.re.to_bits().hash(state);
                z.im.to_bits().hash(state);
            }
            Value::Fraction(fd) => {
                15u8.hash(state);
                fd.numer().hash(state);
                fd.denom().hash(state);
            }
            Value::Set(s) => {
                19u8.hash(state);
                s.len().hash(state);
                let mut hashes: Vec<u64> = s
                    .iter()
                    .map(|item| {
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        item.hash(&mut hasher);
                        hasher.finish()
                    })
                    .collect();
                hashes.sort();
                for h in hashes {
                    h.hash(state);
                }
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
            Value::BuiltinFunction(name) => {
                10u8.hash(state);
                name.hash(state);
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
                    CasKind::Call(name, args) => {
                        2u8.hash(state);
                        name.hash(state);
                        for arg in args.iter() {
                            arg.hash(state);
                        }
                    }
                    CasKind::Eq(lhs, rhs) => {
                        3u8.hash(state);
                        lhs.hash(state);
                        rhs.hash(state);
                    }
                }
            }
            Value::Stream(s) => {
                17u8.hash(state);
                Arc::as_ptr(s).hash(state);
            }
            Value::Algebraic(a) => {
                20u8.hash(state);
                a.poly.hash(state);
                a.interval.0.to_bits().hash(state);
                a.interval.1.to_bits().hash(state);
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
        // All unit values are cross-equal regardless of concrete type.
        if self.is_unit() && other.is_unit() {
            return true;
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
            (Dict(a), Dict(b)) => a.len() == b.len() && a.iter().all(|(k, v)| b.get(k) == Some(v)),
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
            (Complex(a), Complex(b)) => a == b,
            (Fraction(a), Fraction(b)) => a == b,
            (Cas(a), Cas(b)) => match (&a.kind, &b.kind) {
                (CasKind::Var(na), CasKind::Var(nb)) => na == nb,
                (CasKind::Const(na), CasKind::Const(nb)) => na == nb,
                (CasKind::Op(opa, arga), CasKind::Op(opb, argb)) => {
                    opa == opb
                        && arga.len() == argb.len()
                        && arga.iter().zip(argb.iter()).all(|(x, y)| x == y)
                }
                (CasKind::Call(na, arga), CasKind::Call(nb, argb)) => {
                    na == nb
                        && arga.len() == argb.len()
                        && arga.iter().zip(argb.iter()).all(|(x, y)| x == y)
                }
                (CasKind::Eq(lhsa, rhsa), CasKind::Eq(lhsb, rhsb)) => lhsa == lhsb && rhsa == rhsb,
                _ => false,
            },
            (String(a), String(b)) => a == b,
            (String(a), List(b)) | (List(b), String(a)) => {
                a.chars().count() == b.len()
                    && a.chars()
                        .zip(b.iter())
                        .all(|(c, v)| matches!(v, Char(ch) if *ch == c))
            }
            (Set(a), Set(b)) => a.len() == b.len() && a.iter().all(|va| b.contains(va)),
            (BuiltinFunction(a), BuiltinFunction(b)) => a == b,
            (Stream(a), Stream(b)) => Arc::ptr_eq(a, b),
            (Algebraic(a), Algebraic(b)) => {
                a.poly == b.poly
                    && a.interval == b.interval
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

    use indexmap::{IndexMap, IndexSet};

    use super::*;

    fn hash_value(v: &Value) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        v.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn unit_cross_type_hash() {
        let empty_string = Value::String(Arc::new(String::new()));
        let empty_list = Value::List(Arc::new(vec![]));
        let empty_intlist = Value::IntList(Arc::new(vec![]));
        let empty_dict = Value::Dict(Arc::new(IndexMap::new()));
        let empty_set = Value::Set(Arc::new(IndexSet::new()));

        assert_eq!(hash_value(&empty_string), hash_value(&empty_list));
        assert_eq!(hash_value(&empty_string), hash_value(&empty_intlist));
        assert_eq!(hash_value(&empty_string), hash_value(&empty_dict));
        assert_eq!(hash_value(&empty_string), hash_value(&empty_set));
    }

    #[test]
    fn unit_cross_type_eq() {
        let empty_string = Value::String(Arc::new(String::new()));
        let empty_list = Value::List(Arc::new(vec![]));
        let empty_intlist = Value::IntList(Arc::new(vec![]));
        let empty_dict = Value::Dict(Arc::new(IndexMap::new()));
        let empty_set = Value::Set(Arc::new(IndexSet::new()));

        assert_eq!(empty_string, empty_list);
        assert_eq!(empty_string, empty_intlist);
        assert_eq!(empty_string, empty_dict);
        assert_eq!(empty_string, empty_set);
    }

    #[test]
    fn dict_unordered_eq_and_hash() {
        let mut map_a = IndexMap::new();
        map_a.insert(Arc::from("a"), Value::Int(1));
        map_a.insert(Arc::from("b"), Value::Int(2));
        let dict_a = Value::Dict(Arc::new(map_a));

        let mut map_b = IndexMap::new();
        map_b.insert(Arc::from("b"), Value::Int(2));
        map_b.insert(Arc::from("a"), Value::Int(1));
        let dict_b = Value::Dict(Arc::new(map_b));

        assert_eq!(dict_a, dict_b);
        assert_eq!(hash_value(&dict_a), hash_value(&dict_b));
    }

    #[test]
    fn set_unordered_eq_and_hash() {
        let set_a = Value::Set(Arc::new(IndexSet::from_iter([
            Value::Int(1),
            Value::Int(2),
        ])));
        let set_b = Value::Set(Arc::new(IndexSet::from_iter([
            Value::Int(2),
            Value::Int(1),
        ])));

        assert_eq!(set_a, set_b);
        assert_eq!(hash_value(&set_a), hash_value(&set_b));
    }

    #[test]
    fn display_no_cycle_normal_output() {
        let list = Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)]));
        let s = list.to_string();
        assert_eq!(s, "(1;2)");
    }

    #[test]
    fn deep_nesting_works() {
        // Build a deeply nested list — should not crash.
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
        assert_ne!(list, Value::unit());
    }
}
