use std::sync::Arc;

use crate::ast::{BinaryOperator, UnaryOperator};
use crate::value::cell::ValueCell;
use crate::value::func::CallableExpr;
use crate::value::{Excerpt, Value, WqResult, eval_binary, eval_unary};
use crate::vm::inst::{Instruction, Operand};
use crate::wqerror::{WqError, WqErrorType};

#[derive(Clone)]
pub(crate) struct PureCallback {
    result: Arc<PureExpr>,
}

enum PureExpr {
    Arg(usize),
    Const(Value),
    Capture(ValueCell),
    CasCall {
        expr: Value,
        var: Value,
    },
    Unary {
        op: UnaryOperator,
        operand: Arc<PureExpr>,
    },
    Index {
        target: Arc<PureExpr>,
        args: Box<[Arc<PureExpr>]>,
    },
    Binary {
        op: BinaryOperator,
        left: Arc<PureExpr>,
        right: Arc<PureExpr>,
    },
}

impl PureCallback {
    pub(crate) fn compile(func: &Value, arity: usize) -> Option<Self> {
        if let Some(result) = PureExpr::from_cas_callable(func, arity) {
            return Some(Self { result });
        }

        if let Value::LiftedCallable(data) = func {
            return Some(Self {
                result: PureExpr::from_callable_expr(&data.expr, arity)?,
            });
        }

        let shape = func.as_user_function()?;
        if shape.named_params.is_some() {
            return None;
        }
        match shape.params_len() {
            Some(expected) if expected != arity => return None,
            None if arity > 3 => return None,
            _ => {}
        }
        if usize::from(shape.locals) < arity {
            return None;
        }

        let (last, body) = shape.instructions.split_last()?;
        if !matches!(last, Instruction::Return) {
            return None;
        }

        let captures = shape.captured();
        let mut locals = vec![None; usize::from(shape.locals)];
        for (slot, local) in locals.iter_mut().take(arity).enumerate() {
            *local = Some(Arc::new(PureExpr::Arg(slot)));
        }
        let mut stack = Vec::new();
        for inst in body {
            match inst {
                Instruction::LoadConst(v) => {
                    stack.push(Arc::new(PureExpr::Const((**v).clone())));
                }
                Instruction::LoadLocal(slot) => {
                    stack.push(Self::local_expr(&locals, *slot)?);
                }
                Instruction::LoadCapture(slot) => {
                    stack.push(Self::capture_expr(&captures, *slot)?);
                }
                Instruction::StoreLocal(slot) => {
                    let value = stack.pop()?;
                    *locals.get_mut(usize::from(*slot))? = Some(value);
                }
                Instruction::StoreLocalKeep(slot) => {
                    let value = Arc::clone(stack.last()?);
                    *locals.get_mut(usize::from(*slot))? = Some(value);
                }
                Instruction::UnaryOp(data) => {
                    let operand =
                        Self::operand_expr(&mut stack, &data.operand, &locals, &captures)?;
                    stack.push(Arc::new(PureExpr::Unary {
                        op: data.op,
                        operand,
                    }));
                }
                Instruction::BinaryOp(data) => {
                    let right = Self::operand_expr(&mut stack, &data.right, &locals, &captures)?;
                    let left = Self::operand_expr(&mut stack, &data.left, &locals, &captures)?;
                    stack.push(Arc::new(PureExpr::Binary {
                        op: data.op,
                        left,
                        right,
                    }));
                }
                Instruction::Index => {
                    let index = stack.pop()?;
                    let target = stack.pop()?;
                    stack.push(Self::index_expr(target, vec![index]));
                }
                Instruction::IndexMany(argc) if *argc > 0 => {
                    let args = Self::index_args(&mut stack, *argc)?;
                    let target = stack.pop()?;
                    stack.push(Self::index_expr(target, args));
                }
                Instruction::IndexManyLoadLocal(slot, argc) if *argc > 0 => {
                    let args = Self::index_args(&mut stack, *argc)?;
                    let target = Self::local_expr(&locals, *slot)?;
                    stack.push(Self::index_expr(target, args));
                }
                Instruction::IndexManyLoadCapture(slot, argc) if *argc > 0 => {
                    let args = Self::index_args(&mut stack, *argc)?;
                    let target = Self::capture_expr(&captures, *slot)?;
                    stack.push(Self::index_expr(target, args));
                }
                Instruction::Postfix(argc) | Instruction::TailPostfix(argc) if *argc > 0 => {
                    let args = Self::index_args(&mut stack, *argc)?;
                    let target = stack.pop()?;
                    stack.push(Self::index_expr(target, args));
                }
                _ => return None,
            }
        }

        if stack.len() == 1 {
            Some(Self {
                result: stack.pop()?,
            })
        } else {
            None
        }
    }

    pub(crate) fn eval(&self, args: &[&Value]) -> WqResult<Option<Value>> {
        self.result.eval(args)
    }

    fn local_expr(locals: &[Option<Arc<PureExpr>>], slot: u16) -> Option<Arc<PureExpr>> {
        locals.get(usize::from(slot))?.as_ref().map(Arc::clone)
    }

    fn capture_expr(captures: &[ValueCell], slot: u16) -> Option<Arc<PureExpr>> {
        let cell = captures.get(usize::from(slot))?;
        Some(Arc::new(PureExpr::Capture(Arc::clone(cell))))
    }

    fn index_expr(target: Arc<PureExpr>, args: Vec<Arc<PureExpr>>) -> Arc<PureExpr> {
        Arc::new(PureExpr::Index {
            target,
            args: args.into_boxed_slice(),
        })
    }

    fn index_args(stack: &mut Vec<Arc<PureExpr>>, argc: usize) -> Option<Vec<Arc<PureExpr>>> {
        let base = stack.len().checked_sub(argc)?;
        Some(stack.drain(base..).collect())
    }

    fn operand_expr(
        stack: &mut Vec<Arc<PureExpr>>,
        operand: &Operand,
        locals: &[Option<Arc<PureExpr>>],
        captures: &[ValueCell],
    ) -> Option<Arc<PureExpr>> {
        match operand {
            Operand::Stack => stack.pop(),
            Operand::Const(v) => Some(Arc::new(PureExpr::Const((**v).clone()))),
            Operand::Local(slot) => Self::local_expr(locals, *slot),
            Operand::Capture(slot) => Self::capture_expr(captures, *slot),
            Operand::Var(_) | Operand::Self_ => None,
        }
    }
}

impl PureExpr {
    fn from_cas_callable(value: &Value, arity: usize) -> Option<Arc<Self>> {
        if arity != 1 || !value.is_cas_expr() {
            return None;
        }
        let var = crate::cas::infer_single_cas_var(value).ok()?;
        Some(Arc::new(Self::CasCall {
            expr: value.clone(),
            var: Value::from_cas_var(var),
        }))
    }

    fn from_callable_expr(expr: &CallableExpr, arity: usize) -> Option<Arc<Self>> {
        match expr {
            CallableExpr::Const(value) => Some(Arc::new(Self::Const(value.clone()))),
            CallableExpr::Call(value) => {
                PureCallback::compile(value, arity).map(|callback| callback.result)
            }
            CallableExpr::Unary { op, operand } => Some(Arc::new(Self::Unary {
                op: *op,
                operand: Self::from_callable_expr(operand, arity)?,
            })),
            CallableExpr::Binary { op, left, right } => Some(Arc::new(Self::Binary {
                op: *op,
                left: Self::from_callable_expr(left, arity)?,
                right: Self::from_callable_expr(right, arity)?,
            })),
        }
    }

    fn eval(&self, args: &[&Value]) -> WqResult<Option<Value>> {
        match self {
            Self::Arg(slot) => {
                let arg = args.get(*slot).ok_or_else(|| {
                    WqError::new(WqErrorType::Vm).msg("pure callback argument missing")
                })?;
                Ok(Some((**arg).clone()))
            }
            Self::Const(v) => Ok(Some(v.clone())),
            Self::Capture(cell) => Ok(Some(cell.lock().expect("poisoned capture").clone())),
            Self::CasCall { expr, var } => {
                let arg = args.first().ok_or_else(|| {
                    WqError::new(WqErrorType::Vm).msg("pure callback argument missing")
                })?;
                crate::cas::substitute_cas(expr, var, arg).map(Some)
            }
            Self::Unary { op, operand } => {
                let Some(value) = operand.eval(args)? else {
                    return Ok(None);
                };
                eval_unary(op, &value).map(Some)
            }
            Self::Index {
                target,
                args: index_args,
            } => {
                let Some(target) = target.eval(args)? else {
                    return Ok(None);
                };
                if target.is_callable() {
                    return Ok(None);
                }
                let mut resolved_args = Vec::with_capacity(index_args.len());
                for index_arg in index_args {
                    let Some(value) = index_arg.eval(args)? else {
                        return Ok(None);
                    };
                    resolved_args.push(value);
                }
                let result = if resolved_args.len() == 1 {
                    target.index(
                        resolved_args
                            .first()
                            .expect("single index argument should exist"),
                    )
                } else {
                    target.index_many(&resolved_args)
                };
                match result {
                    Some(value) => Ok(Some(value)),
                    None => Err(pure_index_err(&target, &resolved_args)),
                }
            }
            Self::Binary { op, left, right } => {
                let Some(left) = left.eval(args)? else {
                    return Ok(None);
                };
                let Some(right) = right.eval(args)? else {
                    return Ok(None);
                };
                eval_binary(op, &left, &right).map(Some)
            }
        }
    }
}

fn pure_index_err(target: &Value, args: &[Value]) -> WqError {
    let index = if args.len() == 1 {
        args.first()
            .expect("single index argument should exist")
            .clone()
    } else {
        Value::from_items(args.to_vec())
    };
    WqError::new(WqErrorType::Index)
        .src("pure callback")
        .msg("invalid index")
        .attach_note(format!("index: {} ({})", index.excerpt(), index.category()))
        .attach_note(format!(
            "target: {} ({})",
            target.excerpt(),
            target.category()
        ))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::value::Value;
    use crate::value::cas::CasOp;
    use crate::value::func::ClosureData;
    use crate::vm::inst::{Instruction, Operand};
    use crate::vm::pure::PureCallback;

    #[test]
    fn pure_callback_compiles_single_var_cas_callable() {
        let expr = Value::from_cas_op(
            CasOp::Add,
            vec![
                Value::from_cas_op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                Value::Int(1),
            ],
        );
        let callback = PureCallback::compile(&expr, 1).expect("single-var CAS should plan");
        let arg = Value::Int(3);

        assert_eq!(
            callback
                .eval(&[&arg])
                .expect("pure CAS callback should evaluate"),
            Some(Value::Int(10))
        );
    }

    #[test]
    fn pure_callback_rejects_non_unary_cas_callable() {
        let expr = Value::from_cas_op(
            CasOp::Multiply,
            vec![Value::from_cas_var("x"), Value::from_cas_var("y")],
        );

        assert!(PureCallback::compile(&expr, 1).is_none());
        assert!(PureCallback::compile(&Value::from_cas_var("x"), 2).is_none());
    }

    #[test]
    fn pure_callback_reads_current_capture_values() {
        let capture = Arc::new(Mutex::new(Value::Int(10)));
        let closure = Value::Closure(Arc::new(ClosureData {
            params: Some(Arc::from([String::from("x")])),
            named_params: None,
            locals: 1,
            captured: Arc::from([Arc::clone(&capture)]),
            instructions: Arc::from([
                Instruction::binary_op(
                    crate::ast::BinaryOperator::Add,
                    Operand::Local(0),
                    Operand::Capture(0),
                ),
                Instruction::Return,
            ]),
            dbg_chunk: None,
            dbg_stmt_spans: None,
            dbg_source_base_offset: 0,
            dbg_pc_spans: None,
            dbg_stmt_marks: None,
            dbg_local_names: None,
            dbg_provenance: None,
        }));
        let callback = PureCallback::compile(&closure, 1).expect("closure should plan");
        let arg = Value::Int(1);

        assert_eq!(
            callback.eval(&[&arg]).expect("first evaluation"),
            Some(Value::Int(11))
        );
        *capture.lock().expect("capture lock") = Value::Int(20);
        assert_eq!(
            callback.eval(&[&arg]).expect("second evaluation"),
            Some(Value::Int(21))
        );
    }
}
