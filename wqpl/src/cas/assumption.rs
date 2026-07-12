use std::cmp::Ordering;

use num_traits::{Signed as _, Zero as _};

use super::{cas_err, numeric_is_negative, numeric_is_zero};
use crate::value::cas::{CasConst, CasOp, CasPredicate};
use crate::value::{Value, WqResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Truth {
    Proven,
    Refuted,
    Unknown,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CasAssumptions {
    facts: Vec<CasPredicate>,
}

impl CasAssumptions {
    pub(crate) fn from_value(value: &Value) -> WqResult<Self> {
        let mut assumptions = Self::default();
        assumptions.push_value(value)?;
        Ok(assumptions)
    }

    pub(crate) fn with_predicate(&self, predicate: CasPredicate) -> WqResult<Self> {
        let mut assumptions = self.clone();
        assumptions.push_predicate(predicate)?;
        Ok(assumptions)
    }

    fn push_value(&mut self, value: &Value) -> WqResult<()> {
        if let Value::List(items) = value {
            for item in items.iter() {
                self.push_value(item)?;
            }
            return Ok(());
        }

        let predicate = if let Some(predicate) = value.cas_predicate() {
            predicate.clone()
        } else if let Some((lhs, rhs)) = value.cas_eq_parts() {
            if numeric_is_zero(lhs) {
                CasPredicate::Zero(rhs.clone())
            } else if numeric_is_zero(rhs) {
                CasPredicate::Zero(lhs.clone())
            } else {
                return Err(cas_err(
                    "CAS assumptions currently accept zero equations and symbolic predicates",
                )
                .got1(value));
            }
        } else {
            return Err(
                cas_err("CAS assumptions expect a condition or a list of conditions").got1(value),
            );
        };
        self.push_predicate(predicate)
    }

    fn push_predicate(&mut self, predicate: CasPredicate) -> WqResult<()> {
        let truth = self.prove_predicate(&predicate);
        if truth == Truth::Refuted {
            return Err(cas_err(format!(
                "contradictory CAS assumption {}[{}]",
                predicate.name(),
                predicate.expr()
            )));
        }
        if truth == Truth::Unknown {
            self.facts.push(predicate);
        }
        Ok(())
    }

    pub(crate) fn prove_predicate(&self, predicate: &CasPredicate) -> Truth {
        match predicate {
            CasPredicate::Zero(expr) => self.prove_zero(expr),
            CasPredicate::NonZero(expr) => invert(self.prove_zero(expr)),
            CasPredicate::Positive(expr) => self.prove_positive(expr),
            CasPredicate::Negative(expr) => self.prove_negative(expr),
            CasPredicate::NonNegative(expr) => self.prove_nonnegative(expr),
            CasPredicate::Real(expr) => self.prove_real(expr),
            CasPredicate::Integer(expr) => self.prove_integer(expr),
        }
    }

    pub(crate) fn prove_zero(&self, value: &Value) -> Truth {
        if is_known_numeric(value) {
            return match known_numeric_sign(value) {
                Some(Ordering::Equal) => Truth::Proven,
                Some(Ordering::Less | Ordering::Greater) => Truth::Refuted,
                None => Truth::Unknown,
            };
        }
        if let Some(truth) = self.direct_zero_fact(value) {
            return truth;
        }
        if let Some((CasOp::Multiply, factors)) = value.cas_op_parts() {
            let mut all_nonzero = true;
            for factor in factors {
                match self.prove_zero(factor) {
                    Truth::Proven => return Truth::Proven,
                    Truth::Refuted => {}
                    Truth::Unknown => all_nonzero = false,
                }
            }
            return if all_nonzero {
                Truth::Refuted
            } else {
                Truth::Unknown
            };
        }
        if let Some((CasOp::Power, [base, exponent])) = value.cas_op_parts()
            && let Some(exponent) = exponent.exact_int()
        {
            if exponent.is_zero() {
                return Truth::Refuted;
            }
            return match self.prove_zero(base) {
                Truth::Proven if exponent.is_positive() => Truth::Proven,
                Truth::Refuted => Truth::Refuted,
                _ => Truth::Unknown,
            };
        }
        Truth::Unknown
    }

    pub(crate) fn prove_positive(&self, value: &Value) -> Truth {
        if let Some(sign) = known_numeric_sign(value) {
            return truth_for_ordering(sign, Ordering::Greater);
        }
        if let Some(truth) = self.direct_sign_fact(value, SignQuery::Positive) {
            return truth;
        }
        if self.direct_sign_fact(value, SignQuery::NonNegative) == Some(Truth::Proven)
            && self.prove_zero(value) == Truth::Refuted
        {
            return Truth::Proven;
        }
        if let Some((CasOp::Multiply, factors)) = value.cas_op_parts() {
            return self.product_sign(factors, false);
        }
        if let Some((CasOp::Power, [base, exponent])) = value.cas_op_parts()
            && let Some(exponent) = exponent.exact_int()
        {
            if exponent.is_zero() {
                return Truth::Proven;
            }
            if (&exponent % 2u8).is_zero() {
                return if self.prove_real(base) == Truth::Proven {
                    invert(self.prove_zero(base))
                } else {
                    Truth::Unknown
                };
            }
            return self.prove_positive(base);
        }
        Truth::Unknown
    }

    pub(crate) fn prove_negative(&self, value: &Value) -> Truth {
        if let Some(sign) = known_numeric_sign(value) {
            return truth_for_ordering(sign, Ordering::Less);
        }
        if let Some(truth) = self.direct_sign_fact(value, SignQuery::Negative) {
            return truth;
        }
        if let Some((CasOp::Multiply, factors)) = value.cas_op_parts() {
            return self.product_sign(factors, true);
        }
        if let Some((CasOp::Power, [base, exponent])) = value.cas_op_parts()
            && let Some(exponent) = exponent.exact_int()
        {
            if exponent.is_zero() {
                return Truth::Refuted;
            }
            if (&exponent % 2u8).is_zero() {
                return if self.prove_real(base) == Truth::Proven {
                    Truth::Refuted
                } else {
                    Truth::Unknown
                };
            }
            return self.prove_negative(base);
        }
        Truth::Unknown
    }

    pub(crate) fn prove_nonnegative(&self, value: &Value) -> Truth {
        if let Some(sign) = known_numeric_sign(value) {
            return if sign == Ordering::Less {
                Truth::Refuted
            } else {
                Truth::Proven
            };
        }
        if let Some(truth) = self.direct_sign_fact(value, SignQuery::NonNegative) {
            return truth;
        }
        if let Some((CasOp::Power, [base, exponent])) = value.cas_op_parts()
            && exponent
                .exact_int()
                .is_some_and(|exponent| (&exponent % 2u8).is_zero())
            && self.prove_real(base) == Truth::Proven
        {
            return Truth::Proven;
        }
        match (self.prove_positive(value), self.prove_zero(value)) {
            (Truth::Proven, _) | (_, Truth::Proven) => Truth::Proven,
            (Truth::Refuted, Truth::Refuted) => Truth::Refuted,
            _ => Truth::Unknown,
        }
    }

    pub(crate) fn prove_real(&self, value: &Value) -> Truth {
        if is_known_numeric(value) {
            return match value {
                Value::Complex(value) if value.im != 0.0 => Truth::Refuted,
                _ => Truth::Proven,
            };
        }
        if matches!(
            value.cas_const(),
            Some(CasConst::Pi | CasConst::E | CasConst::Infinity | CasConst::NegInfinity)
        ) {
            return Truth::Proven;
        }
        for fact in self.facts.iter().rev() {
            match fact {
                CasPredicate::Real(expr)
                | CasPredicate::Integer(expr)
                | CasPredicate::Positive(expr)
                | CasPredicate::Negative(expr)
                | CasPredicate::NonNegative(expr)
                | CasPredicate::Zero(expr)
                    if expr == value =>
                {
                    return Truth::Proven;
                }
                _ => {}
            }
        }
        if let Some((op, args)) = value.cas_op_parts()
            && matches!(op, CasOp::Add | CasOp::Multiply)
        {
            return all_truth(args.iter().map(|arg| self.prove_real(arg)));
        }
        if let Some((CasOp::Power, [base, exponent])) = value.cas_op_parts()
            && let Some(exponent) = exponent.exact_int()
            && self.prove_real(base) == Truth::Proven
            && (!exponent.is_negative() || self.prove_zero(base) == Truth::Refuted)
        {
            return Truth::Proven;
        }
        Truth::Unknown
    }

    pub(crate) fn prove_integer(&self, value: &Value) -> Truth {
        if value.exact_int().is_some() {
            return Truth::Proven;
        }
        if is_known_numeric(value) {
            return Truth::Refuted;
        }
        for fact in self.facts.iter().rev() {
            if matches!(fact, CasPredicate::Integer(expr) | CasPredicate::Zero(expr) if expr == value)
            {
                return Truth::Proven;
            }
        }
        if let Some((op, args)) = value.cas_op_parts()
            && matches!(op, CasOp::Add | CasOp::Multiply)
        {
            return all_truth(args.iter().map(|arg| self.prove_integer(arg)));
        }
        if let Some((CasOp::Power, [base, exponent])) = value.cas_op_parts()
            && exponent
                .exact_int()
                .is_some_and(|value| !value.is_negative())
            && self.prove_integer(base) == Truth::Proven
        {
            return Truth::Proven;
        }
        Truth::Unknown
    }

    fn direct_zero_fact(&self, value: &Value) -> Option<Truth> {
        for fact in self.facts.iter().rev() {
            match fact {
                CasPredicate::Zero(expr) if expr == value => return Some(Truth::Proven),
                CasPredicate::NonZero(expr)
                | CasPredicate::Positive(expr)
                | CasPredicate::Negative(expr)
                    if expr == value =>
                {
                    return Some(Truth::Refuted);
                }
                _ => {}
            }
        }
        None
    }

    fn direct_sign_fact(&self, value: &Value, query: SignQuery) -> Option<Truth> {
        for fact in self.facts.iter().rev() {
            let truth = match (query, fact) {
                (SignQuery::Positive, CasPredicate::Positive(expr)) if expr == value => {
                    Truth::Proven
                }
                (SignQuery::Positive, CasPredicate::Zero(expr) | CasPredicate::Negative(expr))
                    if expr == value =>
                {
                    Truth::Refuted
                }
                (SignQuery::Negative, CasPredicate::Negative(expr)) if expr == value => {
                    Truth::Proven
                }
                (
                    SignQuery::Negative,
                    CasPredicate::Zero(expr)
                    | CasPredicate::Positive(expr)
                    | CasPredicate::NonNegative(expr),
                ) if expr == value => Truth::Refuted,
                (
                    SignQuery::NonNegative,
                    CasPredicate::NonNegative(expr)
                    | CasPredicate::Positive(expr)
                    | CasPredicate::Zero(expr),
                ) if expr == value => Truth::Proven,
                (SignQuery::NonNegative, CasPredicate::Negative(expr)) if expr == value => {
                    Truth::Refuted
                }
                _ => continue,
            };
            return Some(truth);
        }
        None
    }

    fn product_sign(&self, factors: &[Value], want_negative: bool) -> Truth {
        let mut negative = false;
        for factor in factors {
            if self.prove_positive(factor) == Truth::Proven {
                continue;
            }
            if self.prove_negative(factor) == Truth::Proven {
                negative = !negative;
                continue;
            }
            if self.prove_zero(factor) == Truth::Proven {
                return Truth::Refuted;
            }
            return Truth::Unknown;
        }
        if negative == want_negative {
            Truth::Proven
        } else {
            Truth::Refuted
        }
    }
}

#[derive(Clone, Copy)]
enum SignQuery {
    Positive,
    Negative,
    NonNegative,
}

fn invert(truth: Truth) -> Truth {
    match truth {
        Truth::Proven => Truth::Refuted,
        Truth::Refuted => Truth::Proven,
        Truth::Unknown => Truth::Unknown,
    }
}

fn truth_for_ordering(actual: Ordering, expected: Ordering) -> Truth {
    if actual == expected {
        Truth::Proven
    } else {
        Truth::Refuted
    }
}

fn all_truth(values: impl Iterator<Item = Truth>) -> Truth {
    let mut unknown = false;
    for value in values {
        match value {
            Truth::Proven => {}
            Truth::Refuted => return Truth::Refuted,
            Truth::Unknown => unknown = true,
        }
    }
    if unknown {
        Truth::Unknown
    } else {
        Truth::Proven
    }
}

fn known_numeric_sign(value: &Value) -> Option<Ordering> {
    if let Value::Float(value) = value {
        return value.0.partial_cmp(&0.0);
    }
    if let Value::Complex(value) = value {
        if value.im != 0.0 || value.re.is_nan() {
            return None;
        }
        return value.re.partial_cmp(&0.0);
    }
    if numeric_is_zero(value) {
        Some(Ordering::Equal)
    } else if numeric_is_negative(value) {
        Some(Ordering::Less)
    } else if is_known_numeric(value) {
        Some(Ordering::Greater)
    } else {
        None
    }
}

fn is_known_numeric(value: &Value) -> bool {
    matches!(
        value,
        Value::Int(_)
            | Value::BigInt(_)
            | Value::Float(_)
            | Value::Complex(_)
            | Value::Fraction(_)
            | Value::Algebraic(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cas::{cas_mul, cas_pow};

    #[test]
    fn nonzero_fact_refutes_zero() {
        let a = Value::from_cas_var("a");
        let assumptions = CasAssumptions::from_value(&Value::from_cas_nonzero(a.clone()))
            .expect("valid assumption");
        assert_eq!(assumptions.prove_zero(&a), Truth::Refuted);
    }

    #[test]
    fn zero_equation_proves_zero() {
        let a = Value::from_cas_var("a");
        let assumptions = CasAssumptions::from_value(&Value::from_cas_eq(a.clone(), Value::Int(0)))
            .expect("valid assumption");
        assert_eq!(assumptions.prove_zero(&a), Truth::Proven);
    }

    #[test]
    fn nonzero_product_is_derived_from_factors() {
        let a = Value::from_cas_var("a");
        let b = Value::from_cas_var("b");
        let facts = Value::List(std::sync::Arc::new(vec![
            Value::from_cas_nonzero(a.clone()),
            Value::from_cas_nonzero(b.clone()),
        ]));
        let assumptions = CasAssumptions::from_value(&facts).expect("valid assumptions");
        let product = cas_mul(vec![a, b]).expect("product");
        assert_eq!(assumptions.prove_zero(&product), Truth::Refuted);
    }

    #[test]
    fn even_power_of_nonzero_real_is_positive() {
        let a = Value::from_cas_var("a");
        let facts = Value::List(std::sync::Arc::new(vec![
            Value::from_cas_predicate(CasPredicate::Real(a.clone())),
            Value::from_cas_nonzero(a.clone()),
        ]));
        let assumptions = CasAssumptions::from_value(&facts).expect("valid assumptions");
        let square = cas_pow(a, Value::Int(2)).expect("square");
        assert_eq!(assumptions.prove_positive(&square), Truth::Proven);
    }

    #[test]
    fn even_power_of_real_is_nonnegative_without_a_nonzero_fact() {
        let a = Value::from_cas_var("a");
        let assumptions =
            CasAssumptions::from_value(&Value::from_cas_predicate(CasPredicate::Real(a.clone())))
                .expect("valid assumption");
        let square = cas_pow(a, Value::Int(2)).expect("square");
        assert_eq!(assumptions.prove_nonnegative(&square), Truth::Proven);
    }

    #[test]
    fn nonnegative_and_nonzero_imply_positive() {
        let a = Value::from_cas_var("a");
        let facts = Value::List(std::sync::Arc::new(vec![
            Value::from_cas_predicate(CasPredicate::NonNegative(a.clone())),
            Value::from_cas_nonzero(a.clone()),
        ]));
        let assumptions = CasAssumptions::from_value(&facts).expect("valid assumptions");
        assert_eq!(assumptions.prove_positive(&a), Truth::Proven);
    }

    #[test]
    fn contradictory_facts_are_rejected() {
        let a = Value::from_cas_var("a");
        let facts = Value::List(std::sync::Arc::new(vec![
            Value::from_cas_nonzero(a.clone()),
            Value::from_cas_eq(a, Value::Int(0)),
        ]));
        assert!(CasAssumptions::from_value(&facts).is_err());
    }

    #[test]
    fn nonnegative_and_negative_facts_are_contradictory_in_either_order() {
        let a = Value::from_cas_var("a");
        for facts in [
            vec![
                CasPredicate::NonNegative(a.clone()),
                CasPredicate::Negative(a.clone()),
            ],
            vec![
                CasPredicate::Negative(a.clone()),
                CasPredicate::NonNegative(a.clone()),
            ],
        ] {
            let facts = Value::List(std::sync::Arc::new(
                facts.into_iter().map(Value::from_cas_predicate).collect(),
            ));
            assert!(CasAssumptions::from_value(&facts).is_err());
        }
    }
}
