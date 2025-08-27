use crate::parser::{AstNode, BinaryOperator, UnaryOperator};
use crate::value::Value;

pub fn fold(node: AstNode) -> AstNode {
    use AstNode::*;
    match node {
        Literal(_) | Variable(_) | Break | Continue => node,
        UnaryOp { operator, operand } => {
            let operand = Box::new(fold(*operand));
            if let Literal(v) = operand.as_ref() {
                if let Some(res) = eval_unary(operator.clone(), v.clone()) {
                    return Literal(res);
                }
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
            if let (Literal(lv), Literal(rv)) = (&*left, &*right) {
                if let Some(res) = eval_binary(operator.clone(), lv.clone(), rv.clone()) {
                    return Literal(res);
                }
            }
            BinaryOp {
                left,
                operator,
                right,
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
                if values.iter().all(|v| matches!(v, Value::Int(_))) {
                    let ints: Vec<i64> = values
                        .into_iter()
                        .map(|v| {
                            if let Value::Int(i) = v {
                                i
                            } else {
                                unreachable!()
                            }
                        })
                        .collect();
                    Literal(Value::IntList(ints))
                } else {
                    Literal(Value::List(values))
                }
            } else {
                List(items)
            }
        }
        Dict(pairs) => {
            let pairs: Vec<(String, AstNode)> =
                pairs.into_iter().map(|(k, v)| (k, fold(v))).collect();
            Dict(pairs)
        }
        Assignment { name, value } => Assignment {
            name,
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
        } => Conditional {
            condition: Box::new(fold(*condition)),
            true_branch: Box::new(fold(*true_branch)),
            false_branch: false_branch.map(|b| Box::new(fold(*b))),
        },
        WhileLoop { condition, body } => WhileLoop {
            condition: Box::new(fold(*condition)),
            body: Box::new(fold(*body)),
        },
        ForLoop { count, body } => ForLoop {
            count: Box::new(fold(*count)),
            body: Box::new(fold(*body)),
        },
        Return(expr) => Return(expr.map(|e| Box::new(fold(*e)))),
        Assert(expr) => Assert(Box::new(fold(*expr))),
        Try(expr) => Try(Box::new(fold(*expr))),
        Block(stmts) => Block(stmts.into_iter().map(fold).collect()),
    }
}

fn eval_unary(op: UnaryOperator, val: Value) -> Option<Value> {
    use UnaryOperator::*;
    match op {
        Negate => val.neg_value(),
        Count => Some(Value::Int(val.len() as i64)),
    }
}

fn eval_binary(op: BinaryOperator, left: Value, right: Value) -> Option<Value> {
    use BinaryOperator::*;
    match op {
        Add => left.add(&right),
        Subtract => left.subtract(&right),
        Multiply => left.multiply(&right),
        Power => left.power(&right),
        Divide => left.divide(&right),
        DivideDot => left.divide_dot(&right),
        Modulo => left.modulo(&right),
        ModuloDot => left.modulo_dot(&right),
        Equal => Some(left.equals(&right)),
        NotEqual => Some(left.not_equals(&right)),
        LessThan => Some(left.less_than(&right)),
        LessThanOrEqual => Some(left.less_than_or_equal(&right)),
        GreaterThan => Some(left.greater_than(&right)),
        GreaterThanOrEqual => Some(left.greater_than_or_equal(&right)),
    }
}

#[cfg(test)]
mod tests {
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
        if let AstNode::Literal(val) = folded {
            assert_eq!(val.type_name_verbose(), "intlist");
        } else {
            panic!("expected literal");
        }
    }
}
