use crate::{
    astnode::{AstNode, UnpackItem, binary_op_display, unary_op_display},
    lexer::Lexer,
    parser::Parser,
    value::WqResult,
};

#[derive(Debug, Clone)]
pub struct FormatConfig {
    pub indent_size: usize,
    pub nlcd: bool,             // newline for closing ']' in control blocks
    pub no_bracket_calls: bool, // use space-call syntax when possible
    pub one_line_wizard: bool,  // force single-line formatting where possible
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            indent_size: 2,
            nlcd: false,
            no_bracket_calls: false,
            one_line_wizard: false,
        }
    }
}

fn escape_for_lexer(s: &str, delim: char) -> String {
    crate::escape::quote_string(s, delim)
}

pub struct Formatter {
    opts: FormatConfig,
}

impl Formatter {
    pub fn new(opts: FormatConfig) -> Self {
        Self { opts }
    }

    pub fn format(&self, node: &AstNode) -> String {
        self.format_node(node, 0)
    }

    fn indent(&self, level: usize) -> String {
        if self.opts.one_line_wizard {
            String::new()
        } else {
            " ".repeat(self.opts.indent_size * level)
        }
    }

    fn join_semicolon(&self, items: &[AstNode], level: usize) -> String {
        items
            .iter()
            .map(|n| self.format_node(n, level))
            .collect::<Vec<_>>()
            .join(";")
    }

    // Format a branch/loop body respecting options.
    // - In one-line mode, collapse Blocks into `stmt1;stmt2` with no indentation.
    // - Otherwise, indent each line for Blocks; indent single expressions.
    fn format_inline_block_or_stmt(&self, node: &AstNode, level: usize) -> String {
        match node {
            AstNode::Block(stmts) => {
                if self.opts.one_line_wizard {
                    stmts
                        .iter()
                        .map(|s| self.format_node(s, level))
                        .collect::<Vec<_>>()
                        .join(";")
                } else {
                    let mut res = String::new();
                    for (i, stmt) in stmts.iter().enumerate() {
                        if i > 0 {
                            res.push('\n');
                        }
                        res.push_str(&self.indent(level));
                        res.push_str(&self.format_node(stmt, level));
                    }
                    res
                }
            }
            _ => {
                if self.opts.one_line_wizard {
                    self.format_node(node, level)
                } else {
                    format!("{}{}", self.indent(level), self.format_node(node, level))
                }
            }
        }
    }

    fn closing_bracket(&self, level: usize) -> String {
        if self.opts.one_line_wizard || !self.opts.nlcd {
            "]".to_string()
        } else {
            format!("\n{}]", self.indent(level))
        }
    }

    fn needs_paren_arg(node: &AstNode) -> bool {
        use AstNode::*;
        match node {
            // Negative numeric literals must be wrapped to avoid `fn -1` ambiguity
            Literal(v) => match v {
                crate::value::Value::Int(n) if *n < 0 => true,
                crate::value::Value::Float(f) if *f < 0.0 => true,
                _ => false,
            },
            // Primaries that parse fine without extra parens
            Variable(_)
            | Function { .. }
            | List(_)
            | Dict(_)
            | Call { .. }
            | CallAnonymous { .. }
            | Postfix { .. }
            | Index { .. }
            | BlockExpr(_) => false,
            // Everything else: wrap to be safe in space-call syntax
            _ => true,
        }
    }

    fn fmt_arg_as_call(&self, node: &AstNode, level: usize) -> String {
        let s = self.format_node(node, level);
        if Self::needs_paren_arg(node) {
            format!("({s})")
        } else {
            s
        }
    }

    fn format_node(&self, node: &AstNode, level: usize) -> String {
        match node {
            AstNode::Ellipsis => "...".to_string(),
            AstNode::Postfix {
                object,
                items,
                explicit_call: _,
            } => {
                let args_str = self.join_semicolon(items, level);
                format!("{}[{}]", self.format_node(object, level), args_str)
            }
            AstNode::Literal(v) => {
                if let Ok(s) = v.try_to_string() {
                    escape_for_lexer(&s, '"')
                } else {
                    v.to_string()
                }
            }
            AstNode::Variable(n) => n.clone(),
            AstNode::BinaryOp {
                left,
                operator,
                right,
            } => {
                let op = binary_op_display(operator);
                if self.opts.one_line_wizard {
                    format!(
                        "{}{}{}",
                        self.format_node(left, level),
                        op,
                        self.format_node(right, level)
                    )
                } else {
                    format!(
                        "{} {} {}",
                        self.format_node(left, level),
                        op,
                        self.format_node(right, level)
                    )
                }
            }
            AstNode::ComparisonChain { first, rest } => {
                let mut out = self.format_node(first, level);
                for (op, node) in rest {
                    let op_str = binary_op_display(op);
                    if self.opts.one_line_wizard {
                        out.push_str(op_str);
                        out.push_str(&self.format_node(node, level));
                    } else {
                        out.push(' ');
                        out.push_str(op_str);
                        out.push(' ');
                        out.push_str(&self.format_node(node, level));
                    }
                }
                out
            }
            AstNode::UnaryOp { operator, operand } => {
                let op = unary_op_display(operator);
                format!("{}{}", op, self.format_node(operand, level))
            }
            AstNode::Range {
                start,
                end,
                step,
                inclusive,
            } => {
                let mut out = format!(
                    "{}{}{}",
                    self.format_node(start, level),
                    if *inclusive { "..=" } else { ".." },
                    self.format_node(end, level)
                );
                if let Some(step) = step {
                    out.push_str("..");
                    out.push_str(&self.format_node(step, level));
                }
                out
            }
            AstNode::Assignment { name, value } => {
                if self.opts.one_line_wizard {
                    format!("{}:{}", name, self.format_node(value, level))
                } else {
                    format!("{}: {}", name, self.format_node(value, level))
                }
            }
            AstNode::UnpackAssign { pattern, value } => {
                let left = {
                    let mut parts = Vec::with_capacity(pattern.len());
                    for p in pattern {
                        match p {
                            UnpackItem::Bind(n) => parts.push(n.clone()),
                            UnpackItem::Skip => parts.push("_".into()),
                            UnpackItem::Ellipsis => parts.push("...".into()),
                        }
                    }
                    format!("({})", parts.join(";"))
                };
                if self.opts.one_line_wizard {
                    format!("{}:{}", left, self.format_node(value, level))
                } else {
                    format!("{}: {}", left, self.format_node(value, level))
                }
            }
            AstNode::List(items) => {
                let body = self.join_semicolon(items, level);
                format!("({body})")
            }
            AstNode::Dict(pairs) => {
                let mut parts = Vec::new();
                for (k, v) in pairs {
                    if self.opts.one_line_wizard {
                        parts.push(format!("`{}:{}", k, self.format_node(v, level)));
                    } else {
                        parts.push(format!("`{}: {}", k, self.format_node(v, level)));
                    }
                }
                format!("({})", parts.join(";"))
            }
            AstNode::Call { name, args } => {
                if self.opts.no_bracket_calls && args.len() == 1 {
                    let arg = self.fmt_arg_as_call(&args[0], level);
                    format!("{name} {arg}")
                } else if self.opts.no_bracket_calls && args.is_empty() {
                    format!("{name}[]")
                } else {
                    let args_str = self.join_semicolon(args, level);
                    format!("{name}[{args_str};]")
                }
            }
            AstNode::CallAnonymous { object, args } => {
                if self.opts.no_bracket_calls && args.len() == 1 {
                    let obj = self.format_node(object, level);
                    let arg = self.fmt_arg_as_call(&args[0], level);
                    format!("{obj} {arg}")
                } else if self.opts.no_bracket_calls && args.is_empty() {
                    format!("{}[]", self.format_node(object, level))
                } else {
                    let args_str = self.join_semicolon(args, level);
                    format!("{}[{};]", self.format_node(object, level), args_str)
                }
            }
            AstNode::Index { object, index } => {
                format!(
                    "{}[{}]",
                    self.format_node(object, level),
                    self.format_node(index, level)
                )
            }
            AstNode::IndexAssign {
                object,
                index,
                value,
            } => {
                if self.opts.one_line_wizard {
                    format!(
                        "{}[{}]:{}",
                        self.format_node(object, level),
                        self.format_node(index, level),
                        self.format_node(value, level)
                    )
                } else {
                    format!(
                        "{}[{}]: {}",
                        self.format_node(object, level),
                        self.format_node(index, level),
                        self.format_node(value, level)
                    )
                }
            }
            AstNode::Function { params, body } => {
                let p = match params {
                    Some(ps) => format!("[{}]", ps.join(";")),
                    None => String::new(),
                };
                match **body {
                    AstNode::Block(ref stmts) => {
                        if self.opts.one_line_wizard {
                            let body = stmts
                                .iter()
                                .map(|s| self.format_node(s, level + 1))
                                .collect::<Vec<_>>()
                                .join(";");
                            format!("{{{p}{body}}}")
                        } else {
                            let mut res = String::new();
                            res.push('{');
                            if !p.is_empty() {
                                res.push_str(&p);
                            }
                            res.push('\n');
                            for stmt in stmts {
                                res.push_str(&self.indent(level + 1));
                                res.push_str(&self.format_node(stmt, level + 1));
                                res.push('\n');
                            }
                            res.push_str(&self.indent(level));
                            res.push('}');
                            res
                        }
                    }
                    _ => {
                        // if self.opts.one_line_wizard {
                        //     format!("{{{}{}}}", p, self.format_node(body, level))
                        // } else {
                        //     format!("{{{}{}}}", p, self.format_node(body, level))
                        // }
                        format!("{{{}{}}}", p, self.format_node(body, level))
                    }
                }
            }
            AstNode::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                let cond = self.format_node(condition, level);
                let t = self.format_inline_block_or_stmt(true_branch, level + 1);
                if let Some(f) = false_branch {
                    let f = self.format_inline_block_or_stmt(f, level + 1);
                    if self.opts.one_line_wizard {
                        format!("$[{cond};{t};{f}]")
                    } else {
                        format!("$[{cond};\n{t};\n{f}{}", self.closing_bracket(level))
                    }
                } else if self.opts.one_line_wizard {
                    format!("$.[{cond};{t}]")
                } else {
                    format!("$.[{cond};\n{t}{}", self.closing_bracket(level))
                }
            }
            AstNode::WLoop { condition, body } => {
                let cond = self.format_node(condition, level);
                let body = self.format_inline_block_or_stmt(body, level + 1);
                if self.opts.one_line_wizard {
                    format!("W[{cond};{body}]")
                } else {
                    format!("W[{cond};\n{body}{}", self.closing_bracket(level))
                }
            }
            AstNode::NLoop { count, body } => {
                let cnt = self.format_node(count, level);
                let body = self.format_inline_block_or_stmt(body, level + 1);
                if self.opts.one_line_wizard {
                    format!("N[{cnt};{body}]")
                } else {
                    format!("N[{cnt};\n{body}{}", self.closing_bracket(level))
                }
            }
            AstNode::FLoop { iterable, body } => {
                let it = self.format_node(iterable, level);
                let body = self.format_inline_block_or_stmt(body, level + 1);
                if self.opts.one_line_wizard {
                    format!("F[{it};{body}]")
                } else {
                    format!("F[{it};\n{body}{}", self.closing_bracket(level))
                }
            }
            AstNode::Break => "@b".to_string(),
            AstNode::Continue => "@c".to_string(),
            AstNode::Return(expr) => match expr {
                Some(e) => {
                    if self.opts.one_line_wizard {
                        format!("@r{}", self.format_node(e, level))
                    } else {
                        format!("@r {}", self.format_node(e, level))
                    }
                }
                None => "@r".to_string(),
            },
            AstNode::Try(e) => {
                if self.opts.one_line_wizard {
                    format!("@t{}", self.format_node(e, level))
                } else {
                    format!("@t {}", self.format_node(e, level))
                }
            }
            AstNode::Block(stmts) => {
                if self.opts.one_line_wizard {
                    stmts
                        .iter()
                        .map(|s| self.format_node(s, level))
                        .collect::<Vec<_>>()
                        .join(";")
                } else {
                    let mut res = String::new();
                    for (i, stmt) in stmts.iter().enumerate() {
                        if i > 0 {
                            res.push('\n');
                        }
                        res.push_str(&self.indent(level));
                        res.push_str(&self.format_node(stmt, level));
                    }
                    res
                }
            }
            AstNode::BlockExpr(stmts) => {
                if stmts.is_empty() {
                    "B[]".to_string()
                } else if self.opts.one_line_wizard {
                    let body = stmts
                        .iter()
                        .map(|s| self.format_node(s, level))
                        .collect::<Vec<_>>()
                        .join(";");
                    format!("B[{body}]")
                } else {
                    let mut body = String::new();
                    for (i, stmt) in stmts.iter().enumerate() {
                        if i > 0 {
                            body.push('\n');
                        }
                        body.push_str(&self.indent(level + 1));
                        body.push_str(&self.format_node(stmt, level + 1));
                    }
                    format!("B[\n{body}{}", self.closing_bracket(level))
                }
            }
        }
    }

    fn format_source(&self, src: &str) -> WqResult<String> {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize()?;
        use crate::post_parser::resolver::Resolver;
        let mut parser = Parser::new(tokens, src.to_string());
        let ast = parser.parse()?;
        let mut resolver = Resolver::new();
        let ast = resolver.resolve(ast);
        Ok(self.format(&ast))
    }

    /// Format a script that may contain meta commands like `load <path>`.
    pub fn format_script(&self, content: &str) -> WqResult<String> {
        let mut result = String::new();
        let mut buffer = String::new();
        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            // skip comments and empty lines (they are not preserved by the formatter)
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }
            // special cmds
            if trimmed.starts_with("!") || (i == 0 && trimmed.starts_with("#!")) {
                if !buffer.trim().is_empty() {
                    result.push_str(&self.format_source(&buffer)?);
                    result.push('\n');
                    buffer.clear();
                }
                result.push_str(trimmed);
                result.push('\n');
            } else {
                buffer.push_str(trimmed);
                buffer.push('\n');
            }
        }
        if !buffer.trim().is_empty() {
            result.push_str(&self.format_source(&buffer)?);
        }
        Ok(result.trim_end().to_string())
    }
}
