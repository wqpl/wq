use std::collections::{HashMap, HashSet};

use crate::ast::{AstNode, AstSpan, BinaryOperator, Parameter, PipeKind};
use crate::builtins::Builtins;
use crate::compile::function_ref_capture_names;
use crate::symbol::{DefKind, SymbolIndex};
use crate::value::{IntoWqValue, Value};
use crate::vm::GlobalMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BindingFact {
    Unknown,
    Callable,
    Container,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum BindingId {
    Symbol(usize),
    Runtime(String),
    Synthetic(usize),
}

#[derive(Clone)]
struct ResolverSnapshot {
    scopes: Vec<HashMap<String, BindingId>>,
    binding_facts: HashMap<BindingId, BindingFact>,
}

/// Expression resolver that lowers certain postfix patterns into explicit
/// Call/CallAnonymous/Index nodes.
pub(crate) struct Resolver {
    builtins: Builtins,
    runtime_seed_facts: HashMap<String, BindingFact>,
    scopes: Vec<HashMap<String, BindingId>>,
    binding_facts: HashMap<BindingId, BindingFact>,
    symbol_defs_by_span: HashMap<(String, AstSpan), usize>,
    symbol_def_names: Vec<String>,
    gensym: usize,
    binding_gensym: usize,
}

impl Resolver {
    pub(crate) fn new() -> Self {
        Self::with_builtins(Builtins::new())
    }

    pub(crate) fn with_builtins(builtins: Builtins) -> Self {
        Self {
            builtins,
            runtime_seed_facts: HashMap::new(),
            scopes: Vec::new(),
            binding_facts: HashMap::new(),
            symbol_defs_by_span: HashMap::new(),
            symbol_def_names: Vec::new(),
            gensym: 0,
            binding_gensym: 0,
        }
    }

    pub(crate) fn from_env(env: GlobalMap, builtins: Builtins) -> Self {
        let mut res = Self::with_builtins(builtins);
        for (name, val) in env {
            let fact = Self::fact_from_value(&val);
            if fact != BindingFact::Unknown {
                res.runtime_seed_facts.insert(name, fact);
            }
        }
        res
    }

    pub(crate) fn resolve(&mut self, node: AstNode) -> AstNode {
        let symbols = SymbolIndex::analyze(&node, &self.builtins);
        self.load_symbols(&symbols);
        self.resolve_node(node)
    }

    fn invalid_mutating_index_target(span: AstSpan) -> AstNode {
        AstNode::Error(
            crate::wqerror::WqError::new(crate::wqerror::WqErrorType::Syntax)
                .src("resolver")
                .msg("bang indexing can mutate only a variable")
                .attach_note("assign the container to a variable before using '[!...]'"),
            span,
        )
    }

    fn invalid_unpack_target(span: AstSpan) -> AstNode {
        AstNode::Error(
            crate::wqerror::WqError::new(crate::wqerror::WqErrorType::Syntax)
                .src("resolver")
                .msg(
                    "unpack assignment target must be an identifier, index target, '_', '...', or a nested list",
                ),
            span,
        )
    }

    fn resolve_node(&mut self, node: AstNode) -> AstNode {
        match node {
            AstNode::Assignment {
                name,
                op,
                value,
                span,
                name_span,
            } => {
                let binding = self.binding_for_named_def(&name, name_span);
                let mut captured_by_ref = None;
                let value = if let AstNode::Function {
                    params,
                    ref_capture,
                    body,
                    span: function_span,
                } = *value
                {
                    self.bind_current_scope(name.clone(), binding.clone());
                    self.set_binding_fact(&binding, BindingFact::Callable);

                    let value = self.resolve_function(
                        params,
                        ref_capture,
                        *body,
                        function_span,
                        Some((name.clone(), binding.clone())),
                    );

                    if let AstNode::Function {
                        params,
                        ref_capture,
                        body,
                        ..
                    } = &value
                    {
                        captured_by_ref = Some(function_ref_capture_names(
                            body,
                            params.as_deref(),
                            *ref_capture,
                            Some(name.as_str()),
                        ));
                    }

                    Box::new(value)
                } else {
                    Box::new(self.resolve_node(*value))
                };

                self.bind_current_scope(name.clone(), binding.clone());
                self.set_binding_fact(&binding, self.fact_from_ast(&value));
                if let Some(captured_by_ref) = captured_by_ref {
                    self.invalidate_bindings_named(captured_by_ref);
                }

                AstNode::Assignment {
                    name,
                    op,
                    value,
                    span,
                    name_span,
                }
            }
            AstNode::OuterAssignment {
                name,
                op,
                value,
                span,
                name_span,
            } => {
                let value = Box::new(self.resolve_node(*value));
                if let Some(binding) = self.lookup_outer_binding(&name) {
                    self.set_binding_fact(&binding, self.fact_from_ast(&value));
                }
                AstNode::OuterAssignment {
                    name,
                    op,
                    value,
                    span,
                    name_span,
                }
            }
            AstNode::Postfix {
                object,
                items,
                explicit_call,
                depth,
                span,
            } => {
                let object = Box::new(self.resolve_node(*object));
                let items: Vec<_> = items.into_iter().map(|n| self.resolve_node(n)).collect();
                if depth.is_some() {
                    return AstNode::Postfix {
                        object,
                        items,
                        explicit_call: true,
                        depth,
                        span,
                    };
                }
                // 1) If explicitly called or definitely callable, lower to Call /
                //    CallAnonymous.
                if explicit_call || self.should_call(&object) {
                    if let AstNode::Variable(name, var_span) = *object.clone() {
                        return AstNode::CallName {
                            name,
                            args: items,
                            span,
                            name_span: var_span,
                        };
                    } else {
                        return AstNode::CallAnonymous {
                            object,
                            args: items,
                            span,
                        };
                    }
                }
                // 2) If definitely indexable and no named args, lower to Index. Named args
                //    force the call-path so the VM can error.
                let has_named_args = items.iter().any(|n| matches!(n, AstNode::NamedArg { .. }));
                if !has_named_args && self.should_index(&object, &items) {
                    let idx = if items.len() == 1 {
                        Box::new(items.into_iter().next().unwrap())
                    } else {
                        Box::new(AstNode::List(items, None))
                    };
                    return AstNode::Index {
                        object,
                        index: idx,
                        span,
                    };
                }
                // 3) Otherwise, preserve Postfix
                AstNode::Postfix {
                    object,
                    items,
                    explicit_call: false,
                    depth: None,
                    span,
                }
            }
            AstNode::Pipe {
                input,
                effect,
                kind,
                span,
            } => {
                let input = self.resolve_node(*input);
                let effect = self.resolve_node(*effect);
                match kind {
                    PipeKind::Pipe => Self::apply_pipe_rhs(effect, input, true, span),
                    PipeKind::PipePipe => Self::apply_pipe_rhs(effect, input, false, span),
                    PipeKind::PipeDot => {
                        let effect = Self::apply_pipe_rhs(effect, AstNode::PipeInput, true, span);
                        AstNode::PipeTap {
                            input: Box::new(input),
                            effect: Box::new(effect),
                            span,
                        }
                    }
                    PipeKind::PipePipeDot => {
                        let effect = Self::apply_pipe_rhs(effect, AstNode::PipeInput, false, span);
                        AstNode::PipeTap {
                            input: Box::new(input),
                            effect: Box::new(effect),
                            span,
                        }
                    }
                }
            }
            AstNode::PipeTap {
                input,
                effect,
                span,
            } => AstNode::PipeTap {
                input: Box::new(self.resolve_node(*input)),
                effect: Box::new(self.resolve_node(*effect)),
                span,
            },
            AstNode::BinaryOp {
                left,
                operator,
                right,
                span,
            } => self.resolve_binary_chain(*left, operator, *right, span),
            AstNode::LazyBool {
                operator,
                operands,
                span,
            } => AstNode::LazyBool {
                operator,
                operands: operands
                    .into_iter()
                    .map(|operand| self.resolve_node(operand))
                    .collect(),
                span,
            },
            AstNode::ComparisonChain { first, rest, span } => AstNode::ComparisonChain {
                first: Box::new(self.resolve_node(*first)),
                rest: rest
                    .into_iter()
                    .map(|(op, node)| (op, self.resolve_node(node)))
                    .collect(),
                span,
            },
            AstNode::Range {
                start,
                end,
                step,
                inclusive,
                span,
            } => AstNode::Range {
                start: Box::new(self.resolve_node(*start)),
                end: Box::new(self.resolve_node(*end)),
                step: step.map(|s| Box::new(self.resolve_node(*s))),
                inclusive,
                span,
            },
            AstNode::UnaryOp {
                operator,
                operand,
                span,
            } => AstNode::UnaryOp {
                operator,
                operand: Box::new(self.resolve_node(*operand)),
                span,
            },
            AstNode::Group { expr, span } => AstNode::Group {
                expr: Box::new(self.resolve_node(*expr)),
                span,
            },
            AstNode::Cat(items, span) => AstNode::Cat(
                items.into_iter().map(|n| self.resolve_node(n)).collect(),
                span,
            ),
            AstNode::List(items, span) => AstNode::List(
                items.into_iter().map(|n| self.resolve_node(n)).collect(),
                span,
            ),
            AstNode::Dict(pairs, span) => AstNode::Dict(
                pairs
                    .into_iter()
                    .map(|(k, v)| (k, self.resolve_node(v)))
                    .collect(),
                span,
            ),
            AstNode::CallName {
                name,
                args,
                span,
                name_span,
            } => AstNode::CallName {
                name,
                args: args.into_iter().map(|n| self.resolve_node(n)).collect(),
                span,
                name_span,
            },
            AstNode::CallAnonymous { object, args, span } => AstNode::CallAnonymous {
                object: Box::new(self.resolve_node(*object)),
                args: args.into_iter().map(|n| self.resolve_node(n)).collect(),
                span,
            },
            AstNode::Index {
                object,
                index,
                span,
            } => AstNode::Index {
                object: Box::new(self.resolve_node(*object)),
                index: Box::new(self.resolve_node(*index)),
                span,
            },
            AstNode::MutatingIndex {
                object,
                index,
                span,
            } => {
                let object = Box::new(self.resolve_node(*object));
                let index = Box::new(self.resolve_node(*index));
                if !matches!(
                    object.as_ref(),
                    AstNode::Variable(_, _) | AstNode::OuterVariable(_, _)
                ) {
                    return Self::invalid_mutating_index_target(span);
                }
                AstNode::MutatingIndex {
                    object,
                    index,
                    span,
                }
            }
            AstNode::IndexAssign {
                object,
                index,
                op,
                value,
                span,
            } => AstNode::IndexAssign {
                object: Box::new(self.resolve_node(*object)),
                index: Box::new(self.resolve_node(*index)),
                op,
                value: Box::new(self.resolve_node(*value)),
                span,
            },
            AstNode::MutatingIndexAssign {
                object,
                index,
                value,
                span,
            } => {
                let object = Box::new(self.resolve_node(*object));
                let index = Box::new(self.resolve_node(*index));
                let value = Box::new(self.resolve_node(*value));
                if !matches!(
                    object.as_ref(),
                    AstNode::Variable(_, _) | AstNode::OuterVariable(_, _)
                ) {
                    return Self::invalid_mutating_index_target(span);
                }
                AstNode::MutatingIndexAssign {
                    object,
                    index,
                    value,
                    span,
                }
            }
            AstNode::Function {
                params,
                ref_capture,
                body,
                span,
            } => self.resolve_function(params, ref_capture, *body, span, None),
            AstNode::ConditionalDot {
                condition,
                true_branch,
                span,
            } => AstNode::Conditional {
                condition: Box::new(self.resolve_node(*condition)),
                true_branch: Box::new(self.resolve_node(*true_branch)),
                false_branch: None,
                span,
            },
            AstNode::Conditional {
                condition,
                true_branch,
                false_branch,
                span,
            } => {
                let condition = Box::new(self.resolve_node(*condition));
                let branch_base = self.snapshot();
                let (true_branch, true_state) =
                    self.resolve_from_snapshot(&branch_base, *true_branch);
                let (false_branch, false_state) = if let Some(false_branch) = false_branch {
                    let (false_branch, false_state) =
                        self.resolve_from_snapshot(&branch_base, *false_branch);
                    (Some(Box::new(false_branch)), false_state)
                } else {
                    (None, branch_base.clone())
                };
                let merged_state = self.merge_snapshots(&true_state, &false_state);
                self.restore(merged_state);
                AstNode::Conditional {
                    condition,
                    true_branch: Box::new(true_branch),
                    false_branch,
                    span,
                }
            }
            AstNode::ConditionalChain {
                pairs,
                default_branch,
                span,
            } => {
                let resolved_pairs: Vec<(AstNode, AstNode)> = pairs
                    .into_iter()
                    .map(|(cond, branch)| (self.resolve_node(cond), self.resolve_node(branch)))
                    .collect();
                let resolved_default = Box::new(self.resolve_node(*default_branch));
                // Desugar into nested Conditional nodes
                let mut acc = *resolved_default;
                for (cond, branch) in resolved_pairs.into_iter().rev() {
                    acc = AstNode::Conditional {
                        condition: Box::new(cond),
                        true_branch: Box::new(branch),
                        false_branch: Some(Box::new(acc)),
                        span,
                    };
                }
                acc
            }
            AstNode::WLoop {
                condition,
                body,
                span,
            } => {
                let condition = Box::new(self.resolve_node(*condition));
                let loop_base = self.snapshot();
                let (body, body_state) = self.resolve_from_snapshot(&loop_base, *body);
                let merged_state = self.merge_snapshots(&loop_base, &body_state);
                self.restore(merged_state);
                AstNode::WLoop {
                    condition,
                    body: Box::new(body),
                    span,
                }
            }
            AstNode::NLoop { count, body, span } => {
                let count = Box::new(self.resolve_node(*count));
                let loop_base = self.snapshot();
                let (body, body_state) = self.resolve_from_snapshot(&loop_base, *body);
                let merged_state = self.merge_snapshots(&loop_base, &body_state);
                self.restore(merged_state);
                AstNode::NLoop {
                    count,
                    body: Box::new(body),
                    span,
                }
            }
            AstNode::Debug { expr, span } => AstNode::Debug {
                expr: Box::new(self.resolve_node(*expr)),
                span,
            },
            AstNode::Pause { expr, span } => AstNode::Pause {
                expr: expr.map(|expr| Box::new(self.resolve_node(*expr))),
                span,
            },
            AstNode::Return(expr, span) => {
                AstNode::Return(expr.map(|e| Box::new(self.resolve_node(*e))), span)
            }
            AstNode::Try(e, span) => AstNode::Try(Box::new(self.resolve_node(*e)), span),
            AstNode::Block(stmts, span) => AstNode::Block(
                stmts.into_iter().map(|s| self.resolve_node(s)).collect(),
                span,
            ),
            AstNode::BlockExpr(stmts, span) => AstNode::BlockExpr(
                stmts.into_iter().map(|s| self.resolve_node(s)).collect(),
                span,
            ),
            AstNode::UnpackAssignment { lhs, op, rhs, span } => {
                let rhs = Box::new(self.resolve_node(*rhs));
                let lhs = lhs.into_iter().map(|n| self.resolve_node(n)).collect();
                self.expand_unpack_assignment(lhs, op, *rhs, span)
            }
            AstNode::FString { parts, span } => {
                let mut template = String::new();
                let mut args: Vec<AstNode> = Vec::new();
                for part in parts {
                    match part {
                        crate::ast::FStringPart::Text(t) => {
                            template.push_str(&t);
                        }
                        crate::ast::FStringPart::Expr {
                            expr,
                            encoded_spec,
                            spec_exprs,
                            ..
                        } => {
                            let resolved_expr = self.resolve_node(expr);
                            for spec_expr in spec_exprs {
                                args.push(self.resolve_node(spec_expr));
                            }
                            if let Some(enc) = encoded_spec {
                                template.push_str("{[");
                                template.push_str(&enc);
                                template.push_str("]}");
                            } else {
                                template.push_str("{}");
                            }
                            args.push(resolved_expr);
                        }
                    }
                }
                let mut fmt_args = Vec::with_capacity(1 + args.len());
                fmt_args.push(AstNode::Literal(template.into_wq_value(), None));
                fmt_args.extend(args);
                AstNode::CallName {
                    name: crate::builtins::BuiltinEnum::Fmt.name().into(),
                    args: fmt_args,
                    span,
                    name_span: None,
                }
            }
            AstNode::NamedArg { name, value, span } => AstNode::NamedArg {
                name,
                value: Box::new(self.resolve_node(*value)),
                span,
            },
            AstNode::PipeInput => AstNode::PipeInput,
            AstNode::OuterVariable(name, span) => AstNode::OuterVariable(name, span),
            other => other,
        }
    }

    fn resolve_binary_chain(
        &mut self,
        left: AstNode,
        operator: BinaryOperator,
        right: AstNode,
        span: AstSpan,
    ) -> AstNode {
        let mut chain = vec![(operator, right, span)];
        let mut current = left;
        let mut resolved = loop {
            match current {
                AstNode::BinaryOp {
                    left,
                    operator,
                    right,
                    span,
                } => {
                    chain.push((operator, *right, span));
                    current = *left;
                }
                other => break self.resolve_node(other),
            }
        };

        for (operator, right, span) in chain.into_iter().rev() {
            resolved = AstNode::BinaryOp {
                left: Box::new(resolved),
                operator,
                right: Box::new(self.resolve_node(right)),
                span,
            };
        }
        resolved
    }

    fn expand_unpack_assignment(
        &mut self,
        lhs: Vec<AstNode>,
        op: Option<crate::ast::BinaryOperator>,
        rhs: AstNode,
        span: AstSpan,
    ) -> AstNode {
        // Optimization: if rhs is a literal list/dict and no augmented op,
        // expand directly without creating a temporary.
        if op.is_none() {
            let values_opt = match &rhs {
                AstNode::List(items, _) => Some(items.clone()),
                AstNode::Dict(pairs, _) => Some(pairs.iter().map(|(_, v)| v.clone()).collect()),
                _ => None,
            };

            if let Some(values) = values_opt
                && let Some(stmts) = self.try_lower_literal_unpack(&lhs, values, span)
            {
                return AstNode::Block(stmts, span);
            }
        }

        let tmp_name = format!("--resolver-unpack-{}", self.gensym);
        self.gensym += 1;
        let mut stmts = vec![AstNode::Assignment {
            name: tmp_name.clone(),
            op: None,
            value: Box::new(rhs),
            span,
            name_span: span,
        }];
        let ellipsis_idx = lhs.iter().position(|n| matches!(n, AstNode::Ellipsis(_)));
        for (pos, item) in lhs.iter().enumerate() {
            if matches!(item, AstNode::Ellipsis(_)) {
                break;
            }
            stmts.extend(self.make_unpack_assign(item, &tmp_name, pos as i64, span, op));
        }
        if let Some(ei) = ellipsis_idx {
            for (offset, item) in lhs.iter().skip(ei + 1).enumerate() {
                let pos = -1 - (offset as i64);
                stmts.extend(self.make_unpack_assign(item, &tmp_name, pos, span, op));
            }
        }
        AstNode::Block(stmts, span)
    }

    /// Try to lower an unpack assignment when RHS is a literal list/dict.
    /// Returns `None` if the expansion would change evaluation order semantics
    /// or if lengths are incompatible, so the caller should fall back to
    /// temp+index.
    fn try_lower_literal_unpack(
        &mut self,
        lhs: &[AstNode],
        values: Vec<AstNode>,
        span: AstSpan,
    ) -> Option<Vec<AstNode>> {
        let bound_names = collect_bound_names(lhs);
        let ellipsis_idx = lhs.iter().position(|n| matches!(n, AstNode::Ellipsis(_)));

        // Length check
        match ellipsis_idx {
            None => {
                if lhs.len() > values.len() {
                    return None;
                }
            }
            Some(ei) => {
                let suffix_len = lhs.len() - ei - 1;
                if ei > values.len() || suffix_len > values.len() {
                    return None;
                }
            }
        }

        // Safety check: no value may reference a variable bound at a prior
        // position, because sequential assignment would see the updated value.
        for (i, value) in values.iter().enumerate() {
            let mut forbidden = HashSet::new();
            for (pos, name) in &bound_names {
                if *pos < i {
                    forbidden.insert(name.as_str());
                }
            }
            if expr_uses_vars(value, &forbidden) {
                return None;
            }
        }

        let mut stmts = Vec::new();
        self.lower_literal_unpack_items(lhs, &values, span, &mut stmts, &bound_names)?;
        Some(stmts)
    }

    fn lower_literal_unpack_items(
        &mut self,
        lhs: &[AstNode],
        values: &[AstNode],
        span: AstSpan,
        stmts: &mut Vec<AstNode>,
        bound_names: &[(usize, String)],
    ) -> Option<()> {
        let ellipsis_idx = lhs.iter().position(|n| matches!(n, AstNode::Ellipsis(_)));

        // Prefix items
        for (i, item) in lhs.iter().enumerate() {
            if matches!(item, AstNode::Ellipsis(_)) {
                break;
            }
            let value = values.get(i)?.clone();
            self.lower_literal_unpack_item(item, value, span, stmts, bound_names, i);
        }

        // Suffix items (after ellipsis)
        if let Some(ei) = ellipsis_idx {
            let suffix_len = lhs.len() - ei - 1;
            for (offset, item) in lhs.iter().skip(ei + 1).enumerate() {
                let idx = values.len() - suffix_len + offset;
                let value = values.get(idx)?.clone();
                self.lower_literal_unpack_item(item, value, span, stmts, bound_names, idx);
            }
        }

        Some(())
    }

    fn lower_literal_unpack_item(
        &mut self,
        item: &AstNode,
        value: AstNode,
        span: AstSpan,
        stmts: &mut Vec<AstNode>,
        bound_names: &[(usize, String)],
        pos: usize,
    ) {
        match item {
            AstNode::Variable(name, item_span) if name == "_" => {}
            AstNode::Variable(name, item_span) => {
                stmts.push(AstNode::Assignment {
                    name: name.clone(),
                    op: None,
                    value: Box::new(value),
                    span: *item_span,
                    name_span: *item_span,
                });
            }
            AstNode::Index { object, index, .. } => {
                stmts.push(AstNode::IndexAssign {
                    object: object.clone(),
                    index: index.clone(),
                    op: None,
                    value: Box::new(value),
                    span,
                });
            }
            AstNode::Postfix {
                object,
                items,
                explicit_call: false,
                ..
            } => {
                let index = if items.len() == 1 {
                    Box::new(items[0].clone())
                } else {
                    Box::new(AstNode::List(items.clone(), None))
                };
                stmts.push(AstNode::IndexAssign {
                    object: object.clone(),
                    index,
                    op: None,
                    value: Box::new(value),
                    span,
                });
            }
            AstNode::Ellipsis(_) => {}
            AstNode::List(inner_items, _) => {
                // Try recursive optimization if value is also a literal list/dict
                let sub_values_opt = match &value {
                    AstNode::List(inner, _) => Some(inner.clone()),
                    AstNode::Dict(pairs, _) => Some(pairs.iter().map(|(_, v)| v.clone()).collect()),
                    _ => None,
                };

                if let Some(sub_values) = sub_values_opt {
                    let mut prior_forbidden = HashSet::new();
                    for (p, name) in bound_names {
                        if *p < pos {
                            prior_forbidden.insert(name.as_str());
                        }
                    }
                    if let Some(sub_stmts) = self.try_lower_literal_unpack_nested(
                        inner_items,
                        sub_values,
                        span,
                        &prior_forbidden,
                    ) {
                        stmts.extend(sub_stmts);
                        return;
                    }
                }

                // Fallback: temp + index for this nested pattern
                let sub_tmp = format!("--resolver-unpack-{}", self.gensym);
                self.gensym += 1;
                stmts.push(AstNode::Assignment {
                    name: sub_tmp.clone(),
                    op: None,
                    value: Box::new(value),
                    span,
                    name_span: span,
                });
                let ellipsis_idx = inner_items
                    .iter()
                    .position(|n| matches!(n, AstNode::Ellipsis(_)));
                for (pos, item) in inner_items.iter().enumerate() {
                    if matches!(item, AstNode::Ellipsis(_)) {
                        break;
                    }
                    stmts.extend(self.make_unpack_assign(item, &sub_tmp, pos as i64, span, None));
                }
                if let Some(ei) = ellipsis_idx {
                    for (offset, item) in inner_items.iter().skip(ei + 1).enumerate() {
                        let pos = -1 - (offset as i64);
                        stmts.extend(self.make_unpack_assign(item, &sub_tmp, pos, span, None));
                    }
                }
            }
            _ => {
                stmts.push(Self::invalid_unpack_target(span));
            }
        }
    }

    /// Like `try_lower_literal_unpack` but with an additional set of names
    /// that must not be referenced because they were bound in an outer
    /// pattern at a prior position.
    fn try_lower_literal_unpack_nested(
        &mut self,
        lhs: &[AstNode],
        values: Vec<AstNode>,
        span: AstSpan,
        prior_forbidden: &HashSet<&str>,
    ) -> Option<Vec<AstNode>> {
        let bound_names = collect_bound_names(lhs);
        let ellipsis_idx = lhs.iter().position(|n| matches!(n, AstNode::Ellipsis(_)));

        match ellipsis_idx {
            None => {
                if lhs.len() > values.len() {
                    return None;
                }
            }
            Some(ei) => {
                let suffix_len = lhs.len() - ei - 1;
                if ei > values.len() || suffix_len > values.len() {
                    return None;
                }
            }
        }

        for (i, value) in values.iter().enumerate() {
            let mut forbidden = prior_forbidden.clone();
            for (pos, name) in &bound_names {
                if *pos < i {
                    forbidden.insert(name.as_str());
                }
            }
            if expr_uses_vars(value, &forbidden) {
                return None;
            }
        }

        let mut stmts = Vec::new();
        self.lower_literal_unpack_items(lhs, &values, span, &mut stmts, &bound_names)?;
        Some(stmts)
    }

    fn make_unpack_assign(
        &mut self,
        item: &AstNode,
        tmp_name: &str,
        pos: i64,
        span: AstSpan,
        aug_op: Option<crate::ast::BinaryOperator>,
    ) -> Vec<AstNode> {
        let rhs_value = AstNode::Postfix {
            object: Box::new(AstNode::Variable(tmp_name.into(), None)),
            items: vec![AstNode::Literal(Value::Int(pos), None)],
            explicit_call: false,
            depth: None,
            span: None,
        };
        match item {
            AstNode::Variable(name, item_span) if name == "_" => vec![],
            AstNode::Variable(name, item_span) => vec![AstNode::Assignment {
                name: name.clone(),
                op: aug_op,
                value: Box::new(rhs_value),
                span: *item_span,
                name_span: *item_span,
            }],
            AstNode::Index { object, index, .. } => vec![AstNode::IndexAssign {
                object: object.clone(),
                index: index.clone(),
                op: aug_op,
                value: Box::new(rhs_value),
                span,
            }],
            AstNode::Postfix {
                object,
                items,
                explicit_call: false,
                ..
            } => {
                let index = if items.len() == 1 {
                    Box::new(items[0].clone())
                } else {
                    Box::new(AstNode::List(items.clone(), None))
                };
                vec![AstNode::IndexAssign {
                    object: object.clone(),
                    index,
                    op: aug_op,
                    value: Box::new(rhs_value),
                    span,
                }]
            }
            AstNode::Ellipsis(_) => vec![],
            AstNode::List(inner_items, _) => {
                let sub_tmp = format!("--resolver-unpack-{}", self.gensym);
                self.gensym += 1;
                let mut stmts = vec![AstNode::Assignment {
                    name: sub_tmp.clone(),
                    op: None,
                    value: Box::new(rhs_value),
                    span,
                    name_span: span,
                }];
                let ellipsis_idx = inner_items
                    .iter()
                    .position(|n| matches!(n, AstNode::Ellipsis(_)));
                for (pos, item) in inner_items.iter().enumerate() {
                    if matches!(item, AstNode::Ellipsis(_)) {
                        break;
                    }
                    stmts.extend(self.make_unpack_assign(item, &sub_tmp, pos as i64, span, None));
                }
                if let Some(ei) = ellipsis_idx {
                    for (offset, item) in inner_items.iter().skip(ei + 1).enumerate() {
                        let pos = -1 - (offset as i64);
                        stmts.extend(self.make_unpack_assign(item, &sub_tmp, pos, span, None));
                    }
                }
                stmts
            }
            _ => vec![Self::invalid_unpack_target(span)],
        }
    }

    fn should_index(&self, object: &AstNode, items: &[AstNode]) -> bool {
        if items.is_empty() {
            return false;
        }
        match object {
            AstNode::List(..) | AstNode::Dict(..) => true,
            AstNode::Variable(name, span) => {
                self.lookup_fact(name, *span, false) == BindingFact::Container
            }
            AstNode::OuterVariable(name, span) => {
                self.lookup_fact(name, *span, true) == BindingFact::Container
            }
            AstNode::CallName { .. } => false,
            _ => false,
        }
    }

    fn should_call(&self, object: &AstNode) -> bool {
        match object {
            AstNode::Variable(name, span) => {
                self.lookup_fact(name, *span, false) == BindingFact::Callable
            }
            AstNode::OuterVariable(name, span) => {
                self.lookup_fact(name, *span, true) == BindingFact::Callable
            }
            AstNode::Function { .. } => true,
            _ => false,
        }
    }

    fn invalidate_bindings_named(&mut self, names: impl IntoIterator<Item = String>) {
        let names: HashSet<_> = names.into_iter().collect();
        self.binding_facts.retain(|binding, _| match binding {
            BindingId::Runtime(name) => !names.contains(name),
            BindingId::Symbol(idx) => !names.contains(&self.symbol_def_names[*idx]),
            BindingId::Synthetic(_) => true,
        });
    }

    fn load_symbols(&mut self, symbols: &SymbolIndex) {
        self.scopes.clear();
        self.binding_facts.clear();
        self.symbol_defs_by_span.clear();
        self.symbol_def_names = symbols.defs.iter().map(|def| def.name.clone()).collect();

        let mut root_scope = HashMap::new();
        for (idx, def) in symbols.defs.iter().enumerate() {
            if let Some(name_span) = def.name_span {
                self.symbol_defs_by_span
                    .insert((def.name.clone(), Some(name_span)), idx);
            }
            if def.kind == DefKind::Builtin {
                let binding = BindingId::Symbol(idx);
                root_scope.insert(def.name.clone(), binding.clone());
                self.binding_facts.insert(binding, BindingFact::Callable);
            }
        }
        for (name, fact) in &self.runtime_seed_facts {
            let binding = BindingId::Runtime(name.clone());
            root_scope.insert(name.clone(), binding.clone());
            self.binding_facts.insert(binding, *fact);
        }
        self.scopes.push(root_scope);
    }

    fn resolve_function(
        &mut self,
        params: Option<Vec<Parameter>>,
        ref_capture: bool,
        body: AstNode,
        span: AstSpan,
        recursive_binding: Option<(String, BindingId)>,
    ) -> AstNode {
        self.scopes.push(HashMap::new());
        if let Some((name, binding)) = recursive_binding {
            self.bind_current_scope(name, binding);
        }
        if let Some(params_ref) = params.as_ref() {
            for p in params_ref {
                let pname = p.name();
                let pspan = p.span();
                let binding = self.binding_for_named_def(pname, pspan);
                self.bind_current_scope(pname.to_string(), binding.clone());
                self.set_binding_fact(&binding, BindingFact::Unknown);
            }
        } else {
            for name in ["x", "y", "z"] {
                let binding = self.next_synthetic_binding();
                self.bind_current_scope(name.to_string(), binding.clone());
                self.set_binding_fact(&binding, BindingFact::Unknown);
            }
        }
        // Resolve default expressions in named params
        let params = params.map(|ps| {
            ps.into_iter()
                .map(|p| match p {
                    Parameter::Named {
                        name,
                        span,
                        default: Some(default_expr),
                    } => Parameter::Named {
                        name,
                        span,
                        default: Some(Box::new(self.resolve_node(*default_expr))),
                    },
                    other => other,
                })
                .collect()
        });
        let body = Box::new(self.resolve_node(body));
        self.scopes.pop();
        AstNode::Function {
            params,
            ref_capture,
            body,
            span,
        }
    }

    fn resolve_from_snapshot(
        &mut self,
        snapshot: &ResolverSnapshot,
        node: AstNode,
    ) -> (AstNode, ResolverSnapshot) {
        self.restore(snapshot.clone());
        let node = self.resolve_node(node);
        let out = self.snapshot();
        (node, out)
    }

    fn snapshot(&self) -> ResolverSnapshot {
        ResolverSnapshot {
            scopes: self.scopes.clone(),
            binding_facts: self.binding_facts.clone(),
        }
    }

    fn restore(&mut self, snapshot: ResolverSnapshot) {
        self.scopes = snapshot.scopes;
        self.binding_facts = snapshot.binding_facts;
    }

    fn merge_snapshots(
        &mut self,
        left: &ResolverSnapshot,
        right: &ResolverSnapshot,
    ) -> ResolverSnapshot {
        debug_assert_eq!(left.scopes.len(), right.scopes.len());

        let mut scopes = Vec::with_capacity(left.scopes.len());
        let mut binding_facts = HashMap::new();
        for (left_scope, right_scope) in left.scopes.iter().zip(&right.scopes) {
            let mut merged_scope = HashMap::new();
            let mut names: HashSet<String> = left_scope.keys().cloned().collect();
            names.extend(right_scope.keys().cloned());
            for name in names {
                match (left_scope.get(&name), right_scope.get(&name)) {
                    (Some(left_binding), Some(right_binding)) if left_binding == right_binding => {
                        let fact = Self::join_facts(
                            Self::fact_in_snapshot(left, left_binding),
                            Self::fact_in_snapshot(right, right_binding),
                        );
                        merged_scope.insert(name, left_binding.clone());
                        binding_facts.insert(left_binding.clone(), fact);
                    }
                    (Some(left_binding), Some(right_binding)) => {
                        let merged_binding = self.next_synthetic_binding();
                        let fact = Self::join_facts(
                            Self::fact_in_snapshot(left, left_binding),
                            Self::fact_in_snapshot(right, right_binding),
                        );
                        merged_scope.insert(name, merged_binding.clone());
                        binding_facts.insert(merged_binding, fact);
                    }
                    (Some(left_binding), None) => {
                        let merged_binding = self.next_synthetic_binding();
                        let fact = Self::join_facts(
                            Self::fact_in_snapshot(left, left_binding),
                            BindingFact::Unknown,
                        );
                        merged_scope.insert(name, merged_binding.clone());
                        binding_facts.insert(merged_binding, fact);
                    }
                    (None, Some(right_binding)) => {
                        let merged_binding = self.next_synthetic_binding();
                        let fact = Self::join_facts(
                            BindingFact::Unknown,
                            Self::fact_in_snapshot(right, right_binding),
                        );
                        merged_scope.insert(name, merged_binding.clone());
                        binding_facts.insert(merged_binding, fact);
                    }
                    (None, None) => unreachable!(),
                }
            }
            scopes.push(merged_scope);
        }
        ResolverSnapshot {
            scopes,
            binding_facts,
        }
    }

    fn join_facts(left: BindingFact, right: BindingFact) -> BindingFact {
        if left == right {
            left
        } else {
            BindingFact::Unknown
        }
    }

    fn fact_in_snapshot(snapshot: &ResolverSnapshot, binding: &BindingId) -> BindingFact {
        snapshot
            .binding_facts
            .get(binding)
            .copied()
            .unwrap_or(BindingFact::Unknown)
    }

    fn bind_current_scope(&mut self, name: String, binding: BindingId) {
        self.scopes
            .last_mut()
            .expect("resolver must always have a root scope")
            .insert(name, binding);
    }

    fn binding_for_named_def(&mut self, name: &str, span: AstSpan) -> BindingId {
        self.symbol_defs_by_span
            .get(&(name.to_string(), span))
            .copied()
            .map(BindingId::Symbol)
            .unwrap_or_else(|| self.next_synthetic_binding())
    }

    fn next_synthetic_binding(&mut self) -> BindingId {
        let id = self.binding_gensym;
        self.binding_gensym += 1;
        BindingId::Synthetic(id)
    }

    fn lookup_binding(&self, name: &str) -> Option<BindingId> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
    }

    fn lookup_outer_binding(&self, name: &str) -> Option<BindingId> {
        if self.scopes.len() <= 1 {
            return None;
        }
        self.scopes[..self.scopes.len() - 1]
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
    }

    fn lookup_fact(&self, name: &str, _span: AstSpan, outer: bool) -> BindingFact {
        let binding = if outer {
            self.lookup_outer_binding(name)
        } else {
            self.lookup_binding(name)
        };
        binding
            .as_ref()
            .and_then(|binding| self.binding_facts.get(binding).copied())
            .unwrap_or(BindingFact::Unknown)
    }

    fn set_binding_fact(&mut self, binding: &BindingId, fact: BindingFact) {
        self.binding_facts.insert(binding.clone(), fact);
    }

    fn fact_from_ast(&self, node: &AstNode) -> BindingFact {
        match node {
            AstNode::Function { .. } => BindingFact::Callable,
            AstNode::Cat(..) | AstNode::List(..) | AstNode::Dict(..) | AstNode::Range { .. } => {
                BindingFact::Container
            }
            AstNode::Literal(value, _) => Self::fact_from_value(value),
            AstNode::Group { expr, .. } => self.fact_from_ast(expr),
            AstNode::Variable(name, span) => self.lookup_fact(name, *span, false),
            AstNode::OuterVariable(name, span) => self.lookup_fact(name, *span, true),
            AstNode::CallName { .. } => BindingFact::Unknown,
            _ => BindingFact::Unknown,
        }
    }

    fn fact_from_value(value: &Value) -> BindingFact {
        match value {
            _ if value.is_callable() => BindingFact::Callable,
            _ if value.is_container() => BindingFact::Container,
            _ => BindingFact::Unknown,
        }
    }

    fn apply_pipe_rhs(
        right: AstNode,
        input: AstNode,
        insert_at_start: bool,
        span: AstSpan,
    ) -> AstNode {
        let insert = |items: &mut Vec<AstNode>, input| {
            if insert_at_start {
                items.insert(0, input);
            } else {
                items.push(input);
            }
        };

        match right {
            AstNode::Variable(name, var_span) => {
                let new_span = match (span, var_span) {
                    (None, None) => None,
                    _ => {
                        let name_start = var_span
                            .map(|s| s.0)
                            .unwrap_or_else(|| span.map(|s| s.0).unwrap_or(0));
                        let name_end = span.map(|s| s.1).unwrap_or(name_start);
                        Some((name_start, name_end))
                    }
                };
                AstNode::Postfix {
                    object: Box::new(AstNode::Variable(name, var_span)),
                    items: vec![input],
                    explicit_call: false,
                    depth: None,
                    span: new_span,
                }
            }
            AstNode::Function {
                params,
                ref_capture,
                body,
                span: function_span,
            } => AstNode::CallAnonymous {
                object: Box::new(AstNode::Function {
                    params,
                    ref_capture,
                    body,
                    span: function_span,
                }),
                args: vec![input],
                span,
            },
            AstNode::CallName {
                name,
                mut args,
                span: effect_span,
                name_span,
            } => {
                insert(&mut args, input);
                let new_span = match (span, effect_span) {
                    (None, None) => None,
                    _ => {
                        let name_start = effect_span
                            .map(|s| s.0)
                            .unwrap_or_else(|| span.map(|s| s.0).unwrap_or(0));
                        let name_end = span.map(|s| s.1).unwrap_or(name_start);
                        Some((name_start, name_end))
                    }
                };
                AstNode::CallName {
                    name,
                    args,
                    span: new_span,
                    name_span,
                }
            }
            AstNode::CallAnonymous {
                object,
                mut args,
                span: effect_span,
                ..
            } => {
                insert(&mut args, input);
                let new_span = match (span, effect_span) {
                    (None, None) => None,
                    _ => {
                        let name_start = effect_span
                            .map(|s| s.0)
                            .unwrap_or_else(|| span.map(|s| s.0).unwrap_or(0));
                        let name_end = span.map(|s| s.1).unwrap_or(name_start);
                        Some((name_start, name_end))
                    }
                };
                AstNode::CallAnonymous {
                    object,
                    args,
                    span: new_span,
                }
            }
            AstNode::Postfix {
                object,
                mut items,
                explicit_call: _,
                depth,
                span: effect_span,
                ..
            } => {
                insert(&mut items, input);
                let new_span = match (span, effect_span) {
                    (None, None) => None,
                    _ => {
                        let name_start = if let AstNode::Variable(_, var_span) = object.as_ref() {
                            var_span
                                .map(|s| s.0)
                                .unwrap_or_else(|| effect_span.map(|s| s.0).unwrap_or(0))
                        } else {
                            effect_span.map(|s| s.0).unwrap_or(0)
                        };
                        let name_end = span.map(|s| s.1).unwrap_or(name_start);
                        Some((name_start, name_end))
                    }
                };
                AstNode::Postfix {
                    object,
                    items,
                    explicit_call: false,
                    depth,
                    span: new_span,
                }
            }
            AstNode::Assignment {
                name,
                op,
                value,
                span: assign_span,
                name_span,
            } if matches!(value.as_ref(), AstNode::PipeInput) => AstNode::Assignment {
                name,
                op,
                value: Box::new(input),
                span: assign_span,
                name_span,
            },
            AstNode::OuterAssignment {
                name,
                op,
                value,
                span: assign_span,
                name_span,
            } if matches!(value.as_ref(), AstNode::PipeInput) => AstNode::OuterAssignment {
                name,
                op,
                value: Box::new(input),
                span: assign_span,
                name_span,
            },
            AstNode::IndexAssign {
                object,
                index,
                op,
                value,
                span: assign_span,
            } if matches!(value.as_ref(), AstNode::PipeInput) => AstNode::IndexAssign {
                object,
                index,
                op,
                value: Box::new(input),
                span: assign_span,
            },
            AstNode::MutatingIndexAssign {
                object,
                index,
                value,
                span: assign_span,
            } if matches!(value.as_ref(), AstNode::PipeInput) => AstNode::MutatingIndexAssign {
                object,
                index,
                value: Box::new(input),
                span: assign_span,
            },
            AstNode::Pause {
                expr: None,
                span: pause_span,
            } => Self::pipe_effect_rhs(
                AstNode::Pause {
                    expr: Some(Box::new(AstNode::PipeInput)),
                    span: pause_span,
                },
                input,
                span,
            ),
            AstNode::Pause { .. }
            | AstNode::Break(_)
            | AstNode::Continue(_)
            | AstNode::Return(..)
            | AstNode::Debug { .. }
            | AstNode::Try(..) => Self::pipe_effect_rhs(right, input, span),
            _ => AstNode::Postfix {
                object: Box::new(right),
                items: vec![input],
                explicit_call: false,
                depth: None,
                span,
            },
        }
    }

    fn pipe_effect_rhs(effect: AstNode, input: AstNode, span: AstSpan) -> AstNode {
        match effect {
            AstNode::Return(None, return_span) => {
                AstNode::Return(Some(Box::new(input)), return_span)
            }
            effect => AstNode::PipeTap {
                input: Box::new(input),
                effect: Box::new(effect),
                span,
            },
        }
    }
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Collect bound variable names from an unpack-assignment lhs.
/// Each entry is `(position, name)`. For nested lists the outer
/// position is used so that dependency checks work correctly.
fn collect_bound_names(items: &[AstNode]) -> Vec<(usize, String)> {
    let mut names = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match item {
            AstNode::Variable(name, _) if name != "_" => {
                names.push((i, name.clone()));
            }
            AstNode::List(inner, _) => {
                for (_, inner_name) in collect_bound_names(inner) {
                    names.push((i, inner_name));
                }
            }
            _ => {}
        }
    }
    names
}

/// Return `true` if `node` contains a reference to any variable whose name
/// is in `vars`.
fn expr_uses_vars(node: &AstNode, vars: &HashSet<&str>) -> bool {
    match node {
        AstNode::Variable(name, _) | AstNode::OuterVariable(name, _) => {
            vars.contains(name.as_str())
        }
        AstNode::BinaryOp { left, right, .. } => {
            expr_uses_vars(left, vars) || expr_uses_vars(right, vars)
        }
        AstNode::LazyBool { operands, .. } => {
            operands.iter().any(|operand| expr_uses_vars(operand, vars))
        }
        AstNode::ComparisonChain { first, rest, .. } => {
            expr_uses_vars(first, vars) || rest.iter().any(|(_, n)| expr_uses_vars(n, vars))
        }
        AstNode::UnaryOp { operand, .. } => expr_uses_vars(operand, vars),
        AstNode::Group { expr, .. } => expr_uses_vars(expr, vars),
        AstNode::Range {
            start, end, step, ..
        } => {
            expr_uses_vars(start, vars)
                || expr_uses_vars(end, vars)
                || step.as_ref().is_some_and(|s| expr_uses_vars(s, vars))
        }
        AstNode::Assignment { value, .. } | AstNode::OuterAssignment { value, .. } => {
            expr_uses_vars(value, vars)
        }
        AstNode::Cat(items, _) | AstNode::List(items, _) => {
            items.iter().any(|item| expr_uses_vars(item, vars))
        }
        AstNode::Dict(pairs, _) => pairs.iter().any(|(_, v)| expr_uses_vars(v, vars)),
        AstNode::Postfix { object, items, .. } => {
            expr_uses_vars(object, vars) || items.iter().any(|item| expr_uses_vars(item, vars))
        }
        AstNode::Pipe { input, effect, .. } => {
            expr_uses_vars(input, vars) || expr_uses_vars(effect, vars)
        }
        AstNode::PipeTap { input, effect, .. } => {
            expr_uses_vars(input, vars) || expr_uses_vars(effect, vars)
        }
        AstNode::CallName { args, .. } => args.iter().any(|arg| expr_uses_vars(arg, vars)),
        AstNode::CallAnonymous { object, args, .. } => {
            expr_uses_vars(object, vars) || args.iter().any(|arg| expr_uses_vars(arg, vars))
        }
        AstNode::Index { object, index, .. } => {
            expr_uses_vars(object, vars) || expr_uses_vars(index, vars)
        }
        AstNode::IndexAssign {
            object,
            index,
            value,
            ..
        } => {
            expr_uses_vars(object, vars)
                || expr_uses_vars(index, vars)
                || expr_uses_vars(value, vars)
        }
        AstNode::Function { body, .. } => expr_uses_vars(body, vars),
        AstNode::Conditional {
            condition,
            true_branch,
            false_branch,
            ..
        } => {
            expr_uses_vars(condition, vars)
                || expr_uses_vars(true_branch, vars)
                || false_branch
                    .as_ref()
                    .is_some_and(|b| expr_uses_vars(b, vars))
        }
        AstNode::ConditionalDot {
            condition,
            true_branch,
            ..
        } => expr_uses_vars(condition, vars) || expr_uses_vars(true_branch, vars),
        AstNode::ConditionalChain {
            pairs,
            default_branch,
            ..
        } => {
            pairs
                .iter()
                .any(|(cond, branch)| expr_uses_vars(cond, vars) || expr_uses_vars(branch, vars))
                || expr_uses_vars(default_branch, vars)
        }
        AstNode::WLoop {
            condition, body, ..
        } => expr_uses_vars(condition, vars) || expr_uses_vars(body, vars),
        AstNode::NLoop { count, body, .. } => {
            expr_uses_vars(count, vars) || expr_uses_vars(body, vars)
        }
        AstNode::Return(expr, _) => expr.as_ref().is_some_and(|e| expr_uses_vars(e, vars)),
        AstNode::Debug { expr, .. } => expr_uses_vars(expr, vars),
        AstNode::Pause { expr, .. } => expr.as_ref().is_some_and(|expr| expr_uses_vars(expr, vars)),
        AstNode::Try(expr, _) => expr_uses_vars(expr, vars),
        AstNode::Block(items, _) | AstNode::BlockExpr(items, ..) => {
            items.iter().any(|item| expr_uses_vars(item, vars))
        }
        AstNode::FString { parts, .. } => parts.iter().any(|p| match p {
            crate::ast::FStringPart::Text(_) => false,
            crate::ast::FStringPart::Expr {
                expr, spec_exprs, ..
            } => expr_uses_vars(expr, vars) || spec_exprs.iter().any(|e| expr_uses_vars(e, vars)),
        }),
        AstNode::UnpackAssignment { lhs, rhs, .. } => {
            lhs.iter().any(|item| expr_uses_vars(item, vars)) || expr_uses_vars(rhs, vars)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::lex::Lexer;
    use crate::parse::Parser;
    use crate::vm::GlobalMap;

    fn resolve_src(src: &str) -> AstNode {
        let tokens = Lexer::new(src).tokenize().expect("tokenize");
        let mut parser = Parser::new_with_builtins(tokens, src.to_string(), Builtins::new());
        let ast = parser.parse().expect("parse");
        Resolver::with_builtins(Builtins::new()).resolve(ast)
    }

    fn resolve_src_with_env(src: &str, env: GlobalMap) -> AstNode {
        let tokens = Lexer::new(src).tokenize().expect("tokenize");
        let mut parser = Parser::new_with_builtins(tokens, src.to_string(), Builtins::new());
        let ast = parser.parse().expect("parse");
        Resolver::from_env(env, Builtins::new()).resolve(ast)
    }

    fn stmt(ast: &AstNode, idx: usize) -> &AstNode {
        match ast {
            AstNode::Block(stmts, _) => &stmts[idx],
            other => panic!("expected block, got {other:?}"),
        }
    }

    fn contains_call_name(node: &AstNode, target: &str) -> bool {
        match node {
            AstNode::CallName { name, args, .. } => {
                name == target || args.iter().any(|arg| contains_call_name(arg, target))
            }
            AstNode::CallAnonymous { object, args, .. } => {
                contains_call_name(object, target)
                    || args.iter().any(|arg| contains_call_name(arg, target))
            }
            AstNode::Assignment { value, .. }
            | AstNode::OuterAssignment { value, .. }
            | AstNode::Debug { expr: value, .. } => contains_call_name(value, target),
            AstNode::Pause { expr, .. } => expr
                .as_ref()
                .is_some_and(|expr| contains_call_name(expr, target)),
            AstNode::Postfix { object, items, .. } => {
                contains_call_name(object, target)
                    || items.iter().any(|item| contains_call_name(item, target))
            }
            AstNode::Index { object, index, .. } | AstNode::MutatingIndex { object, index, .. } => {
                contains_call_name(object, target) || contains_call_name(index, target)
            }
            AstNode::IndexAssign {
                object,
                index,
                value,
                ..
            }
            | AstNode::MutatingIndexAssign {
                object,
                index,
                value,
                ..
            } => {
                contains_call_name(object, target)
                    || contains_call_name(index, target)
                    || contains_call_name(value, target)
            }
            AstNode::BinaryOp { left, right, .. } => {
                contains_call_name(left, target) || contains_call_name(right, target)
            }
            AstNode::LazyBool { operands, .. } => operands
                .iter()
                .any(|operand| contains_call_name(operand, target)),
            AstNode::ComparisonChain { first, rest, .. } => {
                contains_call_name(first, target)
                    || rest
                        .iter()
                        .any(|(_, node)| contains_call_name(node, target))
            }
            AstNode::UnaryOp { operand, .. }
            | AstNode::Group { expr: operand, .. }
            | AstNode::Try(operand, _) => contains_call_name(operand, target),
            AstNode::Range {
                start, end, step, ..
            } => {
                contains_call_name(start, target)
                    || contains_call_name(end, target)
                    || step
                        .as_ref()
                        .is_some_and(|step| contains_call_name(step, target))
            }
            AstNode::Cat(items, _) | AstNode::List(items, _) | AstNode::Block(items, _) => {
                items.iter().any(|item| contains_call_name(item, target))
            }
            AstNode::BlockExpr(items, _) => {
                items.iter().any(|item| contains_call_name(item, target))
            }
            AstNode::Dict(pairs, _) => pairs
                .iter()
                .any(|(_, value)| contains_call_name(value, target)),
            AstNode::Function { body, .. }
            | AstNode::WLoop { body, .. }
            | AstNode::NLoop { body, .. } => contains_call_name(body, target),
            AstNode::Conditional {
                condition,
                true_branch,
                false_branch,
                ..
            } => {
                contains_call_name(condition, target)
                    || contains_call_name(true_branch, target)
                    || false_branch
                        .as_ref()
                        .is_some_and(|branch| contains_call_name(branch, target))
            }
            AstNode::ConditionalDot {
                condition,
                true_branch,
                ..
            } => contains_call_name(condition, target) || contains_call_name(true_branch, target),
            AstNode::ConditionalChain {
                pairs,
                default_branch,
                ..
            } => {
                pairs.iter().any(|(condition, branch)| {
                    contains_call_name(condition, target) || contains_call_name(branch, target)
                }) || contains_call_name(default_branch, target)
            }
            AstNode::Pipe { input, effect, .. } | AstNode::PipeTap { input, effect, .. } => {
                contains_call_name(input, target) || contains_call_name(effect, target)
            }
            AstNode::Return(expr, _) => expr
                .as_ref()
                .is_some_and(|expr| contains_call_name(expr, target)),
            AstNode::FString { parts, .. } => parts.iter().any(|part| match part {
                crate::ast::FStringPart::Text(_) => false,
                crate::ast::FStringPart::Expr {
                    expr, spec_exprs, ..
                } => {
                    contains_call_name(expr, target)
                        || spec_exprs
                            .iter()
                            .any(|expr| contains_call_name(expr, target))
                }
            }),
            AstNode::UnpackAssignment { lhs, rhs, .. } => {
                lhs.iter().any(|item| contains_call_name(item, target))
                    || contains_call_name(rhs, target)
            }
            AstNode::NamedArg { value, .. } => contains_call_name(value, target),
            AstNode::Literal(..)
            | AstNode::Import { .. }
            | AstNode::Variable(..)
            | AstNode::OuterVariable(..)
            | AstNode::PipeInput
            | AstNode::Break(_)
            | AstNode::Continue(_)
            | AstNode::Ellipsis(_)
            | AstNode::Error(..) => false,
        }
    }

    #[test]
    fn bang_index_error_explains_the_variable_requirement() {
        for src in ["(1;2)[!]", "(1;2)[!]:3"] {
            let ast = resolve_src(src);
            let AstNode::Error(err, _) = ast else {
                panic!("expected resolver error for {src}, got {ast:?}");
            };
            assert_eq!(
                err.msg.as_deref(),
                Some("bang indexing can mutate only a variable")
            );
            assert_eq!(
                err.notes.as_slice(),
                ["assign the container to a variable before using '[!...]'"]
            );
        }
    }

    #[test]
    fn indexable_alias_from_runtime_env_lowers_postfix_to_index() {
        let mut env = GlobalMap::default();
        env.insert(
            "ys".into(),
            Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)])),
        );
        let ast = resolve_src_with_env("xs:ys; xs[0]", env);

        match stmt(&ast, 1) {
            AstNode::Index { object, index, .. } => {
                assert!(matches!(object.as_ref(), AstNode::Variable(name, _) if name == "xs"));
                assert!(matches!(index.as_ref(), AstNode::Literal(Value::Int(0), _)));
            }
            other => panic!("expected index lowering, got {other:?}"),
        }
    }

    #[test]
    fn range_assignment_lowers_postfix_to_index() {
        let ast = resolve_src("xs:1..10; xs 0");

        match stmt(&ast, 1) {
            AstNode::Index { object, index, .. } => {
                assert!(matches!(object.as_ref(), AstNode::Variable(name, _) if name == "xs"));
                assert!(matches!(index.as_ref(), AstNode::Literal(Value::Int(0), _)));
            }
            other => panic!("expected index lowering, got {other:?}"),
        }
    }

    #[test]
    fn string_assignment_lowers_postfix_to_index() {
        let ast = resolve_src("s:\"abc\"; s 0");

        match stmt(&ast, 1) {
            AstNode::Index { object, index, .. } => {
                assert!(matches!(object.as_ref(), AstNode::Variable(name, _) if name == "s"));
                assert!(matches!(index.as_ref(), AstNode::Literal(Value::Int(0), _)));
            }
            other => panic!("expected index lowering, got {other:?}"),
        }
    }

    #[test]
    fn packed_list_literal_assignment_lowers_postfix_to_index() {
        let ast = AstNode::Block(
            vec![
                AstNode::Assignment {
                    name: "xs".to_string(),
                    op: None,
                    value: Box::new(AstNode::Literal(
                        Value::IntList(Arc::new(vec![1, 2, 3])),
                        None,
                    )),
                    span: None,
                    name_span: None,
                },
                AstNode::Postfix {
                    object: Box::new(AstNode::Variable("xs".to_string(), None)),
                    items: vec![AstNode::Literal(Value::Int(0), None)],
                    explicit_call: false,
                    depth: None,
                    span: None,
                },
            ],
            None,
        );
        let mut resolver = Resolver::new();
        let ast = resolver.resolve(ast);

        match stmt(&ast, 1) {
            AstNode::Index { object, index, .. } => {
                assert!(matches!(object.as_ref(), AstNode::Variable(name, _) if name == "xs"));
                assert!(matches!(index.as_ref(), AstNode::Literal(Value::Int(0), _)));
            }
            other => panic!("expected index lowering, got {other:?}"),
        }
    }

    #[test]
    fn lifted_callable_literal_assignment_lowers_postfix_to_call() {
        let ast = AstNode::Block(
            vec![
                AstNode::Assignment {
                    name: "f".to_string(),
                    op: None,
                    value: Box::new(AstNode::Literal(
                        Value::function_composition(
                            BinaryOperator::Add,
                            Value::Int(1),
                            Value::Int(2),
                        ),
                        None,
                    )),
                    span: None,
                    name_span: None,
                },
                AstNode::Postfix {
                    object: Box::new(AstNode::Variable("f".to_string(), None)),
                    items: vec![AstNode::Literal(Value::Int(0), None)],
                    explicit_call: false,
                    depth: None,
                    span: None,
                },
            ],
            None,
        );
        let mut resolver = Resolver::new();
        let ast = resolver.resolve(ast);

        match stmt(&ast, 1) {
            AstNode::CallName { name, args, .. } => {
                assert_eq!(name, "f");
                assert_eq!(args.len(), 1);
                assert!(matches!(args[0], AstNode::Literal(Value::Int(0), _)));
            }
            other => panic!("expected call lowering, got {other:?}"),
        }
    }

    #[test]
    fn runtime_lifted_callable_fact_lowers_postfix_to_call() {
        let mut env = GlobalMap::new();
        env.insert(
            "f".to_string(),
            Value::function_composition(BinaryOperator::Add, Value::Int(1), Value::Int(2)),
        );

        match resolve_src_with_env("f[0]", env) {
            AstNode::CallName { name, args, .. } => {
                assert_eq!(name, "f");
                assert_eq!(args.len(), 1);
                assert!(matches!(args[0], AstNode::Literal(Value::Int(0), _)));
            }
            other => panic!("expected call lowering, got {other:?}"),
        }
    }

    #[test]
    fn index_lowering_preserves_postfix_span() {
        let src = "xs:(1;2;3); xs[0]";
        let ast = resolve_src(src);
        let expected_start = src.find("xs[0]").expect("index source");
        let expected = Some((expected_start, expected_start + "xs[0]".len()));

        match stmt(&ast, 1) {
            AstNode::Index { span, .. } => assert_eq!(*span, expected),
            other => panic!("expected index lowering, got {other:?}"),
        }
    }

    #[test]
    fn function_local_rebinding_does_not_leak_to_outer_postfix() {
        let ast = resolve_src("f:{x:(1;2)}; x[0]");

        assert!(matches!(stmt(&ast, 1), AstNode::Postfix { .. }));
    }

    #[test]
    fn branch_fact_conflict_keeps_postfix_dynamic() {
        let ast = resolve_src("$[true;f:{x};f:(1;2)]; f[0]");

        assert!(matches!(stmt(&ast, 1), AstNode::Postfix { .. }));
    }

    #[test]
    fn named_function_recursion_lowers_to_call_name() {
        let ast = resolve_src("fact:{[n]$[n=0;1;n*fact[n-1]]}");

        assert!(contains_call_name(&ast, "fact"));
    }
}
