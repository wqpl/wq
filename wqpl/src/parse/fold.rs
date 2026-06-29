use std::sync::Arc;

use indexmap::IndexMap;

use crate::astnode::AstNode;
use crate::builtins::BuiltinFnArgs;
use crate::range::{make_range, make_range_from_next};
use crate::value::{Value, eval_binary, eval_unary};

pub(crate) fn fold(node: AstNode) -> AstNode {
    use AstNode::*;
    match node {
        Error(..)
        | Literal(..)
        | Variable(_, _)
        | OuterVariable(_, _)
        | Break(..)
        | Continue(..)
        | Ellipsis(..)
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
            span,
        } => fold_binary_chain(*left, operator, *right, span),
        ComparisonChain { first, rest, span } => ComparisonChain {
            first: Box::new(fold(*first)),
            rest: rest
                .into_iter()
                .map(|(op, node)| (op, fold(node)))
                .collect(),
            span,
        },
        Range {
            start,
            end,
            step,
            inclusive,
            span,
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

            if let (Some(start_val), Some(end_val)) = (start_val, end_val) {
                let folded = if let Some(step_val) = step_val {
                    make_range_from_next(start_val, step_val, end_val, inclusive)
                } else {
                    make_range(start_val, end_val, None, inclusive)
                };
                if let Ok(value) = folded {
                    return AstNode::Literal(value, span);
                }
            }

            AstNode::Range {
                start,
                end,
                step,
                inclusive,
                span,
            }
        }
        List(items, span) => {
            let items: Vec<AstNode> = items.into_iter().map(fold).collect();
            if items.iter().all(|n| matches!(n, Literal(..))) {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    if let Literal(v, _) = item {
                        values.push(v);
                    }
                }
                Literal(Value::from_items(values), span)
            } else {
                List(items, span)
            }
        }
        Cat(items, span) => {
            let items: Vec<AstNode> = items.into_iter().map(fold).collect();
            Cat(items, span)
        }
        Dict(pairs, span) => {
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
                Literal(Value::Dict(Arc::new(map)), span)
            } else {
                Dict(pairs, span)
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
            depth,
            span,
        } => Postfix {
            object: Box::new(fold(*object)),
            items: items.into_iter().map(fold).collect(),
            explicit_call,
            depth,
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
                    static PURE_BUILTINS: crate::builtins::Builtins =
                        crate::builtins::Builtins::with_preset(crate::builtins::BuiltinPreset::Pure);
                }
                let folded_val = PURE_BUILTINS.with(|builtins| {
                    if let Some(id) = builtins.get_id(name.as_str())
                        && let Some(func) = builtins.get_fn_by_id(id).copied()
                        && let Some(func) = func.as_plain()
                    {
                        return func(BuiltinFnArgs::from(literals)).ok();
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
            index: Box::new(fold_index_child(*index)),
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
            index: Box::new(fold_index_child(*index)),
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
        Function {
            params,
            ref_capture,
            body,
            span,
        } => Function {
            params,
            ref_capture,
            body: Box::new(fold(*body)),
            span,
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
        Pause { expr, span } => Pause {
            expr: expr.map(|expr| Box::new(fold(*expr))),
            span,
        },
        Return(expr, span) => Return(expr.map(|e| Box::new(fold(*e))), span),
        Try(expr, span) => Try(Box::new(fold(*expr)), span),
        Block(stmts, span) => Block(stmts.into_iter().map(fold).collect(), span),
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

fn fold_index_child(node: AstNode) -> AstNode {
    match node {
        AstNode::List(items, None) => AstNode::List(items.into_iter().map(fold).collect(), None),
        other => fold(other),
    }
}

fn fold_binary_chain(
    left: AstNode,
    operator: crate::astnode::BinaryOperator,
    right: AstNode,
    span: crate::astnode::AstSpan,
) -> AstNode {
    use AstNode::*;

    let mut chain = vec![(operator, right, span)];
    let mut current = left;
    let mut folded = loop {
        match current {
            BinaryOp {
                left,
                operator,
                right,
                span,
            } => {
                chain.push((operator, *right, span));
                current = *left;
            }
            other => break fold(other),
        }
    };

    for (operator, right, span) in chain.into_iter().rev() {
        let folded_right = fold(right);
        if let (Literal(lv, _), Literal(rv, _)) = (&folded, &folded_right)
            && let Ok(res) = eval_binary(&operator, lv, rv)
        {
            folded = Literal(res, span);
        } else {
            folded = BinaryOp {
                left: Box::new(folded),
                operator,
                right: Box::new(folded_right),
                span,
            };
        }
    }

    folded
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
            span: None,
        };
        let folded = fold(ast);
        assert_eq!(folded, AstNode::Literal(Value::Int(2), None));
    }

    #[test]
    fn folds_list_addition_to_int_list() {
        let l1 = AstNode::List(
            vec![
                AstNode::Literal(Value::Int(1), None),
                AstNode::Literal(Value::Int(2), None),
                AstNode::Literal(Value::Int(3), None),
                AstNode::Literal(Value::Int(4), None),
            ],
            None,
        );
        let l2 = AstNode::List(
            vec![
                AstNode::Literal(Value::Int(3), None),
                AstNode::Literal(Value::Int(4), None),
                AstNode::Literal(Value::Int(5), None),
                AstNode::Literal(Value::Int(6), None),
            ],
            None,
        );
        let ast = AstNode::BinaryOp {
            left: Box::new(l1),
            operator: BinaryOperator::Add,
            right: Box::new(l2),
            span: None,
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
            span: None,
        };
        let folded = fold(ast);
        assert_eq!(
            folded,
            AstNode::Literal(Value::IntList(Arc::new(vec![1, 2, 3, 4])), None)
        );
    }

    #[test]
    fn folds_range_literal_inclusive_with_next_point() {
        let ast = AstNode::Range {
            start: Box::new(AstNode::Literal(Value::Int(1), None)),
            end: Box::new(AstNode::Literal(Value::Int(5), None)),
            step: Some(Box::new(AstNode::Literal(Value::Int(3), None))),
            inclusive: true,
            span: None,
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
