use crate::builtins::Builtins;
use crate::parser::AstNode;
use crate::value::valuei::Value;
use std::collections::{HashMap, HashSet};

pub struct Resolver {
    builtins: Builtins,
    known_funcs: HashSet<String>,
}

impl Resolver {
    pub fn new() -> Self {
        Self {
            builtins: Builtins::new(),
            known_funcs: HashSet::new(),
        }
    }

    /// Create a resolver that knows about functions defined in `env`.
    pub fn from_env(env: &HashMap<String, Value>) -> Self {
        let mut res = Self::new();
        for (name, val) in env {
            if matches!(val, Value::Function { .. } | Value::BytecodeFunction { .. }) {
                res.known_funcs.insert(name.clone());
            }
        }
        res
    }

    pub fn resolve(&mut self, node: AstNode) -> AstNode {
        self.resolve_node(node)
    }

    fn resolve_node(&mut self, node: AstNode) -> AstNode {
        match node {
            AstNode::Assignment { name, value } => {
                let value = Box::new(self.resolve_node(*value));
                if matches!(&*value, AstNode::Function { .. }) {
                    self.known_funcs.insert(name.clone());
                }
                AstNode::Assignment { name, value }
            }
            AstNode::Postfix {
                object,
                items,
                explicit_call,
            } => {
                let object = Box::new(self.resolve_node(*object));
                let items: Vec<_> = items.into_iter().map(|n| self.resolve_node(n)).collect();
                if explicit_call || self.should_call(&object) {
                    if let AstNode::Variable(name) = *object.clone() {
                        AstNode::Call { name, args: items }
                    } else {
                        AstNode::CallAnonymous {
                            object,
                            args: items,
                        }
                    }
                } else {
                    let idx = if items.len() == 1 {
                        Box::new(items.into_iter().next().unwrap())
                    } else {
                        Box::new(AstNode::List(items))
                    };
                    AstNode::Index { object, index: idx }
                }
            }
            AstNode::BinaryOp {
                left,
                operator,
                right,
            } => AstNode::BinaryOp {
                left: Box::new(self.resolve_node(*left)),
                operator,
                right: Box::new(self.resolve_node(*right)),
            },
            AstNode::UnaryOp { operator, operand } => AstNode::UnaryOp {
                operator,
                operand: Box::new(self.resolve_node(*operand)),
            },
            AstNode::List(items) => {
                AstNode::List(items.into_iter().map(|n| self.resolve_node(n)).collect())
            }
            AstNode::Dict(pairs) => AstNode::Dict(
                pairs
                    .into_iter()
                    .map(|(k, v)| (k, self.resolve_node(v)))
                    .collect(),
            ),
            AstNode::Call { name, args } => AstNode::Call {
                name,
                args: args.into_iter().map(|n| self.resolve_node(n)).collect(),
            },
            AstNode::CallAnonymous { object, args } => AstNode::CallAnonymous {
                object: Box::new(self.resolve_node(*object)),
                args: args.into_iter().map(|n| self.resolve_node(n)).collect(),
            },
            AstNode::Index { object, index } => AstNode::Index {
                object: Box::new(self.resolve_node(*object)),
                index: Box::new(self.resolve_node(*index)),
            },
            AstNode::IndexAssign {
                object,
                index,
                value,
            } => AstNode::IndexAssign {
                object: Box::new(self.resolve_node(*object)),
                index: Box::new(self.resolve_node(*index)),
                value: Box::new(self.resolve_node(*value)),
            },
            AstNode::Function { params, body } => AstNode::Function {
                params,
                body: Box::new(self.resolve_node(*body)),
            },
            AstNode::Conditional {
                condition,
                true_branch,
                false_branch,
            } => AstNode::Conditional {
                condition: Box::new(self.resolve_node(*condition)),
                true_branch: Box::new(self.resolve_node(*true_branch)),
                false_branch: false_branch.map(|b| Box::new(self.resolve_node(*b))),
            },
            AstNode::WhileLoop { condition, body } => AstNode::WhileLoop {
                condition: Box::new(self.resolve_node(*condition)),
                body: Box::new(self.resolve_node(*body)),
            },
            AstNode::ForLoop { count, body } => AstNode::ForLoop {
                count: Box::new(self.resolve_node(*count)),
                body: Box::new(self.resolve_node(*body)),
            },
            AstNode::Return(expr) => AstNode::Return(expr.map(|e| Box::new(self.resolve_node(*e)))),
            AstNode::Assert(e) => AstNode::Assert(Box::new(self.resolve_node(*e))),
            AstNode::Block(stmts) => {
                AstNode::Block(stmts.into_iter().map(|s| self.resolve_node(s)).collect())
            }
            other => other,
        }
    }

    fn should_call(&self, object: &AstNode) -> bool {
        match object {
            AstNode::Variable(name) => {
                self.builtins.has_function(name) || self.known_funcs.contains(name)
            }
            AstNode::Function { .. } => true,
            _ => false,
        }
    }
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}
