use std::borrow::Cow;
use std::sync::Arc;

use num_bigint::BigInt;
use num_complex::Complex64;
use num_rational::Ratio;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::value::seq::ValueSeqBuilder;
use crate::value::{Value, into_wq_string};

impl Value {
    pub(crate) fn can_convert_to_vec_u8(&self) -> bool {
        self.exact_int_seq()
            .is_some_and(|items| items.iter().all(|item| u8::try_from(item).is_ok()))
    }

    pub(crate) fn as_rust_char_slice<'a>(&'a self) -> Option<Cow<'a, [Value]>> {
        match self {
            Value::String(value) => Some(Cow::Owned(value.chars().map(Value::Char).collect())),
            Value::List(items) if items.iter().all(|item| matches!(item, Value::Char(_))) => {
                Some(Cow::Borrowed(items))
            }
            Value::Char(value) => Some(Cow::Owned(vec![Value::Char(*value)])),
            value if value.is_unit() => Some(Cow::Owned(vec![])),
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn try_to_rust_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// Convert a char, string, unit, or char-only list into a Rust string.
    ///
    /// Do not call from `Display::fmt`.
    pub(crate) fn try_to_rust_string(&self) -> Option<String> {
        match self {
            Value::String(value) => Some(value.to_string()),
            Value::Char(value) => Some(value.to_string()),
            Value::List(items) => {
                let mut string = String::with_capacity(items.len());
                for item in items.iter() {
                    let Value::Char(value) = item else {
                        return None;
                    };
                    string.push(*value);
                }
                Some(string)
            }
            value if value.is_unit() => Some(String::new()),
            _ => None,
        }
    }

    /// Try to flatten a value into a Rust [`String`].
    ///
    /// - `Char` → single-character string
    /// - `String` → cloned string
    /// - `List` where every element is string-like → concatenated string
    /// - empty list containers → empty string
    /// - everything else → `None`
    pub(crate) fn try_flatten_to_rust_string(&self) -> Option<String> {
        match self {
            Value::Char(value) => Some(value.to_string()),
            Value::String(value) => Some(value.to_string()),
            Value::List(items) => {
                if items.is_empty() {
                    return Some(String::new());
                }
                if !items.iter().all(Value::is_string) {
                    return None;
                }
                let mut output = String::new();
                for item in items.iter() {
                    output.push_str(&item.try_to_rust_string()?);
                }
                Some(output)
            }
            value if value.is_unit() => Some(String::new()),
            _ => None,
        }
    }

    pub(crate) fn try_to_rust_vec_u8(&self) -> Option<Vec<u8>> {
        self.exact_int_seq()?
            .iter()
            .map(|item| u8::try_from(item).ok())
            .collect()
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            Value::BigInt(n) => n.to_i64(),
            Value::Float(f) => Some(**f as i64),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(n) => Some(*n as f64),
            Value::BigInt(n) => n.to_f64(),
            Value::Float(f) => Some(**f),
            Value::Fraction(fd) => fd.to_f64(),
            _ => None,
        }
    }
    pub(crate) fn from_bigint(n: BigInt) -> Value {
        n.to_i64()
            .map(Value::Int)
            .unwrap_or_else(|| Value::BigInt(Arc::new(n)))
    }

    /// Construct a list value from items, selecting packed storage for
    /// homogeneous ints, floats, or bools and string storage for chars.
    pub(crate) fn from_items(items: Vec<Value>) -> Value {
        ValueSeqBuilder::from_items(items)
    }

    pub(crate) fn value_from_str_chunks(chunks: Vec<String>) -> Value {
        Value::from_items(
            chunks
                .into_iter()
                .map(|chunk| chunk.into_wq_value())
                .collect(),
        )
    }
}

pub(crate) trait IntoWqValue {
    fn into_wq_value(self) -> Value;
}

impl IntoWqValue for String {
    fn into_wq_value(self) -> Value {
        into_wq_string(self)
    }
}

impl IntoWqValue for &str {
    fn into_wq_value(self) -> Value {
        into_wq_string(self)
    }
}

impl IntoWqValue for usize {
    fn into_wq_value(self) -> Value {
        match i64::try_from(self) {
            Ok(n) => Value::Int(n),
            Err(_) => Value::BigInt(Arc::new(BigInt::from(self))),
        }
    }
}

impl IntoWqValue for u16 {
    fn into_wq_value(self) -> Value {
        Value::Int(i64::from(self))
    }
}

impl IntoWqValue for u64 {
    fn into_wq_value(self) -> Value {
        match i64::try_from(self) {
            Ok(n) => Value::Int(n),
            Err(_) => Value::BigInt(Arc::new(BigInt::from(self))),
        }
    }
}

impl Value {
    pub(crate) fn format_numeric_component(n: f64) -> String {
        if n.is_infinite() && n.is_sign_positive() {
            "inf".to_string()
        } else if n.is_infinite() && n.is_sign_negative() {
            "-inf".to_string()
        } else if n.is_nan() {
            "nan".to_string()
        } else if n.fract() == 0.0 {
            format!("{n:.0}")
        } else {
            n.to_string()
        }
    }

    pub(crate) fn is_complex(&self) -> bool {
        matches!(self, Value::Complex(_))
    }

    pub(crate) fn as_complex64(&self) -> Option<Complex64> {
        match self {
            Value::Complex(z) => Some(*z),
            Value::Int(n) => Some(Complex64::new(*n as f64, 0.0)),
            Value::BigInt(n) => Some(Complex64::new(n.as_ref().to_f64()?, 0.0)),
            Value::Float(f) => Some(Complex64::new(**f, 0.0)),
            _ if self.is_fraction() => Some(Complex64::new(self.as_f64()?, 0.0)),
            _ => None,
        }
    }

    pub(crate) fn from_complex64(z: Complex64) -> Value {
        Value::Complex(z)
    }

    pub(crate) fn format_complex64(z: Complex64, _stylize: bool) -> String {
        let i_text = "i".to_string();
        let re = z.re;
        let im = z.im;
        if im == 0.0 {
            return format!("{}+0{i_text}", Self::format_numeric_component(re));
        }
        let imag_mag = Self::format_numeric_component(im.abs());
        let imag_term = format!("{imag_mag}{i_text}");
        if re == 0.0 {
            format!(
                "{sign}{imag_term}",
                sign = if im.is_sign_negative() { "-" } else { "" }
            )
        } else {
            format!(
                "{re_term}{sign}{imag_term}",
                re_term = Self::format_numeric_component(re),
                sign = if im.is_sign_negative() { "-" } else { "+" }
            )
        }
    }
}

impl Value {
    pub(crate) fn is_fraction(&self) -> bool {
        matches!(self, Value::Fraction(_))
    }

    pub(crate) fn dict_fraction_parts(&self) -> Option<(BigInt, BigInt)> {
        match self {
            Value::Fraction(fd) => Some((fd.numer().clone(), fd.denom().clone())),
            _ => None,
        }
    }

    pub(crate) fn rational_parts(&self) -> Option<(BigInt, BigInt)> {
        match self {
            Value::Fraction(fd) => Some((fd.numer().clone(), fd.denom().clone())),
            Value::Int(n) => Some((BigInt::from(*n), BigInt::one())),
            Value::BigInt(n) => Some(((**n).clone(), BigInt::one())),
            _ => None,
        }
    }

    pub(crate) fn raw_from_fraction_parts_ref(numer: &BigInt, denom: &BigInt) -> Value {
        debug_assert!(!denom.is_zero());
        if numer.is_zero() {
            return Value::Fraction(Arc::new(Ratio::new_raw(BigInt::zero(), BigInt::one())));
        }
        let g = gcd_bigint(numer, denom);
        let mut numer = numer / &g;
        let mut denom = denom / &g;
        if denom.is_negative() {
            numer = -numer;
            denom = -denom;
        }
        Value::Fraction(Arc::new(Ratio::new_raw(numer, denom)))
    }

    pub(crate) fn from_fraction_parts(numer: BigInt, denom: BigInt) -> Value {
        Self::raw_from_fraction_parts_ref(&numer, &denom)
    }
}

fn gcd_bigint(a: &BigInt, b: &BigInt) -> BigInt {
    let mut a = a.abs();
    let mut b = b.abs();
    while !b.is_zero() {
        let r = &a % &b;
        a = b;
        b = r;
    }
    a
}

#[cfg(test)]
mod tests {
    use num_bigint::BigInt;

    use super::*;

    #[test]
    fn fraction_normalizes_sign() {
        let value = Value::from_fraction_parts(BigInt::from(1), BigInt::from(-2));
        assert_eq!(value.to_string(), "-1/2");
        assert!(value.is_fraction());
    }

    #[test]
    fn fraction_from_pairs() {
        let f = Value::from_fraction_parts(BigInt::from(3), BigInt::from(9));
        assert_eq!(f.to_string(), "1/3");
    }

    #[test]
    fn string_conversion_has_no_diagnostic_side_effects() {
        let value = Value::List(Arc::new(vec![Value::Char('a'), Value::Int(2)]));

        assert_eq!(value.try_to_rust_string(), None);
    }

    #[test]
    fn byte_conversion_has_no_diagnostic_side_effects() {
        let value = Value::IntList(Arc::new(vec![0, 255, 256]));

        assert_eq!(value.try_to_rust_vec_u8(), None);
    }
}
