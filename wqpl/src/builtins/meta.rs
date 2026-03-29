use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity};
use crate::value::{IntoWqValue as _, Value, WqResult};
use crate::vm::Vm;

pub(super) fn scount(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Scount, [1], &args)?;
    Ok(args[0].strong_count().into_wq_value())
}

pub(super) fn wcount(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Wcount, [1], &args)?;
    Ok(args[0].weak_count().into_wq_value())
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

#[cfg(test)]
mod arc_count_tests {

    use std::sync::Arc;

    use super::*;
    #[test]
    fn scount_inline_types_return_1() {
        let mut vm = Vm::new(vec![]);
        assert_eq!(
            scount(&mut vm, BuiltinFnArgs::from(Value::Int(42))).unwrap(),
            Value::Int(1)
        );
        assert_eq!(
            scount(&mut vm, BuiltinFnArgs::from(Value::float(4.14))).unwrap(),
            Value::Int(1)
        );
        assert_eq!(
            scount(&mut vm, BuiltinFnArgs::from(Value::Char('a'))).unwrap(),
            Value::Int(1)
        );
        assert_eq!(
            scount(&mut vm, BuiltinFnArgs::from(Value::Bool(true))).unwrap(),
            Value::Int(1)
        );
    }

    #[test]
    fn scount_arc_backed_returns_positive() {
        let mut vm = Vm::new(vec![]);
        let list = Value::List(Arc::new(vec![Value::Int(1)]));
        let result = scount(&mut vm, BuiltinFnArgs::from(list)).unwrap();
        assert!(matches!(result, Value::Int(n) if n >= 1));
    }

    #[test]
    fn wcount_inline_types_return_1() {
        let mut vm = Vm::new(vec![]);
        assert_eq!(
            wcount(&mut vm, BuiltinFnArgs::from(Value::Int(42))).unwrap(),
            Value::Int(1)
        );
        assert_eq!(
            wcount(&mut vm, BuiltinFnArgs::from(Value::float(4.14))).unwrap(),
            Value::Int(1)
        );
        assert_eq!(
            wcount(&mut vm, BuiltinFnArgs::from(Value::Char('a'))).unwrap(),
            Value::Int(1)
        );
        assert_eq!(
            wcount(&mut vm, BuiltinFnArgs::from(Value::Bool(true))).unwrap(),
            Value::Int(1)
        );
    }

    #[test]
    fn wcount_arc_backed_returns_weak_count() {
        let mut vm = Vm::new(vec![]);
        let list = Value::List(Arc::new(vec![Value::Int(1)]));
        let result = wcount(&mut vm, BuiltinFnArgs::from(list)).unwrap();
        assert!(matches!(result, Value::Int(n) if n >= 0));
    }
}
