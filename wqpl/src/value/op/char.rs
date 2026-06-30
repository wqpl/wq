use std::sync::Arc;

use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive};
use rayon::prelude::*;

use crate::value::op::PAR_BC_THRESHOLD;
use crate::value::{Value, WqResult, expected_integer1};
use crate::wqerror::{WqError, WqErrorType};

fn invalid_unicode(v: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .msg("invalid Unicode code point")
        .attach_note("valid Unicode code points are 0x0000..=0xD7FF, 0xE000..=0x10FFFF")
        .got1(v)
}

fn chr_intlist(v: &Value) -> Option<Value> {
    match v {
        Value::IntList(a) => {
            if a.len() > PAR_BC_THRESHOLD {
                let chars: Option<Vec<char>> = a
                    .par_iter()
                    .map(|&x| u32::try_from(x).ok().and_then(char::from_u32))
                    .collect();
                Some(Value::String(Arc::new(chars?.into_iter().collect())))
            } else {
                let mut s = String::with_capacity(a.len());
                for &x in a.iter() {
                    s.push(u32::try_from(x).ok().and_then(char::from_u32)?);
                }
                Some(Value::String(Arc::new(s)))
            }
        }
        _ => None,
    }
}

fn ord_string(v: &Value) -> Option<Value> {
    match v {
        Value::String(s) => {
            let codes: Vec<i64> = s.chars().map(|c| i64::from(u32::from(c))).collect();
            Some(Value::IntList(Arc::new(codes)))
        }
        _ => None,
    }
}

impl Value {
    pub(crate) fn chr(&self) -> WqResult<Value> {
        if let Some(res) = chr_intlist(self) {
            return Ok(res);
        }
        self.bc1(|v| match v {
            Value::Int(i) => {
                let ch = u32::try_from(*i) // reject negatives/overflow
                    .ok()
                    .and_then(char::from_u32) // reject > 0x10FFFF and surrogates
                    .ok_or_else(|| invalid_unicode(v))?;
                Ok(Value::Char(ch))
            }
            Value::BigInt(n) => {
                let ch = n
                    .to_u32()
                    .and_then(char::from_u32)
                    .ok_or_else(|| invalid_unicode(v))?;
                Ok(Value::Char(ch))
            }
            _ => Err(expected_integer1(v)),
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn ord(&self) -> WqResult<Value> {
        if let Some(res) = ord_string(self) {
            return Ok(res);
        }
        self.bc1(|v| match v {
            Value::Char(c) => Ok(Value::Int(i64::from(u32::from(*c)))),
            // Value::Symbol(s) => {
            //     let mut codes = Vec::with_capacity(s.chars().count());
            //     codes.extend(s.chars().map(|c| i64::from(u32::from(c))));
            //     Ok(Value::IntList(Arc::new(codes)))
            // }
            _ => Err(WqError::new(WqErrorType::Domain)
                .msg("expected char or string")
                .got1(v)),
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn to_hex_repr(&self, with_prefix: bool) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => {
                let s = to_radix_string(*n, 16, with_prefix, "0x");
                Ok(Value::String(Arc::new(s)))
            }
            Value::BigInt(n) => {
                let s = to_bigint_radix_string(n, 16, with_prefix, "0x");
                Ok(Value::String(Arc::new(s)))
            }
            _ => Err(expected_integer1(v)),
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn to_bin_repr(&self, with_prefix: bool) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => {
                let s = to_radix_string(*n, 2, with_prefix, "0b");
                Ok(Value::String(Arc::new(s)))
            }
            Value::BigInt(n) => {
                let s = to_bigint_radix_string(n, 2, with_prefix, "0b");
                Ok(Value::String(Arc::new(s)))
            }
            _ => Err(expected_integer1(v)),
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn to_oct_repr(&self, with_prefix: bool) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => {
                let s = to_radix_string(*n, 8, with_prefix, "0o");
                Ok(Value::String(Arc::new(s)))
            }
            Value::BigInt(n) => {
                let s = to_bigint_radix_string(n, 8, with_prefix, "0o");
                Ok(Value::String(Arc::new(s)))
            }
            _ => Err(expected_integer1(v)),
        })
        .map_err(|e| e.into_wqerror())
    }
}

fn to_radix_string(n: i64, base: u32, with_prefix: bool, prefix: &str) -> String {
    let neg = n < 0;
    let mag = n.unsigned_abs();
    let digits = match base {
        16 => format!("{mag:x}",),
        8 => format!("{mag:o}",),
        2 => format!("{mag:b}",),
        _ => unreachable!("to_radix_string only used for base 2, 8 and 16"),
    };
    match (neg, with_prefix) {
        (true, true) => format!("-{prefix}{digits}"),
        (true, false) => format!("-{digits}"),
        (false, true) => format!("{prefix}{digits}"),
        (false, false) => digits,
    }
}

fn to_bigint_radix_string(n: &BigInt, base: u32, with_prefix: bool, prefix: &str) -> String {
    let neg = n.is_negative();
    let mag = n.abs().to_str_radix(base);
    match (neg, with_prefix) {
        (true, true) => format!("-{prefix}{mag}"),
        (true, false) => format!("-{mag}"),
        (false, true) => format!("{prefix}{mag}"),
        (false, false) => mag,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn chr_valid() {
        assert_eq!(Value::Int(65).chr().unwrap(), Value::Char('A'));
        assert_eq!(Value::Int(0x1F600).chr().unwrap(), Value::Char('😀'));
    }

    #[test]
    fn chr_invalid() {
        assert!(Value::Int(-1).chr().is_err());
        assert!(Value::Int(0x110000).chr().is_err()); // > Unicode max
    }

    #[test]
    fn radix_repr_handles_i64_min() {
        assert_eq!(
            Value::Int(i64::MIN).to_hex_repr(false).unwrap(),
            Value::String(Arc::new("-8000000000000000".to_owned()))
        );
        assert_eq!(
            Value::Int(i64::MIN).to_hex_repr(true).unwrap(),
            Value::String(Arc::new("-0x8000000000000000".to_owned()))
        );
    }
}
