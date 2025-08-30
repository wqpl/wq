use crate::astnode::AstNode;
use crate::builtins::Builtins;
use crate::value::Value;
use std::collections::{HashMap, HashSet};

/// Expression resolver that lowers certain postfix patterns into explicit
/// Call / CallAnonymous / Index nodes.
pub struct Resolver {
    builtins: Builtins,
    known_funcs: HashSet<String>,
    known_indexables: HashSet<String>,
    in_func: usize,
}

impl Resolver {
    pub fn new() -> Self {
        Self {
            builtins: Builtins::new(),
            known_funcs: HashSet::new(),
            known_indexables: HashSet::new(),
            in_func: 0,
        }
    }

    /// Create a resolver that knows about functions and indexables defined in `env`.
    pub fn from_env(env: &HashMap<String, Value>) -> Self {
        let mut res = Self::new();
        for (name, val) in env {
            match val {
                Value::Function { .. } | Value::CompiledFunction { .. } => {
                    res.known_funcs.insert(name.clone());
                    res.known_indexables.remove(name);
                }
                Value::List(..) | Value::Dict(..) => {
                    res.known_indexables.insert(name.clone());
                    res.known_funcs.remove(name);
                }
                _ => {
                    // Unknown/other runtime values; leave as-is.
                }
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
                // Resolve RHS first
                let value = Box::new(self.resolve_node(*value));

                // Maintain `known_funcs` and `known_indexables` based on the resolved AST
                if matches!(&*value, AstNode::Function { .. }) {
                    self.known_funcs.insert(name.clone());
                    self.known_indexables.remove(&name);
                } else {
                    // Update entries
                    self.known_funcs.remove(&name);

                    if self.is_indexable_ast(&value) {
                        self.known_indexables.insert(name.clone());
                    } else {
                        self.known_indexables.remove(&name);
                    }
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

                // 1) If explicitly called or definitely callable, lower to Call / CallAnonymous.
                if explicit_call || self.should_call(&object) {
                    if let AstNode::Variable(name) = *object.clone() {
                        return AstNode::Call { name, args: items };
                    } else {
                        return AstNode::CallAnonymous {
                            object,
                            args: items,
                        };
                    }
                }

                // 2) If definitely indexable, lower to Index.
                if self.should_index(&object, &items) {
                    let idx = if items.len() == 1 {
                        Box::new(items.into_iter().next().unwrap())
                    } else {
                        Box::new(AstNode::List(items))
                    };
                    return AstNode::Index { object, index: idx };
                }

                // 3) Otherwise, preserve Postfix
                AstNode::Postfix {
                    object,
                    items,
                    explicit_call: false,
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

            AstNode::Function { params, body } => {
                self.in_func += 1;
                let body = Box::new(self.resolve_node(*body));
                self.in_func -= 1;
                AstNode::Function { params, body }
            }

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
            AstNode::Try(e) => AstNode::Try(Box::new(self.resolve_node(*e))),
            AstNode::Block(stmts) => {
                AstNode::Block(stmts.into_iter().map(|s| self.resolve_node(s)).collect())
            }
            other => other,
        }
    }

    fn should_index(&self, object: &AstNode, items: &[AstNode]) -> bool {
        if items.is_empty() {
            return false;
        }
        match object {
            AstNode::List(_) | AstNode::Dict(_) => true,
            AstNode::Variable(name) => self.known_indexables.contains(name),
            _ => false,
        }
    }

    fn is_indexable_ast(&self, node: &AstNode) -> bool {
        matches!(node, AstNode::List(_) | AstNode::Dict(_))
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
