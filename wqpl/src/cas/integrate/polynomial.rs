use crate::cas::{cas_div, cas_err, cas_pow, numeric_add};
use crate::value::cas::CasFunction;
use crate::value::{Value, WqResult};

pub(super) fn integrate_power_rule(base: &Value, exp: &Value, var: &str) -> WqResult<Value> {
    debug_assert!(base.cas_var_name() == Some(var));

    let (numer, denom) = exp.rational_parts().ok_or_else(|| {
        cas_err("symbolic integration currently requires exact rational exponents")
    })?;
    if numer == -denom {
        return Ok(Value::from_cas_function(
            CasFunction::Ln,
            vec![Value::from_cas_function(
                CasFunction::Abs,
                vec![Value::from_cas_var(var)],
            )],
        ));
    }
    let next = numeric_add(exp, &Value::Int(1))?;
    cas_div(cas_pow(Value::from_cas_var(var), next.clone())?, next)
}
