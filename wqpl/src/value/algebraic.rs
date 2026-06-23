use std::fmt::{self, Write as _};
use std::sync::Arc;

use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};
use ordered_float::OrderedFloat;

use crate::value::{Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

/// Base coefficient field for an algebraic extension.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum AlgebraicBase {
    Rational,
    #[allow(dead_code)]
    Extension(Arc<AlgebraicField>),
}

impl AlgebraicBase {
    fn push_canonical_key(&self, out: &mut String) {
        match self {
            Self::Rational => out.push('Q'),
            Self::Extension(field) => {
                out.push_str("E(");
                field.push_canonical_key(out);
                out.push(')');
            }
        }
    }
}

/// Root identity for an algebraic generator.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum AlgebraicRoot {
    RealInterval {
        lo: OrderedFloat<f64>,
        hi: OrderedFloat<f64>,
    },
}

impl AlgebraicRoot {
    pub(crate) fn interval(&self) -> (f64, f64) {
        match self {
            Self::RealInterval { lo, hi } => (**lo, **hi),
        }
    }
}

/// Field descriptor for a simple algebraic extension K(α).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct AlgebraicField {
    base: AlgebraicBase,
    /// Primitive positive-leading integer polynomial:
    /// poly[0] + poly[1]·x + ... + poly[n]·x^n.
    poly: Arc<[BigInt]>,
    /// Root identity for the generator α.
    root: AlgebraicRoot,
}

impl AlgebraicField {
    pub(crate) fn new_real_root(poly: Vec<BigInt>, interval: (f64, f64)) -> WqResult<Arc<Self>> {
        Self::new_real_root_over(AlgebraicBase::Rational, poly, interval)
    }

    pub(crate) fn new_real_root_over(
        base: AlgebraicBase,
        poly: Vec<BigInt>,
        interval: (f64, f64),
    ) -> WqResult<Arc<Self>> {
        let poly = normalize_field_poly(poly)?;
        validate_real_interval(&poly, interval)?;
        let field = Arc::new(Self {
            base,
            poly: Arc::from(poly),
            root: AlgebraicRoot::RealInterval {
                lo: OrderedFloat(interval.0),
                hi: OrderedFloat(interval.1),
            },
        });
        debug_assert!(
            field.validate_invariants().is_ok(),
            "constructed algebraic field violates invariants"
        );
        Ok(field)
    }

    pub(crate) fn degree(&self) -> usize {
        self.poly.len().saturating_sub(1)
    }

    pub(crate) fn poly(&self) -> &[BigInt] {
        &self.poly
    }

    pub(crate) fn interval(&self) -> (f64, f64) {
        self.root.interval()
    }

    pub(crate) fn validate_invariants(&self) -> WqResult<()> {
        validate_normalized_field_poly(&self.poly)?;
        validate_real_interval(&self.poly, self.interval())?;
        if let AlgebraicBase::Extension(base) = &self.base {
            base.validate_invariants()?;
        }
        Ok(())
    }

    pub(crate) fn push_canonical_key(&self, out: &mut String) {
        out.push_str("field(base:");
        self.base.push_canonical_key(out);
        out.push_str(";poly:");
        for coeff in self.poly.iter() {
            write!(out, "{coeff};").expect("writing to String should not fail");
        }
        match &self.root {
            AlgebraicRoot::RealInterval { lo, hi } => {
                write!(
                    out,
                    "root:R:{:016x}:{:016x}",
                    (**lo).to_bits(),
                    (**hi).to_bits()
                )
                .expect("writing to String should not fail");
            }
        }
        out.push(')');
    }
}

/// Proven sign of an exact numeric value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumericSign {
    Negative,
    Zero,
    Positive,
    Unknown,
}

/// An element of a simple algebraic extension K(α).
///
/// Invariants:
/// 1. `field` identifies the base field, normalized polynomial, and chosen
///    root.
/// 2. `coeffs.len() <= field.degree()` and represents c0 + c1·α + ... +
///    c{d-1}·α^{d-1}.
/// 3. Coefficients are exact scalars in the field base.
#[derive(Debug, Clone)]
pub struct AlgebraicData {
    pub(crate) field: Arc<AlgebraicField>,
    /// Coefficients in the basis {1, α, α², ..., α^{d-1}}.
    pub(crate) coeffs: Arc<[Value]>,
}

impl AlgebraicData {
    pub(crate) fn new(field: Arc<AlgebraicField>, mut coeffs: Vec<Value>) -> WqResult<Self> {
        if coeffs.len() > field.degree() {
            return Err(algebraic_err(
                "algebraic coefficient degree exceeds field degree",
            ));
        }
        if coeffs.is_empty() {
            coeffs.push(Value::Int(0));
        }
        while coeffs.len() > 1 && coeffs.last().is_some_and(crate::cas::numeric_is_zero) {
            coeffs.pop();
        }
        for coeff in &coeffs {
            validate_coeff_in_base(&field, coeff)?;
        }
        let data = Self {
            field,
            coeffs: Arc::from(coeffs),
        };
        debug_assert!(
            data.validate_invariants().is_ok(),
            "constructed algebraic data violates invariants"
        );
        Ok(data)
    }

    pub(crate) fn value(field: Arc<AlgebraicField>, coeffs: Vec<Value>) -> WqResult<Value> {
        Ok(Value::Algebraic(Arc::new(Self::new(field, coeffs)?)))
    }

    pub(crate) fn generator(field: Arc<AlgebraicField>) -> WqResult<Value> {
        Self::value(field, vec![Value::Int(0), Value::Int(1)])
    }

    pub(crate) fn zero(field: Arc<AlgebraicField>) -> WqResult<Value> {
        Self::value(field, vec![Value::Int(0)])
    }

    pub(crate) fn one(field: Arc<AlgebraicField>) -> WqResult<Value> {
        Self::value(field, vec![Value::Int(1)])
    }

    pub(crate) fn constant(field: Arc<AlgebraicField>, value: Value) -> WqResult<Value> {
        Self::value(field, vec![value])
    }

    pub(crate) fn field(&self) -> &Arc<AlgebraicField> {
        &self.field
    }

    pub(crate) fn poly(&self) -> &[BigInt] {
        self.field.poly()
    }

    pub(crate) fn interval(&self) -> (f64, f64) {
        self.field.interval()
    }

    pub(crate) fn degree(&self) -> usize {
        self.field.degree()
    }

    pub(crate) fn validate_invariants(&self) -> WqResult<()> {
        self.field.validate_invariants()?;
        if self.coeffs.is_empty() {
            return Err(algebraic_err(
                "algebraic coefficient vector must not be empty",
            ));
        }
        if self.coeffs.len() > self.field.degree() {
            return Err(algebraic_err(
                "algebraic coefficient degree exceeds field degree",
            ));
        }
        if self.coeffs.len() > 1
            && self
                .coeffs
                .last()
                .is_some_and(crate::cas::numeric_is_zero)
        {
            return Err(algebraic_err(
                "algebraic coefficient vector must be trimmed",
            ));
        }
        for coeff in self.coeffs.iter() {
            validate_coeff_in_base(&self.field, coeff)?;
            if let Value::Algebraic(coeff_data) = coeff {
                coeff_data.validate_invariants()?;
            }
        }
        Ok(())
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.coeffs.iter().all(crate::cas::numeric_is_zero)
    }

    pub(crate) fn is_one(&self) -> bool {
        if self.coeffs.is_empty() {
            return false;
        }
        crate::cas::numeric_is_one(&self.coeffs[0])
            && self.coeffs[1..].iter().all(crate::cas::numeric_is_zero)
    }

    pub(crate) fn same_field(&self, other: &Self) -> bool {
        self.field == other.field
    }

    pub(crate) fn sign(&self) -> NumericSign {
        if self.is_zero() {
            return NumericSign::Zero;
        }

        let (lo, hi) = self.interval();
        if lo > 0.0 {
            // Pure constant (only c0 non-zero) – sign is c0's sign.
            if !self.coeffs.is_empty() && !crate::cas::numeric_is_zero(&self.coeffs[0]) {
                let all_higher_zero = self.coeffs[1..].iter().all(crate::cas::numeric_is_zero);
                if all_higher_zero {
                    return numeric_sign_of_scalar(&self.coeffs[0]);
                }
            }

            // Single non-constant term c·α^k.
            // Since α > 0, α^k > 0 for all k, so the sign is c's sign.
            let non_zero_indices: Vec<usize> = self
                .coeffs
                .iter()
                .enumerate()
                .filter(|(_, c)| !crate::cas::numeric_is_zero(c))
                .map(|(i, _)| i)
                .collect();

            if non_zero_indices.len() == 1 {
                return numeric_sign_of_scalar(&self.coeffs[non_zero_indices[0]]);
            }
        }

        if hi < 0.0 {
            let non_zero_indices: Vec<usize> = self
                .coeffs
                .iter()
                .enumerate()
                .filter(|(_, c)| !crate::cas::numeric_is_zero(c))
                .map(|(i, _)| i)
                .collect();

            if non_zero_indices.len() == 1 {
                let idx = non_zero_indices[0];
                let coeff_sign = numeric_sign_of_scalar(&self.coeffs[idx]);
                return if idx.is_multiple_of(2) {
                    coeff_sign
                } else {
                    negate_sign(coeff_sign)
                };
            }
        }

        NumericSign::Unknown
    }

    /// Determine whether this algebraic number is certainly negative.
    pub(crate) fn is_negative(&self) -> bool {
        self.sign() == NumericSign::Negative
    }
}

impl Value {
    pub(crate) fn is_algebraic_number(&self) -> bool {
        matches!(self, Value::Algebraic(_))
    }

    /// Unwrap a constant Algebraic value to its scalar coefficient.
    /// e.g. `Algebraic([-2])` in Q(∛2) → `Int(-2)`.
    pub(crate) fn unwrap_algebraic_constant(&self) -> Value {
        if let Value::Algebraic(a) = self
            && !a.coeffs.is_empty()
            && a.coeffs[1..].iter().all(crate::cas::numeric_is_zero)
        {
            a.coeffs[0].clone()
        } else {
            self.clone()
        }
    }
}

/// Euclidean GCD for BigInt.
fn bigint_gcd(a: &BigInt, b: &BigInt) -> BigInt {
    let mut a = a.clone();
    let mut b = b.clone();
    while b != BigInt::zero() {
        let r = &a % &b;
        a = b;
        b = r;
    }
    a
}

fn normalize_field_poly(mut poly: Vec<BigInt>) -> WqResult<Vec<BigInt>> {
    while poly.last().is_some_and(BigInt::is_zero) {
        poly.pop();
    }
    if poly.len() < 3 {
        return Err(algebraic_err(
            "algebraic field polynomial must have degree at least 2",
        ));
    }

    let mut content = BigInt::zero();
    for coeff in &poly {
        content = bigint_gcd(&content, &coeff.abs());
    }
    if content.is_zero() {
        return Err(algebraic_err("algebraic field polynomial cannot be zero"));
    }
    if !content.is_one() {
        for coeff in &mut poly {
            *coeff /= &content;
        }
    }

    if poly.last().is_some_and(BigInt::is_negative) {
        for coeff in &mut poly {
            *coeff = -coeff.clone();
        }
    }

    Ok(poly)
}

fn validate_normalized_field_poly(poly: &[BigInt]) -> WqResult<()> {
    if poly.len() < 3 {
        return Err(algebraic_err(
            "algebraic field polynomial must have degree at least 2",
        ));
    }
    if poly.last().is_some_and(BigInt::is_zero) {
        return Err(algebraic_err(
            "algebraic field polynomial must be trimmed",
        ));
    }
    if !poly.last().is_some_and(BigInt::is_positive) {
        return Err(algebraic_err(
            "algebraic field polynomial must have positive leading coefficient",
        ));
    }

    let mut content = BigInt::zero();
    for coeff in poly {
        content = bigint_gcd(&content, &coeff.abs());
    }
    if content != BigInt::one() {
        return Err(algebraic_err(
            "algebraic field polynomial must be primitive",
        ));
    }

    Ok(())
}

fn eval_field_poly_f64(poly: &[BigInt], x: f64) -> Option<f64> {
    let mut acc = 0.0f64;
    for coeff in poly.iter().rev() {
        acc = acc.mul_add(x, coeff.to_f64()?);
    }
    Some(acc)
}

fn validate_real_interval(poly: &[BigInt], interval: (f64, f64)) -> WqResult<()> {
    let (lo, hi) = interval;
    if !lo.is_finite() || !hi.is_finite() || lo >= hi {
        return Err(algebraic_err(
            "algebraic root interval must be finite with lo < hi",
        ));
    }

    let lo_value = eval_field_poly_f64(poly, lo).ok_or_else(|| {
        algebraic_err("algebraic field polynomial is too large to validate interval")
    })?;
    let hi_value = eval_field_poly_f64(poly, hi).ok_or_else(|| {
        algebraic_err("algebraic field polynomial is too large to validate interval")
    })?;
    if !lo_value.is_finite() || !hi_value.is_finite() {
        return Err(algebraic_err(
            "algebraic field polynomial produced a non-finite interval value",
        ));
    }
    if lo_value.is_sign_positive() == hi_value.is_sign_positive()
        && lo_value != 0.0
        && hi_value != 0.0
    {
        return Err(algebraic_err(
            "algebraic root interval must bracket a real root",
        ));
    }
    Ok(())
}

fn validate_coeff_in_base(field: &AlgebraicField, coeff: &Value) -> WqResult<()> {
    match (&field.base, coeff) {
        (_, Value::Int(_) | Value::BigInt(_) | Value::Fraction(_)) => Ok(()),
        (AlgebraicBase::Extension(base), Value::Algebraic(value)) if value.field == *base => Ok(()),
        (AlgebraicBase::Rational, Value::Algebraic(_)) => Err(algebraic_err(
            "algebraic coefficient is not in the rational base field",
        )),
        (AlgebraicBase::Extension(_), Value::Algebraic(_)) => Err(algebraic_err(
            "algebraic coefficient belongs to a different base field",
        )),
        _ => Err(algebraic_err(
            "algebraic coefficient must be an exact scalar in the base field",
        )),
    }
}

fn numeric_sign_of_scalar(value: &Value) -> NumericSign {
    match value {
        Value::Int(n) => match n.cmp(&0) {
            std::cmp::Ordering::Less => NumericSign::Negative,
            std::cmp::Ordering::Equal => NumericSign::Zero,
            std::cmp::Ordering::Greater => NumericSign::Positive,
        },
        Value::BigInt(n) => {
            if n.is_negative() {
                NumericSign::Negative
            } else if n.is_zero() {
                NumericSign::Zero
            } else {
                NumericSign::Positive
            }
        }
        Value::Fraction(fr) => {
            if fr.numer().is_negative() {
                NumericSign::Negative
            } else if fr.numer().is_zero() {
                NumericSign::Zero
            } else {
                NumericSign::Positive
            }
        }
        Value::Algebraic(a) => a.sign(),
        _ => NumericSign::Unknown,
    }
}

fn negate_sign(sign: NumericSign) -> NumericSign {
    match sign {
        NumericSign::Negative => NumericSign::Positive,
        NumericSign::Zero => NumericSign::Zero,
        NumericSign::Positive => NumericSign::Negative,
        NumericSign::Unknown => NumericSign::Unknown,
    }
}

/// Try to recognize the generator of an algebraic field as a common radical.
///
/// Matches pure-power minimal polynomials `cn*x^n + c0 = 0` (giving
/// `(-c0/cn)^(1/n)`) and the golden-ratio polynomial `x^2 - x - 1 = 0`.
fn recognize_radical_name(poly: &[BigInt], interval: (f64, f64)) -> Option<String> {
    let deg = poly.len().saturating_sub(1);
    if deg < 2 {
        return None;
    }

    // Golden ratio: x^2 - x - 1 = 0
    if deg == 2
        && poly[0] == BigInt::from(-1)
        && poly[1] == BigInt::from(-1)
        && poly[2] == BigInt::from(1)
    {
        let mid = (interval.0 + interval.1) * 0.5;
        return Some(if mid > 0.0 {
            "phi".to_string()
        } else {
            "1-phi".to_string()
        });
    }

    // Pure power form: cn*x^n + c0 = 0  ->  x^n = -c0/cn
    let middle_all_zero = poly[1..deg].iter().all(|c| c.is_zero());
    if !middle_all_zero {
        return None;
    }

    let c0 = &poly[0];
    let cn = &poly[deg];

    if c0.is_zero() || cn.is_zero() {
        return None;
    }
    // Need opposite signs for x^n = -c0/cn to have a positive real root.
    if (c0.is_positive() && cn.is_positive()) || (c0.is_negative() && cn.is_negative()) {
        return None;
    }

    let num = c0.abs();
    let den = cn.abs();
    let g = bigint_gcd(&num, &den);
    let num = num / &g;
    let den = den / &g;

    let base = if den.is_one() {
        format!("{num}")
    } else {
        format!("({num}/{den})")
    };

    let radical = format!("{base}^(1/{deg})");

    Some(radical)
}

/// Convert a minimal polynomial to a short human-readable string like
/// `x^3-2*x+1`.
fn poly_to_short_string(poly: &[BigInt]) -> String {
    const SYMBOL: &str = "_";
    let mut parts = Vec::new();
    for (power, c) in poly.iter().enumerate().rev() {
        if c.is_zero() {
            continue;
        }
        let is_first = parts.is_empty();
        let term = match power {
            0 => {
                if is_first {
                    format!("{c}")
                } else {
                    format!("{c:+}")
                }
            }
            1 => match (c, is_first) {
                (c, true) if *c == BigInt::from(1) => SYMBOL.to_string(),
                (c, true) if *c == BigInt::from(-1) => format!("-{SYMBOL}"),
                (c, false) if *c == BigInt::from(1) => format!("+{SYMBOL}"),
                (c, false) if *c == BigInt::from(-1) => format!("-{SYMBOL}"),
                (_, true) => format!("{c}*{SYMBOL}"),
                (_, false) => format!("{c:+}*{SYMBOL}"),
            },
            n => match (c, is_first) {
                (c, true) if *c == BigInt::from(1) => format!("{SYMBOL}^{n}"),
                (c, true) if *c == BigInt::from(-1) => format!("-{SYMBOL}^{n}"),
                (c, false) if *c == BigInt::from(1) => format!("+{SYMBOL}^{n}"),
                (c, false) if *c == BigInt::from(-1) => format!("-{SYMBOL}^{n}"),
                (_, true) => format!("{c}*{SYMBOL}^{n}"),
                (_, false) => format!("{c:+}*{SYMBOL}^{n}"),
            },
        };
        parts.push(term);
    }
    if parts.is_empty() {
        "0".to_string()
    } else {
        parts.concat()
    }
}

/// Format a float as a raw approximate value.
fn format_approx_f64(value: f64) -> String {
    format!("{value}")
}

fn has_top_level_additive_operator(text: &str) -> bool {
    let mut depth = 0usize;
    for (idx, ch) in text.char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            '+' | '-' if depth == 0 && idx > 0 => return true,
            _ => {}
        }
    }
    false
}

/// Human-friendly display for an algebraic number.
///
/// Formats as a linear combination `c0 + c1*name + c2*name^2 + ...` where the
/// generator name is a recognized radical (e.g. `2^(1/2)`) when possible,
/// otherwise a descriptive `root(...)` string.
pub(crate) fn fmt_algebraic_human(a: &AlgebraicData, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let interval = a.interval();
    let name = recognize_radical_name(a.poly(), interval).unwrap_or_else(|| {
        let poly_str = poly_to_short_string(a.poly());
        let alpha_approx = (interval.0 + interval.1) * 0.5;
        let approx_str = format_approx_f64(alpha_approx);
        format!("root({}, {approx_str})", poly_str)
    });

    let name_needs_parens = name
        .chars()
        .any(|c| matches!(c, '+' | '-' | '*' | '/' | '^' | '(' | ')' | ' '));
    let name_has_top_level_add = has_top_level_additive_operator(&name);
    let non_zero_count = a
        .coeffs
        .iter()
        .filter(|c| !crate::cas::numeric_is_zero(c))
        .count();

    let mut first = true;

    for (i, c) in a.coeffs.iter().enumerate() {
        if crate::cas::numeric_is_zero(c) {
            continue;
        }

        let is_negative = crate::cas::numeric_is_negative(c);
        let abs_c = if is_negative {
            c.neg()
                .expect("algebraic coefficient negation should succeed")
        } else {
            c.clone()
        };
        let is_one = crate::cas::numeric_is_one(&abs_c);

        let abs_c_str = abs_c.to_string();
        let coeff_needs_parens = abs_c_str
            .chars()
            .any(|ch| matches!(ch, '+' | '-' | '*' | '/' | '^' | '(' | ')' | ' '));

        if !first {
            write!(f, " ")?;
            if is_negative {
                write!(f, "-")?;
            } else {
                write!(f, "+")?;
            }
            write!(f, " ")?;
        } else if is_negative {
            write!(f, "-")?;
        }
        first = false;

        match i {
            0 => {
                if !is_one {
                    write!(f, "{abs_c}")?;
                } else {
                    write!(f, "1")?;
                }
            }
            1 => {
                if !is_one {
                    if coeff_needs_parens {
                        write!(f, "({abs_c})*")?;
                    } else {
                        write!(f, "{abs_c}*")?;
                    }
                }
                let wrap_name =
                    name_has_top_level_add && (is_negative || !is_one || non_zero_count > 1);
                if wrap_name {
                    write!(f, "({name})")?;
                } else {
                    write!(f, "{name}")?;
                }
            }
            _ => {
                if !is_one {
                    if coeff_needs_parens {
                        write!(f, "({abs_c})*")?;
                    } else {
                        write!(f, "{abs_c}*")?;
                    }
                }
                if name_needs_parens {
                    write!(f, "({name})^{i}")?;
                } else {
                    write!(f, "{name}^{i}")?;
                }
            }
        }
    }

    if first {
        write!(f, "0")?;
    }

    Ok(())
}

fn algebraic_err(msg: &str) -> WqError {
    WqError::new(WqErrorType::Domain).msg(msg.to_string())
}

/// Negate an algebraic number: negate each coefficient.
pub(crate) fn algebraic_neg(a: &AlgebraicData) -> Value {
    let coeffs: Vec<Value> = a
        .coeffs
        .iter()
        .map(|c| {
            c.neg()
                .expect("algebraic coefficient negation should succeed")
        })
        .collect();
    AlgebraicData::value(a.field.clone(), coeffs).expect("negated algebraic should stay in field")
}

/// Add two algebraic numbers in the same field.
pub(crate) fn algebraic_add(a: &AlgebraicData, b: &AlgebraicData) -> WqResult<Value> {
    if !a.same_field(b) {
        return Err(algebraic_err(
            "cannot add algebraic numbers from different fields",
        ));
    }
    let len = a.coeffs.len().max(b.coeffs.len());
    let mut coeffs = Vec::with_capacity(len);
    for i in 0..len {
        let ac = a.coeffs.get(i).unwrap_or(&Value::Int(0));
        let bc = b.coeffs.get(i).unwrap_or(&Value::Int(0));
        coeffs.push(crate::cas::numeric_add(ac, bc)?);
    }
    if coeffs.iter().all(crate::cas::numeric_is_zero) {
        return AlgebraicData::zero(a.field.clone());
    }
    AlgebraicData::value(a.field.clone(), coeffs)
}

/// Subtract two algebraic numbers in the same field.
pub(crate) fn algebraic_sub(a: &AlgebraicData, b: &AlgebraicData) -> WqResult<Value> {
    if !a.same_field(b) {
        return Err(algebraic_err(
            "cannot subtract algebraic numbers from different fields",
        ));
    }
    let len = a.coeffs.len().max(b.coeffs.len());
    let mut coeffs = Vec::with_capacity(len);
    for i in 0..len {
        let ac = a.coeffs.get(i).unwrap_or(&Value::Int(0));
        let bc = b.coeffs.get(i).unwrap_or(&Value::Int(0));
        coeffs.push(crate::cas::numeric_sub(ac, bc)?);
    }
    if coeffs.iter().all(crate::cas::numeric_is_zero) {
        return AlgebraicData::zero(a.field.clone());
    }
    AlgebraicData::value(a.field.clone(), coeffs)
}

/// Multiply two algebraic numbers in the same field: poly_mul then reduce mod
/// minimal polynomial.
pub(crate) fn algebraic_mul(a: &AlgebraicData, b: &AlgebraicData) -> WqResult<Value> {
    if !a.same_field(b) {
        return Err(algebraic_err(
            "cannot multiply algebraic numbers from different fields",
        ));
    }
    // Multiply as polynomials
    let raw = crate::cas::poly_mul(&a.coeffs, &b.coeffs)?;
    // Reduce modulo the minimal polynomial
    let min_poly: Vec<Value> = a
        .poly()
        .iter()
        .map(|c| Value::from_bigint(c.clone()))
        .collect();
    let (_, rem) = crate::cas::poly_divide(&raw, &min_poly)?;
    AlgebraicData::value(a.field.clone(), rem)
}

/// Promote an Int, BigInt, or Fraction value into Q(α) as a constant element.
pub(crate) fn promote_to_algebraic(value: &Value, field: &AlgebraicData) -> WqResult<Value> {
    if let Value::Algebraic(a) = value {
        if a.same_field(field) {
            return Ok(value.clone());
        }
        return Err(algebraic_err(
            "cannot mix algebraic numbers from different fields",
        ));
    }
    // Scalar → constant in K(α)
    AlgebraicData::constant(field.field.clone(), value.clone())
}

/// Extended Euclidean algorithm for polynomials over Q(β).
///
/// Returns `(g, s, t)` such that `s·a + t·b = g = gcd(a, b)`.
/// For irreducible minimal polynomial b and deg(a) < deg(b), g is a non-zero
/// constant and s is the modular inverse of a modulo b.
pub(crate) fn poly_egcd(
    a: &[Value],
    b: &[Value],
) -> WqResult<(Vec<Value>, Vec<Value>, Vec<Value>)> {
    if crate::cas::poly_is_zero(b) {
        return Ok((a.to_vec(), vec![Value::Int(1)], vec![Value::Int(0)]));
    }
    if crate::cas::poly_is_zero(a) {
        return Ok((b.to_vec(), vec![Value::Int(0)], vec![Value::Int(1)]));
    }

    // Ensure deg(r0) >= deg(r1)
    let (mut r0, mut r1) = if crate::cas::poly_degree(a) >= crate::cas::poly_degree(b) {
        (a.to_vec(), b.to_vec())
    } else {
        (b.to_vec(), a.to_vec())
    };
    // Track swap: if we swapped, s and t order also swaps
    let swapped = crate::cas::poly_degree(a) < crate::cas::poly_degree(b);

    let mut s0 = vec![Value::Int(1)];
    let mut s1 = vec![Value::Int(0)];
    let mut t0 = vec![Value::Int(0)];
    let mut t1 = vec![Value::Int(1)];
    if swapped {
        // s corresponds to a, t corresponds to b. When swapped, exchange s and t.
        std::mem::swap(&mut s0, &mut t0);
        std::mem::swap(&mut s1, &mut t1);
    }

    while !crate::cas::poly_is_zero(&r1) {
        let (q, r2) = crate::cas::poly_divide(&r0, &r1)?;

        let qs1 = crate::cas::poly_mul(&q, &s1)?;
        let s2 = crate::cas::poly_sub(&s0, &qs1)?;

        let qt1 = crate::cas::poly_mul(&q, &t1)?;
        let t2 = crate::cas::poly_sub(&t0, &qt1)?;

        r0 = r1;
        r1 = r2;
        s0 = s1;
        s1 = s2;
        t0 = t1;
        t1 = t2;
    }

    // Normalize g to monic
    crate::cas::poly_trim(&mut r0);
    if !r0.is_empty() && !crate::cas::poly_is_zero(&r0) {
        let lc = r0.last().cloned().unwrap_or(Value::Int(1));
        if !crate::cas::numeric_is_one(&lc) {
            let inv_lc = crate::cas::eval_exact_numeric_div(&Value::Int(1), &lc)?;
            r0 = crate::cas::poly_scalar_mul(&r0, &inv_lc)?;
            s0 = crate::cas::poly_scalar_mul(&s0, &inv_lc)?;
            t0 = crate::cas::poly_scalar_mul(&t0, &inv_lc)?;
        }
    }

    Ok((r0, s0, t0))
}

/// Divide two algebraic numbers in the same field: compute modular inverse of
/// b, then multiply a * b^(-1).
pub(crate) fn algebraic_div(a: &AlgebraicData, b: &AlgebraicData) -> WqResult<Value> {
    if !a.same_field(b) {
        return Err(algebraic_err(
            "cannot divide algebraic numbers from different fields",
        ));
    }
    if b.is_zero() {
        return Err(algebraic_err("cannot divide by zero in algebraic field"));
    }

    let min_poly: Vec<Value> = b
        .poly()
        .iter()
        .map(|c| Value::from_bigint(c.clone()))
        .collect();

    let egcd_result = poly_egcd(&b.coeffs, &min_poly);
    let (_g, inv, _t) = egcd_result?;

    let raw = crate::cas::poly_mul(&a.coeffs, &inv)?;
    let (_, rem) = crate::cas::poly_divide(&raw, &min_poly)?;

    AlgebraicData::value(a.field.clone(), rem)
}

/// Raise an algebraic number to an integer power via fast exponentiation.
pub(crate) fn algebraic_pow(a: &AlgebraicData, n: i64) -> WqResult<Value> {
    if n == 0 {
        return AlgebraicData::one(a.field.clone());
    }
    if n < 0 {
        let one = AlgebraicData::new(a.field.clone(), vec![Value::Int(1)])
            .expect("one should be valid in algebraic field");
        let inv = algebraic_div(&one, a)?;
        let inv_a = if let Value::Algebraic(inv_a) = &inv {
            inv_a
        } else {
            unreachable!()
        };
        return algebraic_pow(inv_a, -n);
    }

    let mut n = n as u64;
    let mut base = a.clone();
    let mut result = AlgebraicData::new(a.field.clone(), vec![Value::Int(1)])
        .expect("one should be valid in algebraic field");
    while n > 0 {
        if n & 1 == 1 {
            let prod = algebraic_mul(&result, &base)?;
            result = if let Value::Algebraic(r) = &prod {
                (**r).clone()
            } else {
                unreachable!()
            };
        }
        n >>= 1;
        if n > 0 {
            let sq = algebraic_mul(&base, &base)?;
            base = if let Value::Algebraic(b) = &sq {
                (**b).clone()
            } else {
                unreachable!()
            };
        }
    }
    Ok(Value::Algebraic(Arc::new(result)))
}

/// Compute `a^(numer/denom)` for an algebraic number.
///
/// Handles the common case where `a` is a single-term element `c·α^k` in a
/// pure-power field Q(α) with α^deg = const.  The general case falls back to
/// a symbolic representation.
pub(crate) fn algebraic_rational_pow(
    a: &AlgebraicData,
    numer: &BigInt,
    denom: &BigInt,
) -> WqResult<Value> {
    if denom.is_one() {
        let n = i64::try_from(numer).map_err(|_| {
            WqError::new(WqErrorType::Domain).msg("exponent too large for algebraic pow")
        })?;
        return algebraic_pow(a, n);
    }

    let deg = a.degree();
    // Only handle pure-power fields: poly = c0 + c_deg·x^deg
    let poly = a.poly();
    let is_pure = deg >= 1 && poly[1..deg].iter().all(|c| c.is_zero());
    if !is_pure {
        return Err(algebraic_err(
            "algebraic_rational_pow: field is not pure-power",
        ));
    }

    // Find the single non-zero term in the basis representation
    let non_zero: Vec<(usize, &Value)> = a
        .coeffs
        .iter()
        .enumerate()
        .filter(|(_, c)| !crate::cas::numeric_is_zero(c))
        .collect();

    if non_zero.len() != 1 {
        return Err(algebraic_err(
            "algebraic_rational_pow: not a single-term algebraic",
        ));
    }

    let (k, c_val) = (non_zero[0].0, non_zero[0].1.clone());

    // Compute α-exponent: k * numer / denom
    let k_bi = BigInt::from(k);
    let k_num = &k_bi * numer;
    // Euclidean division: remainder in [0, denom)
    let (alpha_quot, alpha_rem) = euclidean_div_rem(&k_num, denom);
    if !alpha_rem.is_zero() {
        return Err(algebraic_err(
            "algebraic_rational_pow: exponent would require field extension",
        ));
    }

    // Compute c^(numer/denom) with exact rational arithmetic.
    // c_val is a rational coefficient from the algebraic basis.
    let c_pow = rational_pow(&c_val, numer, denom)?;

    // Reduce α^alpha_quot modulo the field relation α^deg = const.
    // const = -c0 / c_deg  (since α^deg = -c0/c_deg)
    let c0 = &poly[0];
    let c_deg = &poly[deg];
    if c_deg.is_zero() {
        return Err(algebraic_err(
            "algebraic_rational_pow: field constant is zero",
        ));
    }
    let (const_num, const_den) = if c_deg.is_negative() {
        (c0.clone(), -c_deg)
    } else {
        (-c0.clone(), c_deg.clone())
    };

    let deg_bi = BigInt::from(deg);
    // euclidean_div_rem returns (quotient, remainder): α^alpha_quot =
    // field_const^quotient * α^remainder
    let (q, r) = euclidean_div_rem(&alpha_quot, &deg_bi);
    // α^alpha_quot = field_const^q * α^r

    // field_const^q = (const_num / const_den)^q, computed exactly.
    let const_factor = rational_integer_pow(&const_num, &const_den, &q);

    let final_coeff = crate::cas::numeric_mul(&c_pow, &const_factor)
        .map_err(|_| algebraic_err("algebraic_rational_pow: failed to multiply coefficients"))?;

    let r_usize = usize::try_from(&r).unwrap_or(0);

    let mut coeffs = vec![Value::Int(0); deg];
    if r_usize < deg {
        coeffs[r_usize] = final_coeff;
    }

    AlgebraicData::value(a.field.clone(), coeffs)
}

/// Euclidean division: returns (quotient, remainder) with remainder in [0,
/// divisor).
fn euclidean_div_rem(a: &BigInt, b: &BigInt) -> (BigInt, BigInt) {
    let q = a / b;
    let r = a % b;
    if r.is_negative() {
        (q - BigInt::one(), r + b)
    } else {
        (q, r)
    }
}

/// Compute `(numer/denom)^exp` exactly, returning an Int or Fraction.
/// `exp` may be negative.
fn rational_integer_pow(numer: &BigInt, denom: &BigInt, exp: &BigInt) -> Value {
    if exp.is_zero() {
        return Value::Int(1);
    }
    if exp.is_negative() {
        let pos_exp = -exp;
        let p = pos_exp.to_u32().unwrap_or(0);
        if p == 0 {
            return Value::float(0.0); // fallback, shouldn't happen
        }
        return Value::from_fraction_parts(denom.pow(p), numer.pow(p));
    }
    let p = exp.to_u32().unwrap_or(0);
    if p == 0 {
        return Value::float(0.0);
    }
    let pow_num = numer.pow(p);
    let pow_den = denom.pow(p);
    if pow_den.is_one() {
        Value::from_bigint(pow_num)
    } else {
        Value::from_fraction_parts(pow_num, pow_den)
    }
}

/// Compute `value^(numer/denom)` exactly for a rational coefficient.
/// Returns the result as an Int, Fraction, or (for non-perfect-powers)
/// Algebraic.
fn rational_pow(value: &Value, numer: &BigInt, denom: &BigInt) -> WqResult<Value> {
    if denom.is_one() {
        // Integer exponent
        if numer.is_zero() {
            return Ok(Value::Int(1));
        }
        if let Some((n, d)) = value.rational_parts() {
            return Ok(rational_integer_pow(&n, &d, numer));
        }
        // Non-rational value (shouldn't happen for our use case)
        let exp = Value::from_bigint(numer.clone());
        return crate::cas::numeric_pow(value, &exp);
    }

    // Fractional exponent: numer/denom
    let denom_u32 = denom
        .to_u32()
        .ok_or_else(|| algebraic_err("rational_pow: denominator too large"))?;

    // Extract rational parts of the value
    let (base_n, base_d) = value
        .rational_parts()
        .ok_or_else(|| algebraic_err("rational_pow: non-rational coefficient"))?;

    // Check if base_n and base_d are perfect denom-th powers
    let root_n = nth_root_bigint(&base_n, denom_u32);
    let root_d = nth_root_bigint(&base_d, denom_u32);

    match (root_n, root_d) {
        (Some(rn), Some(rd)) => {
            // Exact rational result
            if numer.is_negative() {
                let pos_exp = (-numer).to_u32().unwrap_or(0);
                if pos_exp == 0 {
                    return Ok(Value::Int(1));
                }
                Ok(Value::from_fraction_parts(rd.pow(pos_exp), rn.pow(pos_exp)))
            } else if numer.is_zero() {
                Ok(Value::Int(1))
            } else {
                let p = numer.to_u32().unwrap_or(0);
                if p == 0 {
                    return Ok(Value::Int(1));
                }
                let pow_num = rn.pow(p);
                let pow_den = rd.pow(p);
                if pow_den.is_one() {
                    Ok(Value::from_bigint(pow_num))
                } else {
                    Ok(Value::from_fraction_parts(pow_num, pow_den))
                }
            }
        }
        _ => {
            // Non-exact: create an algebraic sqrt for denominator-2 case
            if denom_u32 == 2 {
                // √(base_n/base_d) as an algebraic number Q(√(base_n·base_d))
                let c = &base_n * &base_d;
                let mut poly = vec![BigInt::zero(); 3];
                poly[0] = -c.clone();
                poly[2] = BigInt::one();
                // Compute isolating interval numerically
                let n_f = c.to_f64().unwrap_or(0.0);
                let sqrt_f = n_f.sqrt();
                let eps = sqrt_f.abs().max(1.0) * 1e-12 + 1e-10;
                let iv = (sqrt_f - eps, sqrt_f + eps);
                // √(base_n/base_d) = √(base_n·base_d) / base_d
                let alpha_coeff = if base_d.is_one() {
                    Value::Int(1)
                } else {
                    Value::from_fraction_parts(BigInt::one(), base_d.clone())
                };
                let field = AlgebraicField::new_real_root(poly, iv)?;
                let sqrt_val =
                    AlgebraicData::value(field.clone(), vec![Value::Int(0), alpha_coeff])?;
                if numer.is_negative() {
                    // 1/sqrt
                    if let Value::Algebraic(a) = &sqrt_val {
                        let one = AlgebraicData::new(field, vec![Value::Int(1)])
                            .expect("one should be valid in sqrt field");
                        algebraic_div(&one, a)
                    } else {
                        crate::cas::numeric_div(&Value::Int(1), &sqrt_val)
                    }
                } else if numer.is_one() {
                    Ok(sqrt_val)
                } else {
                    let exp = Value::from_bigint(numer.clone());
                    crate::cas::numeric_pow(&sqrt_val, &exp)
                }
            } else {
                Err(algebraic_err(
                    "rational_pow: non-square-root fractional exponents not implemented",
                ))
            }
        }
    }
}

/// Compute the positive q-th root of a non-negative BigInt.
fn nth_root_bigint(n: &BigInt, q: u32) -> Option<BigInt> {
    if n.is_zero() || n.is_one() {
        return Some(n.clone());
    }
    if n.is_negative() {
        return None;
    }
    let f = n.to_f64()?;
    let root_f = f.powf(1.0 / q as f64);
    let c = root_f.round() as i64;
    for cand in [c - 1, c, c + 1] {
        if cand > 0 || (cand == 0 && q > 0) {
            let cand_bi = BigInt::from(cand);
            if cand_bi.pow(q) == *n {
                return Some(cand_bi);
            }
        }
    }
    None
}

/// Normalize a pure-power algebraic field to a canonical radical form.
///
/// When the minimal polynomial is `c0 + c_deg·x^deg = 0` (only c0 and c_deg
/// non-zero), the generator α satisfies α^deg = -c0/c_deg.  This function
/// extracts perfect deg-th powers from -c0/c_deg and re-expresses α as
/// `scale · β` where β has minimal polynomial `x^deg - r = 0` with r being
/// deg-th-power-free.
///
/// Example: α = ∛(1/108) with poly `[1, 0, 0, -108]` normalizes to
/// α = ⅙·∛2 with poly `[-2, 0, 0, 1]`.
pub(crate) fn normalize_algebraic_field(a: &AlgebraicData) -> Option<AlgebraicData> {
    let deg = a.degree();
    if deg < 2 {
        return None;
    }

    let poly = a.poly();
    // Pure-power form: only poly[0] and poly[deg] non-zero
    if poly[1..deg].iter().any(|c| !c.is_zero()) {
        return None;
    }

    let c0 = &poly[0];
    let c_deg = &poly[deg];

    if c0.is_zero() || c_deg.is_zero() {
        return None;
    }

    // α^deg = -c0/c_deg.  Compute numerator and denominator of -c0/c_deg.
    let (rad_num, rad_den) = if c_deg.is_negative() {
        (c0.clone(), -c_deg)
    } else {
        (-c0.clone(), c_deg.clone())
    };
    if rad_num.is_negative() || rad_den.is_negative() || rad_num.is_zero() {
        return None;
    }

    let q = deg as u32;
    // N = rad_num * rad_den^(q-1)
    let n = &rad_num * &rad_den.pow(q - 1);
    let (p, r) = crate::cas::extract_perfect_power_factor(&n, q);

    // No simplification possible
    if p.is_one() && r == n {
        return None;
    }

    // α = (p / rad_den) · r^(1/deg).  Build scale factor.
    let p_bi = p;
    let g = bigint_gcd(&p_bi, &rad_den);
    let s_num = &p_bi / &g;
    let s_den = &rad_den / &g;
    let scale = if s_den.is_one() {
        Value::from_bigint(s_num)
    } else {
        Value::from_fraction_parts(s_num, s_den)
    };

    // New polynomial: x^deg - r = 0  →  [-r, 0, ..., 0, 1]
    let mut new_poly = vec![BigInt::zero(); deg + 1];
    new_poly[0] = -r;
    new_poly[deg] = BigInt::one();

    // Map coefficients: old c_k·α^k → c_k·scale^k·β^k  (α = scale·β)
    let mut new_coeffs: Vec<Value> = Vec::with_capacity(deg);
    let mut scale_pow = Value::Int(1);
    for old_c in a.coeffs.iter().take(deg) {
        let mapped = if crate::cas::numeric_is_one(&scale_pow) {
            old_c.clone()
        } else if crate::cas::numeric_is_zero(old_c) {
            Value::Int(0)
        } else {
            crate::cas::numeric_mul(old_c, &scale_pow).ok()?
        };
        new_coeffs.push(mapped);
        scale_pow = crate::cas::numeric_mul(&scale_pow, &scale).ok()?;
    }

    // Map isolating interval: lo < α < hi  →  lo/scale < β < hi/scale
    let scale_f = scale.as_f64()?;
    if scale_f <= 0.0 {
        return None;
    }
    let (lo, hi) = a.interval();
    let new_lo = lo / scale_f;
    let new_hi = hi / scale_f;
    let new_interval = if new_lo < new_hi {
        (new_lo, new_hi)
    } else {
        (new_hi, new_lo)
    };

    let field =
        AlgebraicField::new_real_root_over(a.field.base.clone(), new_poly, new_interval).ok()?;
    AlgebraicData::new(field, new_coeffs).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::cas::CasOp;

    fn sqrt2_poly() -> Arc<[BigInt]> {
        Arc::new([
            BigInt::from(-2), // constant term
            BigInt::from(0),  // x coefficient
            BigInt::from(1),  // x² coefficient (monic)
        ])
    }

    fn make_sqrt2() -> AlgebraicData {
        sqrt2_data(vec![Value::Int(0), Value::Int(1)])
    }

    fn make_sqrt2_times_2() -> AlgebraicData {
        sqrt2_data(vec![Value::Int(0), Value::Int(2)])
    }

    fn sqrt2_field(interval: (f64, f64)) -> Arc<AlgebraicField> {
        AlgebraicField::new_real_root(
            vec![BigInt::from(-2), BigInt::zero(), BigInt::one()],
            interval,
        )
        .expect("valid sqrt2 field")
    }

    fn algebraic_data(
        poly: Vec<BigInt>,
        interval: (f64, f64),
        coeffs: Vec<Value>,
    ) -> AlgebraicData {
        let field = AlgebraicField::new_real_root(poly, interval).expect("valid algebraic field");
        AlgebraicData::new(field, coeffs).expect("valid algebraic element")
    }

    fn sqrt2_data(coeffs: Vec<Value>) -> AlgebraicData {
        AlgebraicData::new(sqrt2_field((1.0, 2.0)), coeffs).expect("valid sqrt2 element")
    }

    fn sqrt2_value(coeffs: Vec<Value>) -> Value {
        Value::Algebraic(Arc::new(sqrt2_data(coeffs)))
    }

    #[test]
    fn constructor_normalizes_field_and_trims_coeffs() {
        let field = AlgebraicField::new_real_root(
            vec![BigInt::from(4), BigInt::zero(), BigInt::from(-2)],
            (1.0, 2.0),
        )
        .expect("sign-flipped non-primitive field should normalize");
        assert_eq!(
            field.poly(),
            [BigInt::from(-2), BigInt::zero(), BigInt::one()]
        );

        let value =
            AlgebraicData::new(field, vec![Value::Int(1), Value::Int(0)]).expect("valid element");
        assert_eq!(value.coeffs.as_ref(), [Value::Int(1)]);
        value
            .validate_invariants()
            .expect("constructor output should satisfy invariants");
    }

    #[test]
    fn invariant_validation_rejects_internal_bad_shapes() {
        let non_primitive_field = AlgebraicField {
            base: AlgebraicBase::Rational,
            poly: Arc::from(vec![BigInt::from(-4), BigInt::zero(), BigInt::from(2)]),
            root: AlgebraicRoot::RealInterval {
                lo: OrderedFloat(1.0),
                hi: OrderedFloat(2.0),
            },
        };
        assert!(non_primitive_field.validate_invariants().is_err());

        let field = sqrt2_field((1.0, 2.0));
        let untrimmed = AlgebraicData {
            field: field.clone(),
            coeffs: Arc::from(vec![Value::Int(1), Value::Int(0)]),
        };
        assert!(untrimmed.validate_invariants().is_err());

        let too_long = AlgebraicData {
            field: field.clone(),
            coeffs: Arc::from(vec![Value::Int(0), Value::Int(1), Value::Int(1)]),
        };
        assert!(too_long.validate_invariants().is_err());

        let nested = AlgebraicData {
            field,
            coeffs: Arc::from(vec![Value::Algebraic(Arc::new(make_sqrt2()))]),
        };
        assert!(nested.validate_invariants().is_err());
    }

    #[test]
    fn constructor_rejects_bad_algebraic_data() {
        assert!(
            AlgebraicField::new_real_root(vec![BigInt::from(-2), BigInt::one()], (1.0, 2.0))
                .is_err()
        );
        assert!(AlgebraicField::new_real_root(sqrt2_poly().to_vec(), (2.0, 1.0)).is_err());
        assert!(AlgebraicField::new_real_root(sqrt2_poly().to_vec(), (2.0, 3.0)).is_err());

        let field = sqrt2_field((1.0, 2.0));
        assert!(
            AlgebraicData::new(
                field.clone(),
                vec![Value::Int(0), Value::Int(1), Value::Int(0)]
            )
            .is_err()
        );

        let nested = Value::Algebraic(Arc::new(make_sqrt2()));
        assert!(AlgebraicData::new(field, vec![nested]).is_err());
    }

    #[test]
    fn same_polynomial_different_root_interval_is_not_same_field() {
        let pos = Value::Algebraic(Arc::new(
            AlgebraicData::new(sqrt2_field((1.0, 2.0)), vec![Value::Int(0), Value::Int(1)])
                .expect("valid positive sqrt2"),
        ));
        let neg = Value::Algebraic(Arc::new(
            AlgebraicData::new(
                sqrt2_field((-2.0, -1.0)),
                vec![Value::Int(0), Value::Int(1)],
            )
            .expect("valid negative sqrt2 root"),
        ));

        assert!(pos.add(&neg).is_err());
        assert!(pos.subtract(&neg).is_err());
        assert!(pos.multiply(&neg).is_err());
        assert!(pos.divide(&neg).is_err());
    }

    #[test]
    fn algebraic_rational_pow_preserves_generator_interval() {
        let sqrt2 = AlgebraicData::new(sqrt2_field((1.0, 2.0)), vec![Value::Int(0), Value::Int(1)])
            .expect("valid sqrt2");
        let before = sqrt2.interval();

        let result = algebraic_rational_pow(&sqrt2, &BigInt::from(2), &BigInt::from(2))
            .expect("alpha^(2/2) stays in same field");

        let Value::Algebraic(result) = result else {
            unreachable!("algebraic rational pow returns algebraic here");
        };
        assert_eq!(result.interval(), before);
    }

    #[test]
    fn algebraic_unknown_sign_abs_does_not_return_original_value() {
        let ambiguous = Value::Algebraic(Arc::new(
            AlgebraicData::new(sqrt2_field((1.0, 2.0)), vec![Value::Int(1), Value::Int(-2)])
                .expect("valid multi-term algebraic"),
        ));
        let Value::Algebraic(data) = &ambiguous else {
            unreachable!("constructed algebraic");
        };
        assert_eq!(data.sign(), NumericSign::Unknown);
        assert!(ambiguous.abs().is_err());
    }

    #[test]
    fn algebraic_data_degree() {
        let a = make_sqrt2();
        assert_eq!(a.degree(), 2); // x² - 2 has degree 2
    }

    #[test]
    fn algebraic_data_is_zero() {
        let a = make_sqrt2();
        assert!(!a.is_zero());

        let zero = sqrt2_data(vec![Value::Int(0), Value::Int(0)]);
        assert!(zero.is_zero());
    }

    #[test]
    fn algebraic_data_is_one() {
        let one = sqrt2_data(vec![Value::Int(1), Value::Int(0)]);
        assert!(one.is_one());

        let a = make_sqrt2();
        assert!(!a.is_one());
    }

    #[test]
    fn value_algebraic_type_name() {
        let v = Value::Algebraic(Arc::new(make_sqrt2()));
        assert_eq!(v.type_name(), "algebraic");
    }

    #[test]
    fn value_algebraic_is_atom() {
        let v = Value::Algebraic(Arc::new(make_sqrt2()));
        assert!(v.is_atom());
    }

    #[test]
    fn value_algebraic_eq_same() {
        let a = Value::Algebraic(Arc::new(make_sqrt2()));
        let b = Value::Algebraic(Arc::new(make_sqrt2()));
        assert_eq!(a, b);
    }

    #[test]
    fn value_algebraic_eq_different_coeffs() {
        let a = Value::Algebraic(Arc::new(make_sqrt2()));
        let b = Value::Algebraic(Arc::new(make_sqrt2_times_2()));
        assert_ne!(a, b);
    }

    #[test]
    fn value_algebraic_display() {
        let v = Value::Algebraic(Arc::new(make_sqrt2()));
        assert_eq!(v.to_string(), "2^(1/2)");
    }

    #[test]
    fn value_algebraic_display_neg_sqrt2() {
        let neg_sqrt2 = sqrt2_data(vec![Value::Int(0), Value::Int(-1)]);
        let v = Value::Algebraic(Arc::new(neg_sqrt2));
        assert_eq!(v.to_string(), "-2^(1/2)");
    }

    #[test]
    fn value_algebraic_display_one_plus_sqrt2() {
        let a = sqrt2_data(vec![Value::Int(1), Value::Int(1)]);
        let v = Value::Algebraic(Arc::new(a));
        assert_eq!(v.to_string(), "1 + 2^(1/2)");
    }

    #[test]
    fn value_algebraic_display_fraction_coeff() {
        let a = sqrt2_data(vec![
            Value::Int(0),
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
        ]);
        let v = Value::Algebraic(Arc::new(a));
        assert_eq!(v.to_string(), "(1/2)*2^(1/2)");
    }

    #[test]
    fn value_algebraic_display_cbrt_squared() {
        let cbrt2_poly = vec![
            BigInt::from(-2),
            BigInt::from(0),
            BigInt::from(0),
            BigInt::from(1),
        ];
        let a = algebraic_data(
            cbrt2_poly,
            (1.0, 2.0),
            vec![Value::Int(0), Value::Int(0), Value::Int(1)],
        );
        let v = Value::Algebraic(Arc::new(a));
        assert_eq!(v.to_string(), "(2^(1/3))^2");
    }

    #[test]
    fn value_algebraic_display_unrecognized_poly() {
        // x^3 - x - 1 (not a pure power, so unrecognized)
        let poly = vec![
            BigInt::from(-1),
            BigInt::from(-1),
            BigInt::zero(),
            BigInt::one(),
        ];
        let a = algebraic_data(
            poly,
            (1.0, 2.0),
            vec![Value::Int(0), Value::Int(1), Value::Int(0)],
        );
        let v = Value::Algebraic(Arc::new(a));
        assert_eq!(v.to_string(), "root(_^3-_-1, 1.5)");
    }

    #[test]
    fn value_algebraic_display_phi() {
        let phi_poly = vec![BigInt::from(-1), BigInt::from(-1), BigInt::from(1)];
        let a = algebraic_data(phi_poly, (1.0, 2.0), vec![Value::Int(0), Value::Int(1)]);
        let v = Value::Algebraic(Arc::new(a));
        assert_eq!(v.to_string(), "phi");
    }

    #[test]
    fn value_algebraic_display_one_minus_phi() {
        let phi_poly = vec![BigInt::from(-1), BigInt::from(-1), BigInt::from(1)];
        let a = algebraic_data(phi_poly, (-1.0, 0.0), vec![Value::Int(0), Value::Int(1)]);
        let v = Value::Algebraic(Arc::new(a));
        assert_eq!(v.to_string(), "1-phi");
    }

    #[test]
    fn value_algebraic_display_one_minus_phi_as_term_is_grouped() {
        let phi_poly = vec![BigInt::from(-1), BigInt::from(-1), BigInt::from(1)];
        let double = algebraic_data(
            phi_poly.clone(),
            (-1.0, 0.0),
            vec![Value::Int(0), Value::Int(2)],
        );
        assert_eq!(Value::Algebraic(Arc::new(double)).to_string(), "2*(1-phi)");

        let shifted = algebraic_data(phi_poly, (-1.0, 0.0), vec![Value::Int(3), Value::Int(1)]);
        assert_eq!(
            Value::Algebraic(Arc::new(shifted)).to_string(),
            "3 + (1-phi)",
        );
    }

    #[test]
    fn value_algebraic_display_fraction_constant() {
        let a = sqrt2_data(vec![
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
            Value::Int(1),
        ]);
        let v = Value::Algebraic(Arc::new(a));
        // Constant term 1/2 does not need parens; coefficient 1 on sqrt2 is omitted
        assert_eq!(v.to_string(), "1/2 + 2^(1/2)");
    }

    #[test]
    fn value_algebraic_display_in_cas_product_gets_parens() {
        // Multi-term algebraic inside a CAS multiplication must be parenthesised.
        let a = sqrt2_data(vec![Value::Int(-1), Value::Int(1)]);
        let alg = Value::Algebraic(Arc::new(a));
        let product =
            Value::from_cas_op(CasOp::Multiply, vec![alg.clone(), Value::from_cas_var("x")]);
        assert_eq!(product.to_string(), "(-1 + 2^(1/2))*x");
    }

    #[test]
    fn value_algebraic_display_one_minus_phi_in_cas_product_gets_parens() {
        let phi_poly = vec![BigInt::from(-1), BigInt::from(-1), BigInt::from(1)];
        let a = algebraic_data(phi_poly, (-1.0, 0.0), vec![Value::Int(0), Value::Int(1)]);
        let alg = Value::Algebraic(Arc::new(a));
        let product = Value::from_cas_op(CasOp::Multiply, vec![alg, Value::from_cas_var("x")]);
        assert_eq!(product.to_string(), "(1-phi)*x");
    }

    #[test]
    fn value_algebraic_hash_consistent() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let a = Value::Algebraic(Arc::new(make_sqrt2()));
        let b = Value::Algebraic(Arc::new(make_sqrt2()));
        let mut ha = DefaultHasher::new();
        let mut hb = DefaultHasher::new();
        a.hash(&mut ha);
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    // ── P1a: numeric_is_zero / numeric_is_one / neg ──

    #[test]
    fn numeric_is_zero_with_algebraic() {
        let zero = sqrt2_value(vec![Value::Int(0), Value::Int(0)]);
        assert!(crate::cas::numeric_is_zero(&zero));

        let sqrt2 = Value::Algebraic(Arc::new(make_sqrt2()));
        assert!(!crate::cas::numeric_is_zero(&sqrt2));
    }

    #[test]
    fn numeric_is_one_with_algebraic() {
        let one = sqrt2_value(vec![Value::Int(1), Value::Int(0)]);
        assert!(crate::cas::numeric_is_one(&one));

        let sqrt2 = Value::Algebraic(Arc::new(make_sqrt2()));
        assert!(!crate::cas::numeric_is_one(&sqrt2));
    }

    #[test]
    fn algebraic_neg_sqrt2() {
        let sqrt2 = Value::Algebraic(Arc::new(make_sqrt2()));
        // -√2 = 0 + (-1)·α
        let neg_sqrt2 = sqrt2.neg().unwrap();
        let expected = sqrt2_value(vec![Value::Int(0), Value::Int(-1)]);
        assert_eq!(neg_sqrt2, expected);
    }

    #[test]
    fn algebraic_neg_double_neg() {
        let sqrt2 = Value::Algebraic(Arc::new(make_sqrt2()));
        let neg_neg = sqrt2.neg().unwrap().neg().unwrap();
        assert_eq!(neg_neg, sqrt2);
    }

    // ── P1b: add / sub / mul ──

    #[test]
    fn algebraic_add_sqrt2_plus_sqrt2() {
        // α + α = 2α → coeffs [0, 2]
        let sqrt2 = Value::Algebraic(Arc::new(make_sqrt2()));
        let sum = sqrt2.add(&sqrt2).unwrap();
        let expected = Value::Algebraic(Arc::new(make_sqrt2_times_2()));
        assert_eq!(sum, expected);
    }

    #[test]
    fn algebraic_add_int_plus_sqrt2() {
        // 3 + √2 → coeffs [3, 1]
        let three = Value::Int(3);
        let sqrt2 = Value::Algebraic(Arc::new(make_sqrt2()));
        let sum = three.add(&sqrt2).unwrap();
        let expected = sqrt2_value(vec![Value::Int(3), Value::Int(1)]);
        assert_eq!(sum, expected);
    }

    #[test]
    fn algebraic_add_sqrt2_plus_int() {
        // √2 + 3 → coeffs [3, 1]
        let sqrt2 = Value::Algebraic(Arc::new(make_sqrt2()));
        let three = Value::Int(3);
        let sum = sqrt2.add(&three).unwrap();
        let expected = sqrt2_value(vec![Value::Int(3), Value::Int(1)]);
        assert_eq!(sum, expected);
    }

    #[test]
    fn algebraic_sub_sqrt2_minus_sqrt2() {
        // α - α = 0 → coeffs [0]
        let sqrt2 = Value::Algebraic(Arc::new(make_sqrt2()));
        let diff = sqrt2.subtract(&sqrt2).unwrap();
        let zero = sqrt2_value(vec![Value::Int(0)]);
        assert_eq!(diff, zero);
    }

    #[test]
    fn algebraic_mul_sqrt2_times_sqrt2() {
        // α * α = 2 → coeffs [2]
        let sqrt2 = Value::Algebraic(Arc::new(make_sqrt2()));
        let prod = sqrt2.multiply(&sqrt2).unwrap();
        let expected = sqrt2_value(vec![Value::Int(2)]);
        assert_eq!(prod, expected);
    }

    #[test]
    fn algebraic_mul_sqrt2_times_int() {
        // √2 * 3 → coeffs [0, 3]
        let sqrt2 = Value::Algebraic(Arc::new(make_sqrt2()));
        let three = Value::Int(3);
        let prod = sqrt2.multiply(&three).unwrap();
        let expected = sqrt2_value(vec![Value::Int(0), Value::Int(3)]);
        assert_eq!(prod, expected);
    }

    #[test]
    fn algebraic_mul_two_sqrt2_times_three_sqrt2() {
        // 2√2 * 3√2 = 6·2 = 12 → coeffs [12]
        let two_sqrt2 = Value::Algebraic(Arc::new(make_sqrt2_times_2()));
        let three_sqrt2 = sqrt2_value(vec![Value::Int(0), Value::Int(3)]);
        let prod = two_sqrt2.multiply(&three_sqrt2).unwrap();
        let expected = sqrt2_value(vec![Value::Int(12)]);
        assert_eq!(prod, expected);
    }

    #[test]
    fn algebraic_add_through_numeric_add() {
        let sqrt2 = Value::Algebraic(Arc::new(make_sqrt2()));
        let sum = crate::cas::numeric_add(&sqrt2, &sqrt2).unwrap();
        let expected = Value::Algebraic(Arc::new(make_sqrt2_times_2()));
        assert_eq!(sum, expected);
    }

    // ── P2: poly_egcd / div / pow ──

    #[test]
    fn poly_egcd_simple() {
        // gcd(x^2 - 2, x) = 1 over Q. EGCD should return (g, s, t) with g = [1].
        let a = vec![Value::Int(0), Value::Int(1)]; // x
        let b = vec![
            Value::Int(-2),
            Value::Int(0),
            Value::Int(1), // x^2 - 2
        ];
        let (g, _s, _t) = super::poly_egcd(&a, &b).unwrap();
        // g should be a constant (degree 0), non-zero
        assert_eq!(g.len(), 1);
        assert!(!crate::cas::numeric_is_zero(&g[0]));
    }

    #[test]
    fn algebraic_div_sqrt2_by_itself() {
        // √2 / √2 = 1
        let sqrt2 = Value::Algebraic(Arc::new(make_sqrt2()));
        let quot = sqrt2.divide(&sqrt2).unwrap();
        if let Value::Algebraic(a) = &quot {
            assert!(crate::cas::numeric_is_one(&a.coeffs[0]));
            assert_eq!(a.poly(), sqrt2_poly().as_ref());
        } else {
            panic!("expected algebraic");
        }
    }

    #[test]
    fn algebraic_div_one_by_sqrt2() {
        // 1 / √2 = √2/2 → coeffs [0, 1/2]
        let one = sqrt2_value(vec![Value::Int(1)]);
        let sqrt2 = Value::Algebraic(Arc::new(make_sqrt2()));
        let quot = one.divide(&sqrt2).unwrap();
        // Should be [0, 1/2]
        let expected = sqrt2_value(vec![
            Value::Int(0),
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
        ]);
        assert_eq!(quot, expected);
    }

    #[test]
    fn algebraic_pow_sqrt2_squared() {
        // (√2)^2 = 2
        let sqrt2 = Value::Algebraic(Arc::new(make_sqrt2()));
        let pow = sqrt2.power(&Value::Int(2)).unwrap();
        let expected = sqrt2_value(vec![Value::Int(2)]);
        assert_eq!(pow, expected);
    }

    #[test]
    fn algebraic_pow_sqrt2_cubed() {
        // (√2)^3 = 2√2 → coeffs [0, 2]
        let sqrt2 = Value::Algebraic(Arc::new(make_sqrt2()));
        let pow = sqrt2.power(&Value::Int(3)).unwrap();
        let expected = Value::Algebraic(Arc::new(make_sqrt2_times_2()));
        assert_eq!(pow, expected);
    }

    #[test]
    fn algebraic_div_sqrt2_by_int() {
        // √2 / 3 → coeffs [0, 1/3]
        let sqrt2 = Value::Algebraic(Arc::new(make_sqrt2()));
        let three = Value::Int(3);
        let quot = sqrt2.divide(&three).unwrap();
        let expected = sqrt2_value(vec![
            Value::Int(0),
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(3)),
        ]);
        assert_eq!(quot, expected);
    }

    // ── P3: numeric_is_negative / poly_gcd with algebraic coefficients ──

    #[test]
    fn numeric_is_negative_sqrt2() {
        // √2 ≈ 1.414 > 0
        let sqrt2 = Value::Algebraic(Arc::new(make_sqrt2()));
        assert!(!crate::cas::numeric_is_negative(&sqrt2));
    }

    #[test]
    fn numeric_is_negative_neg_sqrt2() {
        // -√2  (α = √2 in (1,2), value = -1·α)
        let neg_sqrt2 = sqrt2_value(vec![Value::Int(0), Value::Int(-1)]);
        assert!(crate::cas::numeric_is_negative(&neg_sqrt2));
    }

    #[test]
    fn numeric_is_negative_zero() {
        let zero = sqrt2_value(vec![Value::Int(0), Value::Int(0)]);
        assert!(!crate::cas::numeric_is_negative(&zero));
    }

    #[test]
    fn poly_gcd_over_q_sqrt2() {
        // gcd(x² - 2, x - √2) over Q(√2) should be x - √2
        // a = x² - 2 with coeffs in Q(√2): [-2, 0, 1]
        let a: Vec<Value> = vec![
            sqrt2_value(vec![Value::Int(-2)]),
            sqrt2_value(vec![Value::Int(0)]),
            sqrt2_value(vec![Value::Int(1)]),
        ];
        // b = x - √2 with coeffs in Q(√2): [-√2, 1]
        let neg_sqrt2 = sqrt2_value(vec![Value::Int(0), Value::Int(-1)]);
        let b: Vec<Value> = vec![neg_sqrt2, sqrt2_value(vec![Value::Int(1)])];
        let g = crate::cas::poly_gcd(&a, &b).unwrap();
        // g should be degree 1 (x - √2), i.e. monic normalization of b
        assert_eq!(crate::cas::poly_degree(&g), 1);
    }

    // ── P4: tower extension Q(√2, √3) ──

    /// Build the tower Q(√2)(√3): inner β²=2, outer α²=3 with coeffs in Q(β).
    fn make_tower_sqrt3_over_sqrt2() -> (AlgebraicData, AlgebraicData) {
        // Inner field: Q(√2), β² - 2 = 0, β ∈ (1, 2)
        let inner_sqrt2 = make_sqrt2();
        let inner_field = inner_sqrt2.field().clone();

        // Outer minimal polynomial: x² - 3 = 0 with coeffs in Q(√2)
        // [-3, 0, 1] where each coeff is promoted to Q(√2)
        let zero_inner = AlgebraicData::value(inner_field.clone(), vec![Value::Int(0)])
            .expect("valid zero in inner field");
        let one_inner = AlgebraicData::value(inner_field.clone(), vec![Value::Int(1)])
            .expect("valid one in inner field");
        // Outer field element: √3 = 0 + 1·α, where α² = 3
        let outer_field = AlgebraicField::new_real_root_over(
            AlgebraicBase::Extension(inner_field),
            vec![
                BigInt::from(-3), // constant: -3 (integer — the minimal poly coeffs are int)
                BigInt::from(0),  // x coefficient
                BigInt::from(1),  // x² coefficient (monic)
            ],
            (1.0, 2.0),
        )
        .expect("valid outer sqrt3 field");
        let outer_sqrt3 =
            AlgebraicData::new(outer_field, vec![zero_inner.clone(), one_inner.clone()])
                .expect("valid sqrt3 tower element");

        (inner_sqrt2, outer_sqrt3)
    }

    #[test]
    fn tower_add_sqrt3_plus_sqrt3() {
        // √3 + √3 = 2√3 in Q(√2)(√3)
        let (_, sqrt3) = make_tower_sqrt3_over_sqrt2();
        let a = Value::Algebraic(Arc::new(sqrt3.clone()));
        let sum = a.add(&a).unwrap();
        if let Value::Algebraic(s) = &sum {
            assert_eq!(s.field(), sqrt3.field());
            // coeffs should be [0, 2] in Q(√2): i.e., 0 + 2·α
            assert_eq!(s.coeffs.len(), 2);
            assert!(crate::cas::numeric_is_zero(&s.coeffs[0]));
            // s.coeffs[1] is 2 in Q(√2) → constant 2
            if let Value::Algebraic(c1) = &s.coeffs[1] {
                // The constant term should be 2 (as element of Q(√2))
                assert!(!crate::cas::numeric_is_zero(&c1.coeffs[0]));
            }
        } else {
            panic!("expected algebraic");
        }
    }

    #[test]
    fn tower_mul_sqrt3_times_sqrt3() {
        // √3 * √3 = 3 in Q(√2)(√3)
        let (_, sqrt3) = make_tower_sqrt3_over_sqrt2();
        let a = Value::Algebraic(Arc::new(sqrt3.clone()));
        let prod = a.multiply(&a).unwrap();
        if let Value::Algebraic(p) = &prod {
            // Result should be constant 3 in Q(√2), i.e. [3, 0] → trimmed to [3]
            assert_eq!(p.field(), sqrt3.field());
            assert!(p.coeffs.len() <= 2);
            // First coeff should be 3 in Q(√2)
            if let Value::Algebraic(c0) = &p.coeffs[0] {
                // c0 is the value 3 as an element of Q(√2)
                let three = c0.coeffs[0].clone();
                assert_eq!(three, Value::Int(3));
            }
        } else {
            panic!("expected algebraic");
        }
    }

    #[test]
    fn tower_div_sqrt3_by_sqrt3() {
        // √3 / √3 = 1
        let (_, sqrt3) = make_tower_sqrt3_over_sqrt2();
        let a = Value::Algebraic(Arc::new(sqrt3));
        let quot = a.divide(&a).unwrap();
        if let Value::Algebraic(q) = &quot {
            assert!(q.is_one());
        } else {
            panic!("expected algebraic");
        }
    }
}
