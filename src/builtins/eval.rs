use super::arity_error;
use crate::{
    builtins::values_to_strings,
    evaluator::Evaluator,
    value::valuei::{Value, WqResult},
};

pub fn eval(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("eval", "1 argument", args.len()));
    }
    let code = values_to_strings(&[args[0].clone()])?
        .into_iter()
        .next()
        .unwrap();
    let mut evaluator = Evaluator::new();
    evaluator.eval_string(&code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_simple_expression() {
        let code = Value::List("1+1".chars().map(Value::Char).collect());
        let result = eval(&[code]).unwrap();
        assert_eq!(result, Value::Int(2));
    }
}
