use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity};
use crate::value::{IntoWqValue as _, Value, WqResult};
use crate::vm::Vm;

pub(super) fn strong_count(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::StrongCount, [1], &args)?;
    Ok(args[0]
        .strong_count()
        .map_or_else(|| Value::Int(1), |v| v.into_wq_value()))
}

pub(super) fn len(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Len, [1], &args)?;
    Ok(args[0].len().into_wq_value())
}

pub(super) fn shape(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Shape, [1], &args)?;
    Ok(args[0].shape())
}

pub(super) fn depth(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Depth, [1], &args)?;
    Ok(Value::Int(args[0].depth()))
}

pub(super) fn is_uniform(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::UniformQ, [1], &args)?;
    Ok(Value::Bool(args[0].is_uniform()))
}

#[cfg(test)]
mod tests {

    use std::sync::Arc;

    use super::*;
    use crate::builtins::listgen::alloc;

    #[test]
    fn shape_and_alloc() {
        let mut vm = Vm::new(vec![]);
        // simple vector
        let vec = alloc(&mut vm, BuiltinFnArgs::from(Value::Int(3))).unwrap();
        assert_eq!(vec, Value::IntList(Arc::new(vec![0, 0, 0])));
        assert_eq!(
            shape(&mut vm, BuiltinFnArgs::from(vec)).unwrap(),
            Value::IntList(Arc::new(vec![3]))
        );

        // matrix
        let mat_shape = Value::List(Arc::new(vec![Value::Int(2), Value::Int(3)]));
        let mat = alloc(&mut vm, BuiltinFnArgs::from(mat_shape)).unwrap();
        assert_eq!(
            shape(&mut vm, BuiltinFnArgs::from(mat)).unwrap(),
            Value::IntList(Arc::new(vec![2, 3]))
        );

        // invalid shape
        let invalid_shape = Value::List(Arc::new(vec![Value::List(Arc::new(vec![
            Value::Int(2),
            Value::Int(2),
        ]))]));
        let invalid = alloc(&mut vm, BuiltinFnArgs::from(invalid_shape));
        assert!(invalid.is_err());
    }

    #[test]
    fn shape_atoms_and_empty() {
        let mut vm = Vm::new(vec![]);
        assert_eq!(
            shape(&mut vm, BuiltinFnArgs::from(Value::Int(5))).unwrap(),
            Value::IntList(Arc::new(vec![]))
        );
        assert_eq!(
            shape(&mut vm, BuiltinFnArgs::from(Value::Char('a'))).unwrap(),
            Value::IntList(Arc::new(vec![]))
        );
        assert_eq!(
            shape(&mut vm, BuiltinFnArgs::from(Value::List(Arc::new(vec![])))).unwrap(),
            Value::IntList(Arc::new(vec![0]))
        );
    }

    #[test]
    fn shape_string_and_mixed_list() {
        let mut vm = Vm::new(vec![]);
        let s = Value::List(Arc::new(vec![Value::Char('h'), Value::Char('i')]));
        assert_eq!(
            shape(&mut vm, BuiltinFnArgs::from(s)).unwrap(),
            Value::IntList(Arc::new(vec![2]))
        );
        let mixed = Value::List(Arc::new(vec![Value::Char('h'), Value::Int(2)]));
        assert_eq!(
            shape(&mut vm, BuiltinFnArgs::from(mixed)).unwrap(),
            Value::IntList(Arc::new(vec![2]))
        );
    }
}
