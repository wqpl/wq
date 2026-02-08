use crate::{
    builtins::{BuiltinEnum as BE, wqerror_helper::check_arity},
    value::{Value, WqResult, into_wq_str},
    vm::Vm,
    wqerror::{WqError, WqErrorType},
};

pub fn type_of(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Type, [1], args)?;
    Ok(into_wq_str(args[0].type_name()))
}

pub fn to_symbol(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Symbol, [1], args)?;
    let input = &args[0];
    if input.is_empty() {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BE::Symbol)
            .msg("symbol cannot be empty"));
    }
    if matches!(input, Value::Symbol(_)) {
        return Ok(input.clone());
    }
    let name = input
        .try_to_string()
        .map_err(|e| e.src(BE::Symbol).at_arg(0))?;
    Ok(Value::Symbol(name))
}

pub fn is_atom(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::AtomQ, [1], args)?;
    Ok(Value::Bool(args[0].is_atom()))
}

pub fn is_unit(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::UnitQ, [1], args)?;
    Ok(Value::Bool(args[0].is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{value::IntoWqValue, vm::Vm};

    #[test]
    fn symbol_accepts_question_mark() {
        let mut vm = Vm::new(vec![]);
        let val = "a?".into_wq_value();
        let result = to_symbol(&mut vm, &[val]).unwrap();
        assert_eq!(result, Value::Symbol("a?".to_string()));
    }

    #[test]
    fn counts_as_atom() {
        let mut vm = Vm::new(vec![]);
        let val = Value::List(vec![Value::Int(1)]);
        let result = is_atom(&mut vm, &[val]).unwrap();
        assert_eq!(result, Value::Bool(false));
    }
}
