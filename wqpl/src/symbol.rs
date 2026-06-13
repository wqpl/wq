use std::collections::HashMap;

use crate::astnode::AstNode;
use crate::value::Value;
use crate::wqerror::WqError;

const PARSER_INTERNAL_PREFIX: &str = "--";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefKind {
    Assignment,
    Function,
    Parameter,
    ImplicitParam,
    LoopCounter,
    Builtin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseKind {
    Read,
    Write,
    OuterRead,
    OuterWrite,
    RefCaptureRead,
    RefCaptureWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolProvenanceKind {
    Builtin,
    Global,
    Local,
    Parameter,
    ImplicitParameter,
    LoopCounter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolProvenance {
    pub kind: SymbolProvenanceKind,
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SymbolOccurrence {
    pub span: (usize, usize),
    pub def_idx: usize,
    pub kind: UseKind,
}

impl UseKind {
    pub fn is_read(self) -> bool {
        matches!(self, Self::Read | Self::OuterRead | Self::RefCaptureRead)
    }

    pub fn is_write(self) -> bool {
        matches!(self, Self::Write | Self::OuterWrite | Self::RefCaptureWrite)
    }

    pub fn is_ref_capture(self) -> bool {
        matches!(
            self,
            Self::OuterRead | Self::OuterWrite | Self::RefCaptureRead | Self::RefCaptureWrite
        )
    }
}

#[derive(Debug, Clone)]
pub struct SymbolDef {
    pub name: String,
    pub span: Option<(usize, usize)>,
    pub name_span: Option<(usize, usize)>,
    pub kind: DefKind,
    pub params: Option<Vec<String>>,
    pub parent: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct SymbolUse {
    pub name: String,
    pub span: Option<(usize, usize)>,
    pub kind: UseKind,
    pub def_idx: Option<usize>,
}

#[derive(Debug, Default, Clone)]
pub struct SymbolIndex {
    pub defs: Vec<SymbolDef>,
    pub uses: Vec<SymbolUse>,
    pub literals: Vec<((usize, usize), Value)>,
    pub errors: Vec<((usize, usize), WqError)>,
}

impl SymbolIndex {
    pub(crate) fn analyze(ast: &AstNode, builtins: &crate::builtins::Builtins) -> Self {
        let mut analyzer = SymbolAnalyzer::new(builtins);
        analyzer.analyze(ast);
        Self {
            defs: analyzer.defs,
            uses: analyzer.uses,
            literals: analyzer.literals,
            errors: analyzer.errors,
        }
    }

    /// Query the symbol at the given byte offset.
    /// Returns the definition span and all locations (defs + uses) that refer
    /// to the same binding.
    /// Query a specific definition by its index.
    pub fn query_def(&self, def_idx: usize) -> Option<SymbolQueryResult> {
        let def = self.defs.get(def_idx)?;
        Some(self.gather_result(def_idx, &def.name))
    }

    pub fn query_at(&self, offset: usize) -> Option<SymbolQueryResult> {
        // Find a def whose *name* span contains the offset.
        // Using name_span instead of span avoids matching an enclosing function
        // definition when the cursor is inside the function body.
        if let Some((def_idx, def)) = self
            .defs
            .iter()
            .enumerate()
            .find(|(_, d)| d.name_span.is_some_and(|(s, e)| s <= offset && offset < e))
        {
            return Some(self.gather_result(def_idx, &def.name));
        }

        // Find a use that contains the offset
        if let Some(use_) = self
            .uses
            .iter()
            .find(|u| u.span.is_some_and(|(s, e)| s <= offset && offset < e))
        {
            let def_idx = use_.def_idx?;
            return Some(self.gather_result(def_idx, &use_.name));
        }

        None
    }

    /// Query the literal value at the given byte offset.
    pub fn query_literal_at(&self, offset: usize) -> Option<Value> {
        self.literals
            .iter()
            .find(|((s, e), _)| s <= &offset && offset < *e)
            .map(|(_, v)| v.clone())
    }

    pub fn def_has_ref_capture(&self, def_idx: usize) -> bool {
        self.ref_capture_count(def_idx) > 0
    }

    pub fn ref_capture_count(&self, def_idx: usize) -> usize {
        self.uses
            .iter()
            .filter(|u| u.def_idx == Some(def_idx) && u.kind.is_ref_capture())
            .count()
    }

    pub fn ref_capture_spans(&self) -> Vec<(usize, usize)> {
        self.uses
            .iter()
            .filter(|u| u.kind.is_ref_capture())
            .filter_map(|u| u.span)
            .collect()
    }

    pub fn semantic_highlight_spans(&self) -> Vec<crate::highlight::SemanticHighlightSpan> {
        self.occurrences()
            .into_iter()
            .filter_map(|occurrence| {
                let def = self.defs.get(occurrence.def_idx)?;
                let name = if occurrence.kind.is_ref_capture() {
                    crate::highlight::HighlightName::VariableRefCapture
                } else if matches!(def.kind, DefKind::Parameter | DefKind::ImplicitParam) {
                    crate::highlight::HighlightName::VariableParameter
                } else {
                    return None;
                };
                Some(crate::highlight::SemanticHighlightSpan {
                    span: occurrence.span,
                    name,
                })
            })
            .collect()
    }

    pub fn occurrences(&self) -> Vec<SymbolOccurrence> {
        let mut occurrences = Vec::new();
        for (def_idx, def) in self.defs.iter().enumerate() {
            let def_kind = if def.kind == DefKind::Assignment || def.kind == DefKind::Function {
                UseKind::Write
            } else {
                UseKind::Read
            };
            if let Some(span) = def.name_span
                && !self.uses.iter().any(|u| {
                    u.def_idx == Some(def_idx) && u.span == Some(span) && u.kind == def_kind
                })
            {
                occurrences.push(SymbolOccurrence {
                    span,
                    def_idx,
                    kind: def_kind,
                });
            }
        }

        for use_ in &self.uses {
            if let (Some(def_idx), Some(span)) = (use_.def_idx, use_.span) {
                occurrences.push(SymbolOccurrence {
                    span,
                    def_idx,
                    kind: use_.kind,
                });
            }
        }
        occurrences
    }

    pub fn def_provenance(&self, def_idx: usize) -> Option<SymbolProvenance> {
        let def = self.defs.get(def_idx)?;
        let origin = def
            .parent
            .and_then(|parent| self.defs.get(parent))
            .map(|parent| parent.name.clone());
        let kind = match def.kind {
            DefKind::Builtin => SymbolProvenanceKind::Builtin,
            DefKind::Assignment | DefKind::Function => {
                if def.parent.is_some() {
                    SymbolProvenanceKind::Local
                } else {
                    SymbolProvenanceKind::Global
                }
            }
            DefKind::Parameter => SymbolProvenanceKind::Parameter,
            DefKind::ImplicitParam => SymbolProvenanceKind::ImplicitParameter,
            DefKind::LoopCounter => SymbolProvenanceKind::LoopCounter,
        };
        Some(SymbolProvenance { kind, origin })
    }

    fn gather_result(&self, def_idx: usize, name: &str) -> SymbolQueryResult {
        let def = &self.defs[def_idx];
        let mut uses = Vec::new();
        let def_kind = if def.kind == DefKind::Assignment || def.kind == DefKind::Function {
            UseKind::Write
        } else {
            UseKind::Read
        };
        // Add the def location if it has a real name span and there isn't already
        // a use with the same span and kind.
        if let Some(span) = def.name_span
            && !self
                .uses
                .iter()
                .any(|u| u.def_idx == Some(def_idx) && u.span == Some(span) && u.kind == def_kind)
        {
            uses.push(SymbolLocation {
                span,
                kind: def_kind,
            });
        }
        for u in &self.uses {
            if u.def_idx == Some(def_idx)
                && let Some(span) = u.span
            {
                uses.push(SymbolLocation { span, kind: u.kind });
            }
        }
        SymbolQueryResult {
            def_idx,
            name: name.to_string(),
            def_span: def.name_span,
            uses,
            params: def.params.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SymbolLocation {
    pub span: (usize, usize),
    pub kind: UseKind,
}

#[derive(Debug, Clone)]
pub struct SymbolQueryResult {
    pub def_idx: usize,
    pub name: String,
    pub def_span: Option<(usize, usize)>,
    pub uses: Vec<SymbolLocation>,
    pub params: Option<Vec<String>>,
}

struct SymbolAnalyzer {
    defs: Vec<SymbolDef>,
    uses: Vec<SymbolUse>,
    scopes: Vec<HashMap<String, usize>>,
    literals: Vec<((usize, usize), Value)>,
    errors: Vec<((usize, usize), WqError)>,
    func_stack: Vec<usize>,
    ref_capture_stack: Vec<bool>,
}

impl SymbolAnalyzer {
    fn new(builtins: &crate::builtins::Builtins) -> Self {
        let mut defs = Vec::new();
        let mut scopes = vec![HashMap::new()];
        let global_scope = scopes.last_mut().unwrap();

        for name in builtins.list_functions_all() {
            let idx = defs.len();
            defs.push(SymbolDef {
                name: name.clone(),
                span: None,
                name_span: None,
                kind: DefKind::Builtin,
                params: None,
                parent: None,
            });
            global_scope.insert(name, idx);
        }

        Self {
            defs,
            uses: Vec::new(),
            scopes,
            literals: Vec::new(),
            errors: Vec::new(),
            func_stack: Vec::new(),
            ref_capture_stack: Vec::new(),
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn current_scope(&self) -> &HashMap<String, usize> {
        self.scopes.last().unwrap()
    }

    fn current_scope_mut(&mut self) -> &mut HashMap<String, usize> {
        self.scopes.last_mut().unwrap()
    }

    fn resolve(&self, name: &str) -> Option<usize> {
        for scope in self.scopes.iter().rev() {
            if let Some(&idx) = scope.get(name) {
                return Some(idx);
            }
        }
        None
    }

    fn resolve_outer(&self, name: &str) -> Option<usize> {
        if self.scopes.len() <= 1 {
            return None;
        }
        for scope in self.scopes[..self.scopes.len() - 1].iter().rev() {
            if let Some(&idx) = scope.get(name) {
                return Some(idx);
            }
        }
        None
    }

    fn resolve_ref_capture_read(&self, name: &str) -> Option<usize> {
        if !self.is_ref_capture_scope() || self.current_scope().contains_key(name) {
            return None;
        }
        let def_idx = self.resolve_outer(name)?;
        let def = self.defs.get(def_idx)?;
        (def.kind != DefKind::Builtin).then_some(def_idx)
    }

    fn resolve_ref_capture_write(&self, name: &str, plain_assignment: bool) -> Option<usize> {
        let def_idx = self.resolve_ref_capture_read(name)?;
        let def = self.defs.get(def_idx)?;
        if plain_assignment && def.parent.is_none() {
            return None;
        }
        Some(def_idx)
    }

    fn is_ref_capture_scope(&self) -> bool {
        self.ref_capture_stack.last().copied().unwrap_or(false)
    }

    fn read_use(&self, name: &str) -> (UseKind, Option<usize>) {
        if let Some(def_idx) = self.resolve_ref_capture_read(name) {
            (UseKind::RefCaptureRead, Some(def_idx))
        } else {
            (UseKind::Read, self.resolve(name))
        }
    }

    fn bind(&mut self, name: &str, def_idx: usize) {
        self.current_scope_mut().insert(name.to_string(), def_idx);
    }

    fn add_def(
        &mut self,
        name: &str,
        span: Option<(usize, usize)>,
        name_span: Option<(usize, usize)>,
        kind: DefKind,
        params: Option<Vec<String>>,
        parent: Option<usize>,
    ) -> usize {
        let idx = self.defs.len();
        self.defs.push(SymbolDef {
            name: name.to_string(),
            span,
            name_span,
            kind,
            params,
            parent,
        });
        idx
    }

    fn add_use(
        &mut self,
        name: &str,
        span: Option<(usize, usize)>,
        kind: UseKind,
        def_idx: Option<usize>,
    ) {
        self.uses.push(SymbolUse {
            name: name.to_string(),
            span,
            kind,
            def_idx,
        });
    }

    fn analyze(&mut self, node: &AstNode) {
        match node {
            AstNode::Error(err, span) => {
                if let Some(span) = *span {
                    self.errors.push((span, err.clone()));
                }
            }
            AstNode::Literal(v, span) => {
                if let Some(span) = *span {
                    self.literals.push((span, v.clone()));
                }
            }
            AstNode::Variable(name, span) => {
                let (kind, def_idx) = self.read_use(name);
                self.add_use(name, *span, kind, def_idx);
            }
            AstNode::OuterVariable(name, span) => {
                let def_idx = self.resolve_outer(name);
                self.add_use(name, *span, UseKind::OuterRead, def_idx);
            }
            AstNode::BinaryOp { left, right, .. } => {
                self.analyze_binary_chain(left, right);
            }
            AstNode::ComparisonChain { first, rest } => {
                self.analyze(first);
                for (_, n) in rest {
                    self.analyze(n);
                }
            }
            AstNode::UnaryOp { operand, .. } => {
                self.analyze(operand);
            }
            AstNode::Group { expr, .. } => {
                self.analyze(expr);
            }
            AstNode::Range {
                start, end, step, ..
            } => {
                self.analyze(start);
                self.analyze(end);
                if let Some(s) = step {
                    self.analyze(s);
                }
            }
            AstNode::Assignment {
                name,
                op,
                value,
                span,
                name_span,
            } => {
                if name.starts_with(PARSER_INTERNAL_PREFIX) {
                    self.analyze(value);
                    return;
                }
                if let AstNode::Function {
                    params,
                    ref_capture,
                    body,
                } = &**value
                {
                    // Named function: name is visible in body for recursion.
                    let param_names = params
                        .as_ref()
                        .map(|ps| ps.iter().map(|p| p.name().to_string()).collect());
                    let parent = self.func_stack.last().copied();
                    let func_def_idx = self.add_def(
                        name,
                        *span,
                        *name_span,
                        DefKind::Function,
                        param_names,
                        parent,
                    );
                    self.bind(name, func_def_idx);

                    self.push_scope();
                    self.bind(name, func_def_idx);
                    self.ref_capture_stack.push(*ref_capture);

                    self.func_stack.push(func_def_idx);
                    if let Some(ps) = params {
                        for p in ps {
                            let pname = p.name();
                            let pspan = p.span();
                            let def_idx = self.add_def(
                                pname,
                                pspan,
                                pspan,
                                DefKind::Parameter,
                                None,
                                Some(func_def_idx),
                            );
                            self.bind(pname, def_idx);
                        }
                    } else {
                        for p in ["x", "y", "z"] {
                            let def_idx = self.add_def(
                                p,
                                None,
                                None,
                                DefKind::ImplicitParam,
                                None,
                                Some(func_def_idx),
                            );
                            self.bind(p, def_idx);
                        }
                    }
                    self.analyze(body);
                    self.func_stack.pop();
                    self.ref_capture_stack.pop();
                    self.pop_scope();

                    self.add_use(name, *name_span, UseKind::Write, Some(func_def_idx));
                } else {
                    self.analyze(value);
                    if let Some(def_idx) = self.resolve_ref_capture_write(name, op.is_none()) {
                        self.add_use(name, *name_span, UseKind::RefCaptureWrite, Some(def_idx));
                    } else {
                        let parent = self.func_stack.last().copied();
                        let def_idx = self.add_def(
                            name,
                            *name_span,
                            *name_span,
                            DefKind::Assignment,
                            None,
                            parent,
                        );
                        self.bind(name, def_idx);
                        self.add_use(name, *name_span, UseKind::Write, Some(def_idx));
                    }
                }
            }
            AstNode::OuterAssignment {
                name,
                value,
                span: _,
                name_span,
                ..
            } => {
                self.analyze(value);
                let def_idx = self.resolve_outer(name);
                self.add_use(name, *name_span, UseKind::OuterWrite, def_idx);
            }
            AstNode::Ellipsis => {}
            AstNode::List(items) | AstNode::Cat(items) => {
                for item in items {
                    self.analyze(item);
                }
            }
            AstNode::Dict(pairs) => {
                for (_, v) in pairs {
                    self.analyze(v);
                }
            }

            AstNode::Postfix { object, items, .. } => {
                self.analyze(object);
                for item in items {
                    self.analyze(item);
                }
            }
            AstNode::PipeInput => {}
            AstNode::Pipe { input, effect, .. } => {
                self.analyze(input);
                self.analyze(effect);
            }
            AstNode::PipeTap { input, effect, .. } => {
                self.analyze(input);
                self.analyze(effect);
            }
            AstNode::CallName {
                name,
                args,
                span: _,
                name_span,
            } => {
                let (kind, def_idx) = self.read_use(name);
                self.add_use(name, *name_span, kind, def_idx);
                for arg in args {
                    self.analyze(arg);
                }
            }
            AstNode::CallAnonymous { object, args, .. } => {
                self.analyze(object);
                for arg in args {
                    self.analyze(arg);
                }
            }
            AstNode::Index { object, index, .. } => {
                self.analyze(object);
                self.analyze(index);
            }
            AstNode::MutatingIndex { object, index, .. } => {
                self.analyze(object);
                self.analyze(index);
            }
            AstNode::IndexAssign {
                object,
                index,
                value,
                ..
            } => {
                self.analyze(object);
                self.analyze(index);
                self.analyze(value);
            }
            AstNode::MutatingIndexAssign {
                object,
                index,
                value,
                ..
            } => {
                self.analyze(object);
                self.analyze(index);
                self.analyze(value);
            }
            AstNode::Function {
                params,
                ref_capture,
                body,
            } => {
                // Anonymous function: create a synthetic def so inner assignments
                // can be nested under it in the symbol tree.
                let lambda_span = body.span();
                let parent = self.func_stack.last().copied();
                let lambda_idx =
                    self.add_def("{...}", lambda_span, None, DefKind::Function, None, parent);
                self.func_stack.push(lambda_idx);
                self.push_scope();
                self.ref_capture_stack.push(*ref_capture);
                if let Some(ps) = params {
                    for p in ps {
                        let pname = p.name();
                        let pspan = p.span();
                        let def_idx = self.add_def(
                            pname,
                            pspan,
                            pspan,
                            DefKind::Parameter,
                            None,
                            Some(lambda_idx),
                        );
                        self.bind(pname, def_idx);
                    }
                } else {
                    for p in ["x", "y", "z"] {
                        let def_idx = self.add_def(
                            p,
                            None,
                            None,
                            DefKind::ImplicitParam,
                            None,
                            Some(lambda_idx),
                        );
                        self.bind(p, def_idx);
                    }
                }
                self.analyze(body);
                self.ref_capture_stack.pop();
                self.pop_scope();
                self.func_stack.pop();
            }
            AstNode::Conditional {
                condition,
                true_branch,
                false_branch,
                ..
            } => {
                self.analyze(condition);
                self.analyze(true_branch);
                if let Some(fb) = false_branch {
                    self.analyze(fb);
                }
            }
            AstNode::ConditionalDot {
                condition,
                true_branch,
                ..
            } => {
                self.analyze(condition);
                self.analyze(true_branch);
            }
            AstNode::ConditionalChain {
                pairs,
                default_branch,
                ..
            } => {
                for (cond, branch) in pairs {
                    self.analyze(cond);
                    self.analyze(branch);
                }
                self.analyze(default_branch);
            }
            AstNode::WLoop {
                condition, body, ..
            } => {
                self.analyze(condition);
                self.analyze(body);
            }
            AstNode::NLoop { count, body, .. } => {
                self.analyze(count);
                let def_idx = self.current_scope().get("_n").copied().unwrap_or_else(|| {
                    let parent = self.func_stack.last().copied();
                    let idx = self.add_def("_n", None, None, DefKind::LoopCounter, None, parent);
                    self.bind("_n", idx);
                    idx
                });
                self.bind("_n", def_idx);
                self.analyze(body);
            }
            AstNode::Break | AstNode::Continue => {}
            AstNode::Return(expr) => {
                if let Some(e) = expr {
                    self.analyze(e);
                }
            }
            AstNode::Assert { expr, .. } | AstNode::Debug { expr, .. } => {
                self.analyze(expr);
            }
            AstNode::Pause { expr, .. } => {
                if let Some(expr) = expr {
                    self.analyze(expr);
                }
            }
            AstNode::Try(expr) => {
                self.analyze(expr);
            }
            AstNode::Block(stmts) | AstNode::BlockExpr(stmts, ..) => {
                for stmt in stmts {
                    self.analyze(stmt);
                }
            }
            AstNode::UnpackAssignment { lhs, rhs, .. } => {
                self.analyze(rhs);
                for item in lhs {
                    self.analyze_unpack_target(item);
                }
            }
            AstNode::NamedArg { value, .. } => {
                self.analyze(value);
            }
            AstNode::FString { parts, .. } => {
                for part in parts {
                    if let crate::astnode::FStringPart::Expr {
                        expr, spec_exprs, ..
                    } = part
                    {
                        self.analyze(expr);
                        for spec_expr in spec_exprs {
                            self.analyze(spec_expr);
                        }
                    }
                }
            }
        }
    }

    fn analyze_unpack_target(&mut self, node: &AstNode) {
        match node {
            AstNode::Variable(name, span) if name != "_" => {
                let parent = self.func_stack.last().copied();
                let def_idx = self.add_def(name, *span, *span, DefKind::Assignment, None, parent);
                self.bind(name, def_idx);
                self.add_use(name, *span, UseKind::Write, Some(def_idx));
            }
            AstNode::Index { object, index, .. } | AstNode::MutatingIndex { object, index, .. } => {
                self.analyze(object);
                self.analyze(index);
            }
            AstNode::Postfix { object, items, .. } => {
                self.analyze(object);
                for item in items {
                    self.analyze(item);
                }
            }
            AstNode::List(items) => {
                for item in items {
                    self.analyze_unpack_target(item);
                }
            }
            AstNode::Ellipsis => {}
            _ => self.analyze(node),
        }
    }

    fn analyze_binary_chain(&mut self, mut left: &AstNode, right: &AstNode) {
        let mut rights = vec![right];
        while let AstNode::BinaryOp {
            left: next_left,
            right: next_right,
            ..
        } = left
        {
            rights.push(next_right);
            left = next_left;
        }

        self.analyze(left);
        for node in rights.into_iter().rev() {
            self.analyze(node);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::astnode::AstNode;
    use crate::lex::Lexer;
    use crate::parse::Parser;
    use crate::parse::resolve::Resolver;

    fn parse(src: &str) -> AstNode {
        let tokens = Lexer::new(src).tokenize().unwrap();
        let mut parser =
            Parser::new_with_builtins(tokens, src.to_string(), crate::builtins::Builtins::new());
        let ast = parser.parse().unwrap();
        Resolver::with_builtins(crate::builtins::Builtins::new()).resolve(ast)
    }

    fn spans_of(result: &SymbolQueryResult, kind: UseKind) -> Vec<(usize, usize)> {
        result
            .uses
            .iter()
            .filter(|u| u.kind == kind)
            .map(|u| u.span)
            .collect()
    }

    #[test]
    fn simple_assignment_and_read() {
        let ast = parse("a:1; echo a");
        let index = SymbolIndex::analyze(&ast, &crate::builtins::Builtins::new());
        let res = index.query_at(0).unwrap(); // on 'a' in a:1
        assert_eq!(res.name, "a");
        assert_eq!(res.def_span, Some((0, 1)));
        assert_eq!(spans_of(&res, UseKind::Write), vec![(0, 1)]);
        // "echo a" -> 'a' is at byte 10
        assert_eq!(spans_of(&res, UseKind::Read), vec![(10, 11)]);
    }

    #[test]
    fn function_params_and_body() {
        let ast = parse("f:{[x] x+1}; f[2]");
        let index = SymbolIndex::analyze(&ast, &crate::builtins::Builtins::new());

        // Query parameter 'x' at its use in the body (position of 'x' in "x+1")
        // f:{[x] x+1}  ->  f=0 :=1 {=2 [=3 x=4 ]=5  =6 x=7 +=8 1=9 }=10
        let res_x = index.query_at(7).unwrap();
        assert_eq!(res_x.name, "x");
        // Parameter def now has a source span
        assert_eq!(res_x.def_span, Some((4, 5)));
        // The parameter def span is included as a Read use, along with the body read
        assert_eq!(spans_of(&res_x, UseKind::Read), vec![(4, 5), (7, 8)]);

        // Verify parent links
        let f_def = index
            .defs
            .iter()
            .find(|d| d.name == "f" && d.kind == DefKind::Function)
            .unwrap();
        let f_idx = index
            .defs
            .iter()
            .position(|d| d.name == "f" && d.kind == DefKind::Function)
            .unwrap();
        assert_eq!(f_def.parent, None);
        let x_def = index
            .defs
            .iter()
            .find(|d| d.name == "x" && d.kind == DefKind::Parameter)
            .unwrap();
        assert_eq!(x_def.parent, Some(f_idx));

        // Query function name 'f' at assignment
        let res_f = index.query_at(0).unwrap();
        assert_eq!(res_f.name, "f");
        assert_eq!(spans_of(&res_f, UseKind::Write), vec![(0, 1)]);
        // Call f[2] is a read of f
        assert_eq!(spans_of(&res_f, UseKind::Read), vec![(13, 14)]);
    }

    #[test]
    fn occurrences_carry_parameter_and_origin_provenance() {
        let ast = parse("f:{[x] x+1}; g:{[] y:2; y}; z:3");
        let index = SymbolIndex::analyze(&ast, &crate::builtins::Builtins::new());

        let x_idx = index
            .defs
            .iter()
            .position(|d| d.name == "x" && d.kind == DefKind::Parameter)
            .expect("x parameter def");
        let x_occurrences: Vec<_> = index
            .occurrences()
            .into_iter()
            .filter(|occurrence| occurrence.def_idx == x_idx)
            .map(|occurrence| occurrence.span)
            .collect();
        assert_eq!(x_occurrences, vec![(4, 5), (7, 8)]);

        let x_provenance = index.def_provenance(x_idx).expect("x provenance");
        assert_eq!(x_provenance.kind, SymbolProvenanceKind::Parameter);
        assert_eq!(x_provenance.origin.as_deref(), Some("f"));

        let y_idx = index
            .defs
            .iter()
            .position(|d| d.name == "y" && d.kind == DefKind::Assignment)
            .expect("y local def");
        let y_provenance = index.def_provenance(y_idx).expect("y provenance");
        assert_eq!(y_provenance.kind, SymbolProvenanceKind::Local);
        assert_eq!(y_provenance.origin.as_deref(), Some("g"));

        let z_idx = index
            .defs
            .iter()
            .position(|d| d.name == "z" && d.kind == DefKind::Assignment)
            .expect("z global def");
        let z_provenance = index.def_provenance(z_idx).expect("z provenance");
        assert_eq!(z_provenance.kind, SymbolProvenanceKind::Global);
        assert_eq!(z_provenance.origin, None);
    }

    #[test]
    fn closure_capture_by_value() {
        let ast = parse("a:1; f:{a}; a:2; f[]");
        let index = SymbolIndex::analyze(&ast, &crate::builtins::Builtins::new());

        // Query first 'a'
        let res_a1 = index.query_at(0).unwrap();
        assert_eq!(res_a1.name, "a");
        assert_eq!(res_a1.def_span, Some((0, 1)));
        // The read inside f should bind to the first 'a' because the function
        // is defined before the second assignment.
        let reads: Vec<_> = res_a1
            .uses
            .iter()
            .filter(|u| u.kind == UseKind::Read)
            .map(|u| u.span)
            .collect();
        assert!(
            reads.contains(&(8, 9)),
            "capture read should bind to first a"
        );
    }

    #[test]
    fn outer_variable_reference() {
        let ast = parse("a:1; f:{'a}");
        let index = SymbolIndex::analyze(&ast, &crate::builtins::Builtins::new());
        // a:1; f:{'a}  ->  a=0 :=1 1=2 ;=3  =4 f=5 :=6 {=7 '=8 a=9 }=10
        let res = index.query_at(9).unwrap(); // on 'a' inside '
        assert_eq!(res.name, "a");
        assert_eq!(res.def_span, Some((0, 1)));
    }

    #[test]
    fn ref_default_read_marks_ref_capture() {
        let ast = parse("a:1; f:'{[] a}; f[]");
        let index = SymbolIndex::analyze(&ast, &crate::builtins::Builtins::new());
        let res = index.query_at(12).unwrap();
        assert_eq!(res.name, "a");
        assert_eq!(res.def_span, Some((0, 1)));
        assert_eq!(spans_of(&res, UseKind::RefCaptureRead), vec![(12, 13)]);

        let def_idx = index
            .defs
            .iter()
            .position(|d| d.name == "a" && d.name_span == Some((0, 1)))
            .unwrap();
        assert!(index.def_has_ref_capture(def_idx));
        assert_eq!(index.ref_capture_spans(), vec![(12, 13)]);
    }

    #[test]
    fn ref_default_write_marks_outer_locals() {
        let src = "outer:{[] a:1; inner:'{[] a:2}; inner[]; a}; outer[]";
        let ast = parse(src);
        let index = SymbolIndex::analyze(&ast, &crate::builtins::Builtins::new());
        let outer_a = src.find("a:1").unwrap();
        let inner_a = src.find("a:2").unwrap();
        let res = index.query_at(outer_a).unwrap();

        assert_eq!(res.name, "a");
        assert_eq!(
            spans_of(&res, UseKind::RefCaptureWrite),
            vec![(inner_a, inner_a + 1)]
        );
    }

    #[test]
    fn ref_default_top_level_plain_assignment_stays_local() {
        let ast = parse("a:1; f:'{[] a:2}; f[]; a");
        let index = SymbolIndex::analyze(&ast, &crate::builtins::Builtins::new());
        let res = index.query_at(0).unwrap();
        assert!(spans_of(&res, UseKind::RefCaptureWrite).is_empty());
    }

    #[test]
    fn ref_default_augmented_assignment_marks_ref_capture() {
        let ast = parse("a:1; f:'{[] a+:2}; f[]; a");
        let index = SymbolIndex::analyze(&ast, &crate::builtins::Builtins::new());
        let res = index.query_at(0).unwrap();
        assert_eq!(spans_of(&res, UseKind::RefCaptureWrite), vec![(12, 13)]);
    }

    #[test]
    fn nloop_counter() {
        let ast = parse("N[3; _n]");
        let index = SymbolIndex::analyze(&ast, &crate::builtins::Builtins::new());
        let res = index.query_at(5).unwrap(); // on _n
        assert_eq!(res.name, "_n");
    }

    #[test]
    fn reassign_same_name() {
        let ast = parse("a:1; echo a; a:2; echo a");
        let index = SymbolIndex::analyze(&ast, &crate::builtins::Builtins::new());

        // Query first assignment: should see first echo only
        let res1 = index.query_at(0).unwrap();
        assert_eq!(res1.def_span, Some((0, 1)));
        let reads1: Vec<_> = res1
            .uses
            .iter()
            .filter(|u| u.kind == UseKind::Read)
            .map(|u| u.span)
            .collect();
        assert_eq!(reads1, vec![(10, 11)]);

        // Query second assignment: should see second echo only
        // a:1; echo a; a:2; echo a
        //  0     5    10 13  15    20  24 25
        let res2 = index.query_at(13).unwrap();
        assert_eq!(res2.def_span, Some((13, 14)));
        let reads2: Vec<_> = res2
            .uses
            .iter()
            .filter(|u| u.kind == UseKind::Read)
            .map(|u| u.span)
            .collect();
        assert_eq!(reads2, vec![(23, 24)]);
    }

    #[test]
    fn query_on_builtin_returns_result() {
        let ast = parse("echo 1");
        let index = SymbolIndex::analyze(&ast, &crate::builtins::Builtins::new());
        // 'echo' is a builtin; query returns a result with def_span=None
        let res = index.query_at(0).unwrap();
        assert_eq!(res.name, "echo");
        assert!(res.def_span.is_none());
        assert!(!res.uses.is_empty());
    }

    #[test]
    fn inner_assignment_nested_under_function() {
        let ast = parse("f:{[x] a:1; b:a+1}");
        let index = SymbolIndex::analyze(&ast, &crate::builtins::Builtins::new());
        let f_def = index
            .defs
            .iter()
            .find(|d| d.name == "f" && d.kind == DefKind::Function)
            .unwrap();
        let f_idx = index
            .defs
            .iter()
            .position(|d| d.name == "f" && d.kind == DefKind::Function)
            .unwrap();
        assert_eq!(f_def.parent, None);
        let a_child = index
            .defs
            .iter()
            .find(|d| d.name == "a" && d.parent == Some(f_idx));
        assert!(
            a_child.is_some(),
            "inner assignment 'a' should be child of 'f'"
        );
        let b_child = index
            .defs
            .iter()
            .find(|d| d.name == "b" && d.parent == Some(f_idx));
        assert!(
            b_child.is_some(),
            "inner assignment 'b' should be child of 'f'"
        );
    }

    #[test]
    fn unpack_assignment_individual_spans() {
        let ast = parse("(a;b):h");
        let index = SymbolIndex::analyze(&ast, &crate::builtins::Builtins::new());
        // Filter out builtins and resolver-unpack temp vars
        let user_defs: Vec<_> = index
            .defs
            .iter()
            .filter(|d| d.kind != DefKind::Builtin && !d.name.starts_with("--"))
            .collect();
        for def in &user_defs {
            eprintln!(
                "def: name={:?} kind={:?} span={:?} name_span={:?}",
                def.name, def.kind, def.span, def.name_span
            );
        }
        let a_def = user_defs
            .iter()
            .find(|d| d.name == "a")
            .expect("'a' def should exist");
        let b_def = user_defs
            .iter()
            .find(|d| d.name == "b")
            .expect("'b' def should exist");
        // In "(a;b):h", 'a' is at byte 1, 'b' is at byte 3
        assert_eq!(
            a_def.name_span,
            Some((1, 2)),
            "'a' name_span should be (1, 2)"
        );
        assert_eq!(
            b_def.name_span,
            Some((3, 4)),
            "'b' name_span should be (3, 4)"
        );
    }
}
