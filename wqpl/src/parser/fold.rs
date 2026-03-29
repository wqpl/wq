use std::sync::Arc;

use indexmap::IndexMap;

use crate::astnode::AstNode;
use crate::builtins::BuiltinFnArgs;
use crate::value::{Value, eval_binary, eval_unary};

pub(crate) fn fold(node: AstNode) -> AstNode {
    use AstNode::*;
    match node {
        Error(..)
        | Literal(..)
        | Variable(_, _)
        | OuterVariable(_, _)
        | Break
        | Continue
        | Ellipsis
        | PipeInput => node,
        UnaryOp {
            operator,
            operand,
            span,
        } => {
            let operand = Box::new(fold(*operand));
            if let Literal(v, _) = operand.as_ref()
                && let Ok(res) = eval_unary(&operator, v)
            {
                return Literal(res, span);
            }
            UnaryOp {
                operator,
                operand,
                span,
            }
        }
        Group { expr, span } => {
            let expr = Box::new(fold(*expr));
            Group { expr, span }
        }
        BinaryOp {
            left,
            operator,
            right,
        } => {
            let left = Box::new(fold(*left));
            let right = Box::new(fold(*right));
            if let (Literal(lv, _), Literal(rv, _)) = (&*left, &*right)
                && let Ok(res) = eval_binary(&operator, lv, rv)
            {
                return Literal(res, None);
            }
            BinaryOp {
                left,
                operator,
                right,
            }
        }
        ComparisonChain { first, rest } => ComparisonChain {
            first: Box::new(fold(*first)),
            rest: rest
                .into_iter()
                .map(|(op, node)| (op, fold(node)))
                .collect(),
        },
        Range {
            start,
            end,
            step,
            inclusive,
        } => {
            let start = Box::new(fold(*start));
            let end = Box::new(fold(*end));
            let step = step.map(|s| Box::new(fold(*s)));

            let start_val = match &*start {
                AstNode::Literal(v, _) => Some(v),
                _ => None,
            };
            let end_val = match &*end {
                AstNode::Literal(v, _) => Some(v),
                _ => None,
            };
            let step_val = match step.as_deref() {
                Some(AstNode::Literal(v, _)) => Some(v),
                _ => None,
            };

            // Try integer constant folding.
            if let (Some(Value::Int(start_int)), Some(Value::Int(end_int))) = (start_val, end_val) {
                let step_int = match step_val {
                    Some(Value::Int(s)) => Some(*s),
                    Some(Value::Float(_)) => None, // fall through to float path
                    Some(_) => None,
                    None => Some(1),
                };
                if let Some(step_val) = step_int
                    && step_val != 0
                {
                    let mut cur = *start_int;
                    let mut items: Vec<i64> = Vec::new();
                    let advance = |c: i64, step: i64| c.checked_add(step);
                    if step_val > 0 {
                        while if inclusive {
                            cur <= *end_int
                        } else {
                            cur < *end_int
                        } {
                            items.push(cur);
                            cur = match advance(cur, step_val) {
                                Some(next) => next,
                                None => {
                                    return AstNode::Range {
                                        start,
                                        end,
                                        step,
                                        inclusive,
                                    };
                                }
                            };
                        }
                    } else {
                        while if inclusive {
                            cur >= *end_int
                        } else {
                            cur > *end_int
                        } {
                            items.push(cur);
                            cur = match advance(cur, step_val) {
                                Some(next) => next,
                                None => {
                                    return AstNode::Range {
                                        start,
                                        end,
                                        step,
                                        inclusive,
                                    };
                                }
                            };
                        }
                    }
                    return AstNode::Literal(Value::IntList(Arc::new(items)), None);
                }
            }

            // Try float constant folding.
            let start_f = start_val.and_then(|v| match v {
                Value::Int(n) => Some(*n as f64),
                Value::Float(f) => Some(**f),
                _ => None,
            });
            let end_f = end_val.and_then(|v| match v {
                Value::Int(n) => Some(*n as f64),
                Value::Float(f) => Some(**f),
                _ => None,
            });
            let step_f = match step_val {
                Some(Value::Int(n)) => Some(*n as f64),
                Some(Value::Float(f)) => Some(**f),
                Some(_) => None,
                None => Some(1.0),
            };
            if let (Some(start_f), Some(end_f), Some(step_f)) = (start_f, end_f, step_f)
                && step_f != 0.0
            {
                let mut items = Vec::new();
                const MAX_ITER: usize = 10_000_000;
                if step_f > 0.0 {
                    for i in 0..MAX_ITER {
                        let cur = start_f + i as f64 * step_f;
                        if if inclusive { cur > end_f } else { cur >= end_f } {
                            break;
                        }
                        items.push(Value::float(cur));
                    }
                } else {
                    for i in 0..MAX_ITER {
                        let cur = start_f + i as f64 * step_f;
                        if if inclusive { cur < end_f } else { cur <= end_f } {
                            break;
                        }
                        items.push(Value::float(cur));
                    }
                }
                return AstNode::Literal(Value::List(Arc::new(items)), None);
            }

            AstNode::Range {
                start,
                end,
                step,
                inclusive,
            }
        }
        List(items) => {
            let items: Vec<AstNode> = items.into_iter().map(fold).collect();
            if items.iter().all(|n| matches!(n, Literal(..))) {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    if let Literal(v, _) = item {
                        values.push(v);
                    }
                }
                Literal(Value::from_items(values), None)
            } else {
                List(items)
            }
        }
        Cat(items) => {
            let items: Vec<AstNode> = items.into_iter().map(fold).collect();
            Cat(items)
        }
        Dict(pairs) => {
            let pairs: Vec<(String, AstNode)> =
                pairs.into_iter().map(|(k, v)| (k, fold(v))).collect();
            // If all values are literals, fold into a literal dict value
            if pairs.iter().all(|(_, v)| matches!(v, Literal(..))) {
                let mut map: IndexMap<Arc<str>, Value> = IndexMap::with_capacity(pairs.len());
                for (k, v) in pairs {
                    if let Literal(val, _) = v {
                        map.insert(k.into(), val);
                    }
                }
                Literal(Value::Dict(Arc::new(map)), None)
            } else {
                Dict(pairs)
            }
        }
        Set(items, span) => {
            let items: Vec<AstNode> = items.into_iter().map(fold).collect();
            // If all items are literals, fold into a literal set value
            if items.iter().all(|n| matches!(n, Literal(..))) {
                let mut set = indexmap::IndexSet::with_capacity(items.len());
                for item in items {
                    if let Literal(v, _) = item {
                        set.insert(v);
                    }
                }
                Literal(Value::Set(Arc::new(set)), span)
            } else {
                Set(items, span)
            }
        }
        Assignment {
            name,
            op,
            value,
            span,
            name_span,
        } => Assignment {
            name,
            op,
            value: Box::new(fold(*value)),
            span,
            name_span,
        },
        OuterAssignment {
            name,
            op,
            value,
            span,
            name_span,
        } => OuterAssignment {
            name,
            op,
            value: Box::new(fold(*value)),
            span,
            name_span,
        },
        Postfix {
            object,
            items,
            explicit_call,
            span,
        } => Postfix {
            object: Box::new(fold(*object)),
            items: items.into_iter().map(fold).collect(),
            explicit_call,
            span,
        },
        Pipe {
            input,
            effect,
            kind,
            span,
        } => Pipe {
            input: Box::new(fold(*input)),
            effect: Box::new(fold(*effect)),
            kind,
            span,
        },
        PipeTap {
            input,
            effect,
            span,
        } => PipeTap {
            input: Box::new(fold(*input)),
            effect: Box::new(fold(*effect)),
            span,
        },
        CallName {
            name,
            args,
            span,
            name_span,
        } => {
            let args: Vec<AstNode> = args.into_iter().map(fold).collect();
            if args.iter().all(|a| matches!(a, Literal(..))) {
                let literals: Vec<Value> = args
                    .iter()
                    .map(|a| {
                        if let Literal(v, _) = a {
                            v.clone()
                        } else {
                            unreachable!()
                        }
                    })
                    .collect();

                thread_local! {
                    static PURE_VM: std::cell::RefCell<(crate::builtins::Builtins, crate::vm::Vm)> = std::cell::RefCell::new((
                        crate::builtins::Builtins::with_preset(crate::builtins::BuiltinPreset::Pure),
                        crate::vm::Vm::new(vec![])
                    ));
                }
                let folded_val = PURE_VM.with(|b| {
                    let mut b = b.borrow_mut();
                    let (builtins, vm) = &mut *b;
                    if let Some(id) = builtins.get_id(name.as_str())
                        && let Some(func) = builtins.get_fn_by_id(id)
                    {
                        return func(vm, BuiltinFnArgs::from(literals)).ok();
                    }
                    None
                });

                if let Some(v) = folded_val {
                    return Literal(v, None);
                }
            }
            CallName {
                name,
                args,
                span,
                name_span,
            }
        }
        CallAnonymous { object, args, span } => CallAnonymous {
            object: Box::new(fold(*object)),
            args: args.into_iter().map(fold).collect(),
            span,
        },
        Index {
            object,
            index,
            span,
        } => Index {
            object: Box::new(fold(*object)),
            index: Box::new(fold(*index)),
            span,
        },
        MutatingIndex {
            object,
            index,
            span,
        } => MutatingIndex {
            object: Box::new(fold(*object)),
            index: Box::new(fold(*index)),
            span,
        },
        IndexAssign {
            object,
            index,
            op,
            value,
            span,
        } => IndexAssign {
            object: Box::new(fold(*object)),
            index: Box::new(fold(*index)),
            op,
            value: Box::new(fold(*value)),
            span,
        },
        MutatingIndexAssign {
            object,
            index,
            value,
            span,
        } => MutatingIndexAssign {
            object: Box::new(fold(*object)),
            index: Box::new(fold(*index)),
            value: Box::new(fold(*value)),
            span,
        },
        Function { params, body } => Function {
            params,
            body: Box::new(fold(*body)),
        },
        Conditional {
            condition,
            true_branch,
            false_branch,
            span,
        } => {
            let condition = Box::new(fold(*condition));
            let true_branch = Box::new(fold(*true_branch));
            let false_branch = false_branch.map(|b| Box::new(fold(*b)));
            Conditional {
                condition,
                true_branch,
                false_branch,
                span,
            }
        }
        ConditionalDot {
            condition,
            true_branch,
            span,
        } => {
            let condition = Box::new(fold(*condition));
            let true_branch = Box::new(fold(*true_branch));
            ConditionalDot {
                condition,
                true_branch,
                span,
            }
        }
        ConditionalChain {
            pairs,
            default_branch,
            span,
        } => {
            let pairs: Vec<(AstNode, AstNode)> = pairs
                .into_iter()
                .map(|(cond, branch)| (fold(cond), fold(branch)))
                .collect();
            let default_branch = Box::new(fold(*default_branch));
            ConditionalChain {
                pairs,
                default_branch,
                span,
            }
        }
        WLoop {
            condition,
            body,
            span,
        } => {
            let condition = Box::new(fold(*condition));
            let body = Box::new(fold(*body));
            if let Literal(Value::Bool(false), _) = condition.as_ref() {
                return Literal(Value::unit(), span);
            }
            WLoop {
                condition,
                body,
                span,
            }
        }
        NLoop { count, body, span } => {
            if let AstNode::Literal(Value::Int(n), _) = count.as_ref()
                && *n <= 0
            {
                return Literal(Value::unit(), span);
            }
            NLoop {
                count: Box::new(fold(*count)),
                body: Box::new(fold(*body)),
                span,
            }
        }
        Assert { expr, span } => Assert {
            expr: Box::new(fold(*expr)),
            span,
        },
        Debug { expr, span } => Debug {
            expr: Box::new(fold(*expr)),
            span,
        },
        Pause { span } => Pause { span },
        Return(expr) => Return(expr.map(|e| Box::new(fold(*e)))),
        Try(expr) => Try(Box::new(fold(*expr))),
        Block(stmts) => Block(stmts.into_iter().map(fold).collect()),
        BlockExpr(stmts, span) => BlockExpr(stmts.into_iter().map(fold).collect(), span),
        NamedArg { name, value, span } => NamedArg {
            name,
            value: Box::new(fold(*value)),
            span,
        },
        UnpackAssignment { .. } => {
            unreachable!("UnpackAssignment should have been resolved before fold")
        }
        FString { .. } => {
            unreachable!("FString should have been resolved before fold")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::astnode::BinaryOperator;

    #[test]
    fn folds_simple_addition() {
        let ast = AstNode::BinaryOp {
            left: Box::new(AstNode::Literal(Value::Int(1), None)),
            operator: BinaryOperator::Add,
            right: Box::new(AstNode::Literal(Value::Int(1), None)),
        };
        let folded = fold(ast);
        assert_eq!(folded, AstNode::Literal(Value::Int(2), None));
    }

    #[test]
    fn folds_list_addition_to_int_list() {
        let l1 = AstNode::List(vec![
            AstNode::Literal(Value::Int(1), None),
            AstNode::Literal(Value::Int(2), None),
            AstNode::Literal(Value::Int(3), None),
            AstNode::Literal(Value::Int(4), None),
        ]);
        let l2 = AstNode::List(vec![
            AstNode::Literal(Value::Int(3), None),
            AstNode::Literal(Value::Int(4), None),
            AstNode::Literal(Value::Int(5), None),
            AstNode::Literal(Value::Int(6), None),
        ]);
        let ast = AstNode::BinaryOp {
            left: Box::new(l1),
            operator: BinaryOperator::Add,
            right: Box::new(l2),
        };
        let folded = fold(ast);
        assert_eq!(
            folded,
            AstNode::Literal(Value::IntList(Arc::new(vec![4, 6, 8, 10])), None)
        );
    }

    #[test]
    fn folds_range_literal_half_open() {
        let ast = AstNode::Range {
            start: Box::new(AstNode::Literal(Value::Int(1), None)),
            end: Box::new(AstNode::Literal(Value::Int(5), None)),
            step: None,
            inclusive: false,
        };
        let folded = fold(ast);
        assert_eq!(
            folded,
            AstNode::Literal(Value::IntList(Arc::new(vec![1, 2, 3, 4])), None)
        );
    }

    #[test]
    fn folds_range_literal_inclusive_with_step() {
        let ast = AstNode::Range {
            start: Box::new(AstNode::Literal(Value::Int(1), None)),
            end: Box::new(AstNode::Literal(Value::Int(5), None)),
            step: Some(Box::new(AstNode::Literal(Value::Int(2), None))),
            inclusive: true,
        };
        let folded = fold(ast);
        assert_eq!(
            folded,
            AstNode::Literal(Value::IntList(Arc::new(vec![1, 3, 5])), None)
        );
    }

    #[test]
    fn folds_pure_builtin() {
        let ast = AstNode::CallName {
            name: "len".to_string(),
            args: vec![AstNode::Literal(
                Value::from_items(vec![Value::Int(10), Value::Int(20)]),
                None,
            )],
            span: None,
            name_span: None,
        };
        let folded = fold(ast);
        assert_eq!(folded, AstNode::Literal(Value::Int(2), None));
    }
}
