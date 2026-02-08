use crate::{
    astnode::AstNode,
    value::{Value, eval_binary, eval_unary},
};

use indexmap::IndexMap;

pub fn fold(node: AstNode) -> AstNode {
    use AstNode::*;
    match node {
        Literal(_) | Variable(_) | Break | Continue | Ellipsis => node,
        UnaryOp { operator, operand } => {
            let operand = Box::new(fold(*operand));
            if let Literal(v) = operand.as_ref()
                && let Ok(res) = eval_unary(&operator, v.clone())
            {
                return Literal(res);
            }
            UnaryOp { operator, operand }
        }
        BinaryOp {
            left,
            operator,
            right,
        } => {
            let left = Box::new(fold(*left));
            let right = Box::new(fold(*right));
            if let (Literal(lv), Literal(rv)) = (&*left, &*right)
                && let Ok(res) = eval_binary(&operator, lv.clone(), rv.clone())
            {
                return Literal(res);
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
            if let (
                AstNode::Literal(Value::Int(start_int)),
                AstNode::Literal(Value::Int(end_int)),
            ) = (&*start, &*end)
            {
                let step_val_opt = match step.as_deref() {
                    Some(AstNode::Literal(Value::Int(s))) => Some(*s),
                    Some(_) => None,
                    None => Some(1),
                };
                if let Some(step_val) = step_val_opt {
                    if step_val == 0 {
                        return AstNode::Range {
                            start,
                            end,
                            step,
                            inclusive,
                        };
                    }
                    // if step_val > 0 && start_int > end_int {
                    //     step_val = -step_val;
                    // }
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
                    return AstNode::Literal(Value::IntList(items));
                }
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
            if items.iter().all(|n| matches!(n, Literal(_))) {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    if let Literal(v) = item {
                        values.push(v);
                    }
                }
                Literal(Value::from_items(values))
            } else {
                List(items)
            }
        }
        Dict(pairs) => {
            let pairs: Vec<(String, AstNode)> =
                pairs.into_iter().map(|(k, v)| (k, fold(v))).collect();
            // If all values are literals, fold into a literal dict value
            if pairs.iter().all(|(_, v)| matches!(v, Literal(_))) {
                let mut map: IndexMap<String, Value> = IndexMap::with_capacity(pairs.len());
                for (k, v) in pairs {
                    if let Literal(val) = v {
                        map.insert(k, val);
                    }
                }
                Literal(Value::Dict(map))
            } else {
                Dict(pairs)
            }
        }
        Assignment { name, value } => Assignment {
            name,
            value: Box::new(fold(*value)),
        },
        UnpackAssign { pattern, value } => UnpackAssign {
            pattern,
            value: Box::new(fold(*value)),
        },
        Postfix {
            object,
            items,
            explicit_call,
        } => Postfix {
            object: Box::new(fold(*object)),
            items: items.into_iter().map(fold).collect(),
            explicit_call,
        },
        Call { name, args } => Call {
            name,
            args: args.into_iter().map(fold).collect(),
        },
        CallAnonymous { object, args } => CallAnonymous {
            object: Box::new(fold(*object)),
            args: args.into_iter().map(fold).collect(),
        },
        Index { object, index } => Index {
            object: Box::new(fold(*object)),
            index: Box::new(fold(*index)),
        },
        IndexAssign {
            object,
            index,
            value,
        } => IndexAssign {
            object: Box::new(fold(*object)),
            index: Box::new(fold(*index)),
            value: Box::new(fold(*value)),
        },
        Function { params, body } => Function {
            params,
            body: Box::new(fold(*body)),
        },
        Conditional {
            condition,
            true_branch,
            false_branch,
        } => {
            let condition = Box::new(fold(*condition));
            let true_branch = Box::new(fold(*true_branch));
            let false_branch = false_branch.map(|b| Box::new(fold(*b)));
            if let Literal(Value::Bool(b)) = condition.as_ref() {
                return if *b {
                    *true_branch
                } else if let Some(fb) = false_branch {
                    *fb
                } else {
                    Literal(Value::unit())
                };
            }
            Conditional {
                condition,
                true_branch,
                false_branch,
            }
        }
        WLoop { condition, body } => {
            let condition = Box::new(fold(*condition));
            let body = Box::new(fold(*body));
            if let Literal(Value::Bool(false)) = condition.as_ref() {
                return Literal(Value::unit());
            }
            WLoop { condition, body }
        }
        NLoop { count, body } => {
            if let AstNode::Literal(Value::Int(n)) = count.as_ref()
                && *n <= 0
            {
                return Literal(Value::unit());
            }
            NLoop {
                count: Box::new(fold(*count)),
                body: Box::new(fold(*body)),
            }
        }
        FLoop { iterable, body } => FLoop {
            iterable: Box::new(fold(*iterable)),
            body: Box::new(fold(*body)),
        },
        Return(expr) => Return(expr.map(|e| Box::new(fold(*e)))),
        // Assert(expr) => Assert(Box::new(fold(*expr))),
        Try(expr) => Try(Box::new(fold(*expr))),
        Block(stmts) => Block(stmts.into_iter().map(fold).collect()),
        BlockExpr(stmts) => BlockExpr(stmts.into_iter().map(fold).collect()),
    }
}

#[cfg(test)]
mod tests {
    use crate::astnode::BinaryOperator;

    use super::*;

    #[test]
    fn folds_simple_addition() {
        let ast = AstNode::BinaryOp {
            left: Box::new(AstNode::Literal(Value::Int(1))),
            operator: BinaryOperator::Add,
            right: Box::new(AstNode::Literal(Value::Int(1))),
        };
        let folded = fold(ast);
        assert_eq!(folded, AstNode::Literal(Value::Int(2)));
    }

    #[test]
    fn folds_list_addition_to_int_list() {
        let l1 = AstNode::List(vec![
            AstNode::Literal(Value::Int(1)),
            AstNode::Literal(Value::Int(2)),
            AstNode::Literal(Value::Int(3)),
            AstNode::Literal(Value::Int(4)),
        ]);
        let l2 = AstNode::List(vec![
            AstNode::Literal(Value::Int(3)),
            AstNode::Literal(Value::Int(4)),
            AstNode::Literal(Value::Int(5)),
            AstNode::Literal(Value::Int(6)),
        ]);
        let ast = AstNode::BinaryOp {
            left: Box::new(l1),
            operator: BinaryOperator::Add,
            right: Box::new(l2),
        };
        let folded = fold(ast);
        assert_eq!(folded, AstNode::Literal(Value::IntList(vec![4, 6, 8, 10])));
    }

    #[test]
    fn folds_range_literal_half_open() {
        let ast = AstNode::Range {
            start: Box::new(AstNode::Literal(Value::Int(1))),
            end: Box::new(AstNode::Literal(Value::Int(5))),
            step: None,
            inclusive: false,
        };
        let folded = fold(ast);
        assert_eq!(folded, AstNode::Literal(Value::IntList(vec![1, 2, 3, 4])));
    }

    #[test]
    fn folds_range_literal_inclusive_with_step() {
        let ast = AstNode::Range {
            start: Box::new(AstNode::Literal(Value::Int(1))),
            end: Box::new(AstNode::Literal(Value::Int(5))),
            step: Some(Box::new(AstNode::Literal(Value::Int(2)))),
            inclusive: true,
        };
        let folded = fold(ast);
        assert_eq!(folded, AstNode::Literal(Value::IntList(vec![1, 3, 5])));
    }
}
