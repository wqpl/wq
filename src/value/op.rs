use super::Value;
use crate::{value::WqResult, wqerror::WqError};

impl Value {
    pub fn neg(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => {
                n.checked_neg()
                    .map(Value::Int)
                    .ok_or(WqError::ArithmeticOverflowError(
                        "`-`: negation overflow".into(),
                    ))
            }
            Value::Float(f) => Ok(Value::Float(-f)),
            _ => Err(type_err1("-", v)),
        })
    }

    pub fn add(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => {
                x.checked_add(*y)
                    .map(Value::Int)
                    .ok_or(WqError::ArithmeticOverflowError(
                        "`+`: addition overflow".into(),
                    ))
            }
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x + y)),
            (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 + *y)),
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(*x + *y as f64)),
            _ => Err(type_err2("+", a, b)),
        })
    }

    pub fn subtract(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => {
                x.checked_sub(*y)
                    .map(Value::Int)
                    .ok_or(WqError::ArithmeticOverflowError(
                        "`-`: subtraction overflow".into(),
                    ))
            }
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x - y)),
            (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 - y)),
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x - *y as f64)),
            _ => Err(type_err2("-", a, b)),
        })
    }

    pub fn multiply(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => {
                x.checked_mul(*y)
                    .map(Value::Int)
                    .ok_or(WqError::ArithmeticOverflowError(
                        "`*`: multiplication overflow".into(),
                    ))
            }
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x * y)),
            (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 * y)),
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x * *y as f64)),
            _ => Err(type_err2("*", a, b)),
        })
    }

    pub fn divide(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(_), Value::Int(0)) => {
                Err(WqError::ZeroDivisionError("`/`: division by 0".into()))
            }
            (Value::Float(_), Value::Int(0)) => {
                Err(WqError::ZeroDivisionError("`/`: division by 0".into()))
            }
            (Value::Float(_), Value::Float(0.0)) => {
                Err(WqError::ZeroDivisionError("`/`: division by 0".into()))
            }

            (Value::Int(x), Value::Int(y)) => Ok(Value::Float(*x as f64 / *y as f64)),
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x / y)),
            (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 / y)),
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x / *y as f64)),
            _ => Err(type_err2("/", a, b)),
        })
    }

    pub fn divide_dot(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            // (Value::Int(_), Value::Int(0)) => None,
            // allow y==0.0`
            // (Value::Float(_), Value::Int(0)) => None,
            // (Value::Float(_), Value::Float(y)) if *y == 0.0 => None,
            (Value::Int(x), Value::Int(y)) => Ok(Value::Float(*x as f64 / *y as f64)),
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x / y)),
            (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 / y)),
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x / *y as f64)),
            _ => Err(type_err2("/.", a, b)),
        })
    }

    pub fn modulo(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(_), Value::Int(0)) => {
                Err(WqError::ZeroDivisionError("`%`: modulo by 0".into()))
            }
            (Value::Float(_), Value::Int(0)) => {
                Err(WqError::ZeroDivisionError("`%`: modulo by 0".into()))
            }
            (Value::Float(_), Value::Float(y)) if *y == 0.0 => {
                Err(WqError::ZeroDivisionError("`%`: modulo by 0".into()))
            }

            (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x % y)),
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x % y)),
            (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 % y)),
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x % *y as f64)),
            _ => Err(type_err2("%", a, b)),
        })
    }

    pub fn modulo_dot(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            // (Value::Int(_), Value::Int(0)) => None,
            // (Value::Float(_), Value::Int(0)) => None,
            // (Value::Float(_), Value::Float(y)) if *y == 0.0 => None,
            (Value::Int(x), Value::Int(y)) => Ok(Value::Float(*x as f64 % *y as f64)),
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x % y)),
            (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 % y)),
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x % *y as f64)),
            _ => Err(type_err2("%.", a, b)),
        })
    }

    // pub fn power(&self, other: &Value) -> WqResult<Value> {
    //     // implicit 0^0=1
    //     self.bc2(other, |a, b| match (a, b) {
    //         (Value::Int(x), Value::Int(y)) if *y >= 0 => {
    //             let uy = u32::try_from(*y).map_err(|_| {
    //                 WqError::ArithmeticOverflowError("`^`: exponent too large".into())
    //             })?;
    //             x.checked_pow(uy)
    //                 .map(Value::Int)
    //                 .ok_or(WqError::ArithmeticOverflowError(
    //                     "`^`: exponentiation overflow".into(),
    //                 ))
    //         }
    //         (Value::Int(0), Value::Int(y)) if *y < 0 => zero_neg_pow_err(),
    //         (Value::Int(x), Value::Int(y)) if *y < 0 => {
    //             Ok(Value::Float((*x as f64).powf(*y as f64)))
    //         }
    //         (Value::Float(0.0), Value::Float(y)) if *y < 0.0 => zero_neg_pow_err(),
    //         (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x.powf(*y))),
    //         (Value::Int(0), Value::Float(y)) if *y < 0.0 => zero_neg_pow_err(),
    //         (Value::Int(x), Value::Float(y)) => Ok(Value::Float((*x as f64).powf(*y))),
    //         (Value::Float(0.0), Value::Int(y)) if *y < 0 => zero_neg_pow_err(),
    //         (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x.powf(*y as f64))),
    //         _ => Err(type_err2("^", a, b)),
    //     })
    // }

    pub fn power(&self, other: &Value) -> WqResult<Value> {
        use std::f64::consts::PI;
        // implicit 0^0=1
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) if *y >= 0 => {
                let uy = u32::try_from(*y).map_err(|_| {
                    WqError::ArithmeticOverflowError("`^`: exponent too large".into())
                })?;
                x.checked_pow(uy)
                    .map(Value::Int)
                    .ok_or(WqError::ArithmeticOverflowError(
                        "`^`: exponentiation overflow".into(),
                    ))
            }

            (Value::Int(0), Value::Int(y)) if *y < 0 => zero_neg_pow_err(),
            (Value::Int(x), Value::Int(y)) if *y < 0 => {
                Ok(Value::Float((*x as f64).powf(*y as f64)))
            }

            (Value::Float(0.0), Value::Float(y)) if *y < 0.0 => zero_neg_pow_err(),

            // Float^Float: if powf() is NaN due to a negative base and non-integer exponent,
            // return principal complex result as [real, imag].
            (Value::Float(x), Value::Float(y)) => {
                let r = x.powf(*y);
                if r.is_nan() && *x < 0.0 && y.is_finite() {
                    let a = -*x;
                    let mag = a.powf(*y);
                    let theta = PI * *y;
                    let re = mag * theta.cos();
                    let im = mag * theta.sin();
                    Ok(Value::List(vec![Value::Float(re), Value::Float(im)]))
                } else {
                    Ok(Value::Float(r))
                }
            }

            (Value::Int(0), Value::Float(y)) if *y < 0.0 => zero_neg_pow_err(),

            // Int^Float: same NaN->complex handling as above
            (Value::Int(x), Value::Float(y)) => {
                let xb = *x as f64;
                let r = xb.powf(*y);
                if r.is_nan() && xb < 0.0 && y.is_finite() {
                    let a = -xb;
                    let mag = a.powf(*y);
                    let theta = PI * *y;
                    let re = mag * theta.cos();
                    let im = mag * theta.sin();
                    Ok(Value::List(vec![Value::Float(re), Value::Float(im)]))
                } else {
                    Ok(Value::Float(r))
                }
            }

            (Value::Float(0.0), Value::Int(y)) if *y < 0 => zero_neg_pow_err(),

            // Float^Int stays real; powf won't NaN here.
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x.powf(*y as f64))),

            _ => Err(type_err2("^", a, b)),
        })
    }

    // bitwise ops
    // ===========

    pub fn bitand(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x & y)),
            _ => Err(type_err2("band", a, b)),
        })
    }

    pub fn bitor(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x | y)),
            _ => Err(type_err2("bor", a, b)),
        })
    }

    pub fn bitxor(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x ^ y)),
            _ => Err(type_err2("bxor", a, b)),
        })
    }

    pub fn bitnot(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(x) => Ok(Value::Int(!x)),
            _ => Err(type_err1("bnot", v)),
        })
    }

    // Shifts (reject negative shift counts)

    pub fn shl(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(s)) if *s >= 0 => Ok(Value::Int(x.wrapping_shl(*s as u32))),
            _ => Err(type_err2("shl", a, b)),
        })
    }

    pub fn shr(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(s)) if *s >= 0 => Ok(Value::Int(x.wrapping_shr(*s as u32))),
            _ => Err(type_err2("shr", a, b)),
        })
    }

    // Logical

    pub fn and_bool(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a && *b)),
            _ => Err(type_err2("and", a, b)),
        })
    }

    pub fn or_bool(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a || *b)),
            _ => Err(type_err2("or", a, b)),
        })
    }

    pub fn xor_bool(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a ^ *b)),
            _ => Err(type_err2("xor", a, b)),
        })
    }

    pub fn not_bool(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Bool(b) => Ok(Value::Bool(!b)),
            _ => Err(type_err1("not", v)),
        })
    }

    pub fn chr(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(i) => {
                let ch = u32::try_from(*i) // reject negatives/overflow
                    .ok()
                    .and_then(char::from_u32) // reject > 0x10FFFF and surrogates
                    .ok_or_else(|| {
                        WqError::DomainError("`chr`: invalid Unicode scalar value".into())
                    })?;
                Ok(Value::Char(ch))
            }
            _ => Err(type_err1("chr", v)),
        })
    }

    pub fn ord(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Char(c) => Ok(Value::Int(i64::from(u32::from(*c)))),
            Value::Symbol(s) => {
                let mut codes = Vec::with_capacity(s.chars().count());
                codes.extend(s.chars().map(|c| i64::from(u32::from(c))));
                Ok(Value::IntList(codes))
            }
            _ => Err(type_err1("ord", v)),
        })
    }

    pub fn hex_with_prefix(&self, with_prefix: bool) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => {
                let s = to_radix_string(*n, 16, with_prefix, "0x");
                Ok(Value::List(s.chars().map(Value::Char).collect()))
            }
            _ => Err(WqError::DomainError(format!(
                "`hex`: cannot convert {v} to hex"
            ))),
        })
    }

    pub fn bin_with_prefix(&self, with_prefix: bool) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => {
                let s = to_radix_string(*n, 2, with_prefix, "0b");
                Ok(Value::List(s.chars().map(Value::Char).collect()))
            }
            _ => Err(WqError::DomainError(format!(
                "`bin`: cannot convert {v} to binary"
            ))),
        })
    }

    pub fn oct_with_prefix(&self, with_prefix: bool) -> WqResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => {
                let s = to_radix_string(*n, 8, with_prefix, "0o");
                Ok(Value::List(s.chars().map(Value::Char).collect()))
            }
            _ => Err(WqError::DomainError(format!(
                "`oct`: cannot convert {v} to octal"
            ))),
        })
    }

    // Comparison

    // pub fn min_value(&self, other: &Value) -> Option<Value> {
    //     self.bc2(other, |a, b| {
    //         match Self::cmp_atom(a, b)? {
    //                 Ordering::Greater => Some(b.clone()),
    //                 _ /* Less|Equal  */ => Some(a.clone()),
    //             }
    //     })
    // }

    // pub fn max_value(&self, other: &Value) -> Option<Value> {
    //     self.bc2(other, |a, b| {
    //         match Self::cmp_atom(a, b)? {
    //                 Ordering::Less => Some(b.clone()),
    //                 _ /* Greater|Equal */ => Some(a.clone()),
    //             }
    //     })
    // }
}

pub fn type_err1(op: &str, v: &Value) -> WqError {
    WqError::DomainError(format!(
        "`{op}`: cannot operate on {} of type {}",
        v,
        v.type_name_verbose()
    ))
}

pub fn type_err2(op: &str, v1: &Value, v2: &Value) -> WqError {
    WqError::DomainError(format!(
        "`{op}`: cannot operate on {} of type {} and {} of type {}",
        v1,
        v1.type_name_verbose(),
        v2,
        v2.type_name_verbose()
    ))
}

fn zero_neg_pow_err() -> WqResult<Value> {
    Err(WqError::ZeroDivisionError(
        "`^`: 0.0 cannot be raised to a negative power".into(),
    ))
}

/// Build a signed string in the given radix with optional prefix.
/// Sign always precedes the prefix.
fn to_radix_string(n: i64, base: u32, with_prefix: bool, prefix: &str) -> String {
    let neg = n < 0;
    // Use i128 to avoid overflow on i64::MIN when taking the absolute value.
    let mag_i128: i128 = if neg { -(n as i128) } else { n as i128 };

    let digits = match base {
        16 => format!("{mag_i128:x}",),
        8 => format!("{mag_i128:o}",),
        2 => format!("{mag_i128:b}",),
        _ => unreachable!("to_radix_string only used for base 2, 8 and 16"),
    };

    match (neg, with_prefix) {
        (true, true) => format!("-{prefix}{digits}"),
        (true, false) => format!("-{digits}"),
        (false, true) => format!("{prefix}{digits}"),
        (false, false) => digits,
    }
}

#[cfg(test)]
mod tests {
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
    fn ord_char_and_symbol() {
        assert_eq!(Value::Char('A').ord().unwrap(), Value::Int(65));
        assert_eq!(
            Value::Symbol("A😀".into()).ord().unwrap(),
            Value::IntList(vec![65, 0x1F600])
        );
    }
}
