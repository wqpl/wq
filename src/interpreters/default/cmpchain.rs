use crate::{
    astnode::BinaryOperator,
    value::{self, Value, WqResult},
};

#[inline]
pub fn eval_cmp_chain(ops: &[BinaryOperator], values: &[Value]) -> WqResult<Value> {
    debug_assert_eq!(ops.len() + 1, values.len());

    if ops.is_empty() || values.is_empty() {
        return Ok(Value::Bool(true));
    }

    let mut result = Value::Bool(true);
    let mut left = &values[0];

    for (idx, op) in ops.iter().enumerate() {
        let right = &values[idx + 1];
        let cmp = value::eval_binary(op, left.clone(), right.clone())?;
        result = result
            .and_bool(&cmp)
            .map_err(|e| e.into_wqerror().src("vm (cmp-chain)"))?;
        if matches!(result, Value::Bool(false)) {
            break;
        }
        left = right;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::astnode::BinaryOperator;

    #[test]
    fn chain_all_true_scalar() {
        let ops = [BinaryOperator::LessThan, BinaryOperator::LessThanOrEqual];
        let values = [Value::Int(1), Value::Int(2), Value::Int(2)];
        let result = eval_cmp_chain(&ops, &values).expect("cmp chain result");
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn chain_false_scalar() {
        let ops = [BinaryOperator::LessThan, BinaryOperator::LessThan];
        let values = [Value::Int(2), Value::Int(1), Value::Int(3)];
        let result = eval_cmp_chain(&ops, &values).expect("cmp chain result");
        assert_eq!(result, Value::Bool(false));
    }

    #[test]
    fn chain_list_broadcast() {
        let ops = [BinaryOperator::LessThan, BinaryOperator::LessThan];
        let values = [
            Value::from_items(vec![Value::Int(1), Value::Int(3)]),
            Value::from_items(vec![Value::Int(2), Value::Int(2)]),
            Value::from_items(vec![Value::Int(3), Value::Int(4)]),
        ];
        let result = eval_cmp_chain(&ops, &values).expect("cmp chain result");
        assert_eq!(
            result,
            Value::List(vec![Value::Bool(true), Value::Bool(false)])
        );
    }

    #[test]
    fn chain_scalar_with_list_broadcast() {
        let ops = [BinaryOperator::LessThan, BinaryOperator::LessThan];
        let values = [
            Value::Int(0),
            Value::from_items(vec![Value::Int(1), Value::Int(2)]),
            Value::Int(3),
        ];
        let result = eval_cmp_chain(&ops, &values).expect("cmp chain result");
        assert_eq!(
            result,
            Value::List(vec![Value::Bool(true), Value::Bool(true)])
        );
    }
}
