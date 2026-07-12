use super::{cas_err, numeric_is_zero};
use crate::value::cas::CasPredicate;
use crate::value::{Value, WqResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Truth {
    Proven,
    Refuted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
enum Assumption {
    Zero(Value),
    NonZero(Value),
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CasAssumptions {
    facts: Vec<Assumption>,
}

impl CasAssumptions {
    pub(crate) fn from_value(value: &Value) -> WqResult<Self> {
        let mut assumptions = Self::default();
        assumptions.push_value(value)?;
        Ok(assumptions)
    }

    fn push_value(&mut self, value: &Value) -> WqResult<()> {
        if let Value::List(items) = value {
            for item in items.iter() {
                self.push_value(item)?;
            }
            return Ok(());
        }

        let fact = if let Some(CasPredicate::NonZero(expr)) = value.cas_predicate() {
            Assumption::NonZero(expr.clone())
        } else if let Some((lhs, rhs)) = value.cas_eq_parts() {
            if numeric_is_zero(lhs) {
                Assumption::Zero(rhs.clone())
            } else if numeric_is_zero(rhs) {
                Assumption::Zero(lhs.clone())
            } else {
                return Err(cas_err(
                    "CAS assumptions currently accept zero equations and nonzero predicates",
                )
                .got1(value));
            }
        } else {
            return Err(
                cas_err("CAS assumptions expect a condition or a list of conditions").got1(value),
            );
        };

        let (expr, expected) = match &fact {
            Assumption::Zero(expr) => (expr, Truth::Proven),
            Assumption::NonZero(expr) => (expr, Truth::Refuted),
        };
        let actual = self.prove_zero(expr);
        if actual != Truth::Unknown && actual != expected {
            return Err(cas_err(format!("contradictory CAS assumption for {expr}")));
        }
        self.facts.push(fact);
        Ok(())
    }

    pub(crate) fn prove_zero(&self, value: &Value) -> Truth {
        if is_known_numeric(value) {
            return if known_numeric_is_zero(value) {
                Truth::Proven
            } else {
                Truth::Refuted
            };
        }

        for fact in self.facts.iter().rev() {
            match fact {
                Assumption::Zero(expr) if expr == value => return Truth::Proven,
                Assumption::NonZero(expr) if expr == value => return Truth::Refuted,
                _ => {}
            }
        }
        Truth::Unknown
    }
}

fn known_numeric_is_zero(value: &Value) -> bool {
    match value {
        Value::Complex(value) => value.re == 0.0 && value.im == 0.0,
        _ => numeric_is_zero(value),
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
    fn contradictory_facts_are_rejected() {
        let a = Value::from_cas_var("a");
        let facts = Value::List(std::sync::Arc::new(vec![
            Value::from_cas_nonzero(a.clone()),
            Value::from_cas_eq(a, Value::Int(0)),
        ]));
        assert!(CasAssumptions::from_value(&facts).is_err());
    }
}
