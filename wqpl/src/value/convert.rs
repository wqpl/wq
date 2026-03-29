use std::sync::Arc;

use colored::Colorize;
use num_bigint::BigInt;
use num_complex::Complex64;
use num_rational::Ratio;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::value::{Value, expected_numeric1, into_wq_string};
use crate::wqerror::WqError;

impl Value {
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

    /// Construct a list value from items, promoting to IntList if all items are
    /// ints, or to String if all items are chars.
    pub(crate) fn from_items(items: Vec<Value>) -> Value {
        if items.iter().all(|v| matches!(v, Value::Int(_))) {
            Value::IntList(Arc::new(
                items
                    .iter()
                    .map(|v| match v {
                        Value::Int(i) => *i,
                        _ => unreachable!(),
                    })
                    .collect(),
            ))
        } else if items.iter().all(|v| matches!(v, Value::Char(_))) {
            Value::String(Arc::new(
                items
                    .iter()
                    .map(|v| match v {
                        Value::Char(c) => *c,
                        _ => unreachable!(),
                    })
                    .collect(),
            ))
        } else {
            Value::List(Arc::new(items))
        }
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

    pub(crate) fn try_as_complex64(&self) -> Result<Complex64, WqError> {
        match self {
            Value::Complex(z) => Ok(*z),
            Value::Int(n) => Ok(Complex64::new(*n as f64, 0.0)),
            Value::BigInt(n) => n
                .as_ref()
                .to_f64()
                .map(|re| Complex64::new(re, 0.0))
                .ok_or_else(|| expected_numeric1(self)),
            Value::Float(f) => Ok(Complex64::new(**f, 0.0)),
            _ if self.is_fraction() => self
                .as_f64()
                .map(|re| Complex64::new(re, 0.0))
                .ok_or_else(|| expected_numeric1(self)),
            _ => Err(expected_numeric1(self)),
        }
    }

    pub(crate) fn from_complex64(z: Complex64) -> Value {
        Value::Complex(z)
    }

    pub(crate) fn format_complex64(z: Complex64, stylize: bool) -> String {
        let i_text = if stylize {
            "i".italic().to_string()
        } else {
            "i".to_string()
        };
        let re = z.re;
        let im = z.im;
        if im == 0.0 {
            return format!("{}+0{i_text}", Self::format_numeric_component(re));
        }
        let imag_mag = Self::format_numeric_component(im.abs());
        let imag_term = if imag_mag == "1" {
            i_text
        } else {
            format!("{imag_mag}{i_text}")
        };
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
}
