use crate::ast::{BinaryOperator, UnaryOperator};
use crate::value::cell::ValueCell;
use crate::value::func::CallableExpr;
use crate::value::{Excerpt, Value, WqResult, eval_binary, eval_unary};
use crate::vm::inst::{Instruction, Operand};
use crate::wqerror::{WqError, WqErrorType};

#[derive(Clone)]
pub(crate) struct PureCallback {
    result: PureExpr,
}

#[derive(Clone)]
enum PureExpr {
    Arg(usize),
    Const(Value),
    CasCall {
        expr: Value,
        var: Value,
    },
    Unary {
        op: UnaryOperator,
        operand: Box<PureExpr>,
    },
    Index {
        target: Box<PureExpr>,
        args: Box<[PureExpr]>,
    },
    Binary {
        op: BinaryOperator,
        left: Box<PureExpr>,
        right: Box<PureExpr>,
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
        let mut stack = Vec::new();
        for inst in body {
            match inst {
                Instruction::LoadConst(v) => stack.push(PureExpr::Const((**v).clone())),
                Instruction::LoadLocal(slot) => {
                    stack.push(Self::local_expr(*slot, arity)?);
                }
                Instruction::LoadCapture(slot) => {
                    stack.push(Self::capture_expr(&captures, *slot)?);
                }
                Instruction::UnaryOp(data) => {
                    let operand = Self::operand_expr(&mut stack, &data.operand, arity, &captures)?;
                    stack.push(PureExpr::Unary {
                        op: data.op,
                        operand: Box::new(operand),
                    });
                }
                Instruction::BinaryOp(data) => {
                    let right = Self::operand_expr(&mut stack, &data.right, arity, &captures)?;
                    let left = Self::operand_expr(&mut stack, &data.left, arity, &captures)?;
                    stack.push(PureExpr::Binary {
                        op: data.op,
                        left: Box::new(left),
                        right: Box::new(right),
                    });
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
                    let target = Self::local_expr(*slot, arity)?;
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
                Instruction::PostfixLocal(slot, argc)
                | Instruction::TailPostfixLocal(slot, argc)
                    if *argc > 0 =>
                {
                    let args = Self::index_args(&mut stack, *argc)?;
                    let target = Self::local_expr(*slot, arity)?;
                    stack.push(Self::index_expr(target, args));
                }
                Instruction::PostfixCapture(slot, argc)
                | Instruction::TailPostfixCapture(slot, argc)
                    if *argc > 0 =>
                {
                    let args = Self::index_args(&mut stack, *argc)?;
                    let target = Self::capture_expr(&captures, *slot)?;
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

    fn local_expr(slot: u16, arity: usize) -> Option<PureExpr> {
        let slot = usize::from(slot);
        if slot < arity {
            Some(PureExpr::Arg(slot))
        } else {
            None
        }
    }

    fn capture_expr(captures: &[ValueCell], slot: u16) -> Option<PureExpr> {
        let cell = captures.get(usize::from(slot))?;
        Some(PureExpr::Const(
            cell.lock().expect("poisoned capture").clone(),
        ))
    }

    fn index_expr(target: PureExpr, args: Vec<PureExpr>) -> PureExpr {
        PureExpr::Index {
            target: Box::new(target),
            args: args.into_boxed_slice(),
        }
    }

    fn index_args(stack: &mut Vec<PureExpr>, argc: usize) -> Option<Vec<PureExpr>> {
        let base = stack.len().checked_sub(argc)?;
        Some(stack.drain(base..).collect())
    }

    fn operand_expr(
        stack: &mut Vec<PureExpr>,
        operand: &Operand,
        arity: usize,
        captures: &[ValueCell],
    ) -> Option<PureExpr> {
        match operand {
            Operand::Stack => stack.pop(),
            Operand::Const(v) => Some(PureExpr::Const((**v).clone())),
            Operand::Local(slot) => Self::local_expr(*slot, arity),
            Operand::Capture(slot) => Self::capture_expr(captures, *slot),
            Operand::Var(_) | Operand::Self_ => None,
        }
    }
}

impl PureExpr {
    fn from_cas_callable(value: &Value, arity: usize) -> Option<Self> {
        if arity != 1 || !value.is_cas_expr() {
            return None;
        }
        let var = crate::cas::infer_single_cas_var(value).ok()?;
        Some(Self::CasCall {
            expr: value.clone(),
            var: Value::from_cas_var(var),
        })
    }

    fn from_callable_expr(expr: &CallableExpr, arity: usize) -> Option<Self> {
        match expr {
            CallableExpr::Const(value) => Some(Self::Const(value.clone())),
            CallableExpr::Call(value) => {
                PureCallback::compile(value, arity).map(|callback| callback.result)
            }
            CallableExpr::Unary { op, operand } => Some(Self::Unary {
                op: *op,
                operand: Box::new(Self::from_callable_expr(operand, arity)?),
            }),
            CallableExpr::Binary { op, left, right } => Some(Self::Binary {
                op: *op,
                left: Box::new(Self::from_callable_expr(left, arity)?),
                right: Box::new(Self::from_callable_expr(right, arity)?),
            }),
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
        .attach_note(format!(
            "index: '{}' ({})",
            index.excerpt(),
            index.type_name()
        ))
        .attach_note(format!(
            "target: '{}' ({})",
            target.excerpt(),
            target.type_name()
        ))
}

#[cfg(test)]
mod tests {
    use crate::value::Value;
    use crate::value::cas::CasOp;
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
}
