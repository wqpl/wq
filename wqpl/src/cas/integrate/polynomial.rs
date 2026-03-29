use crate::cas::{cas_div, cas_err, cas_pow, eval_numeric_binary};
use crate::value::{Value, WqResult};

pub(super) fn integrate_power_rule(base: &Value, exp: &Value, var: &str) -> WqResult<Value> {
    debug_assert!(base.cas_var_name() == Some(var));

    let (numer, denom) = exp.rational_parts().ok_or_else(|| {
        cas_err("symbolic integration currently requires exact rational exponents")
    })?;
    if numer == -denom {
        return Ok(Value::from_cas_call(
            "ln",
            vec![Value::from_cas_call("abs", vec![Value::from_cas_var(var)])],
        ));
    }
    let next = eval_numeric_binary("+", exp, &Value::Int(1))?;
    cas_div(cas_pow(Value::from_cas_var(var), next.clone())?, next)
}
