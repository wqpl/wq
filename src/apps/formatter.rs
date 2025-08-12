use crate::lexer::Lexer;
use crate::parser::{AstNode, BinaryOperator, Parser, UnaryOperator};
use crate::value::WqResult;

#[derive(Debug, Clone)]
pub struct FormatOptions {
    pub indent_size: usize,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self { indent_size: 2 }
    }
}

pub struct Formatter {
    opts: FormatOptions,
}

impl Formatter {
    pub fn new(opts: FormatOptions) -> Self {
        Self { opts }
    }

    pub fn format(&self, node: &AstNode) -> String {
        self.format_node(node, 0)
    }

    fn indent(&self, level: usize) -> String {
        " ".repeat(self.opts.indent_size * level)
    }

    fn join_semicolon(&self, items: &[AstNode], level: usize) -> String {
        items
            .iter()
            .map(|n| self.format_node(n, level))
            .collect::<Vec<_>>()
            .join(";")
    }

    fn format_node(&self, node: &AstNode, level: usize) -> String {
        match node {
            AstNode::Postfix {
                object,
                items,
                explicit_call: _,
            } => {
                let args_str = self.join_semicolon(items, level);
                format!("{}[{}]", self.format_node(object, level), args_str)
            }
            AstNode::Literal(v) => v.to_string(),
            AstNode::Variable(n) => n.clone(),
            AstNode::BinaryOp {
                left,
                operator,
                right,
            } => {
                let op = match operator {
                    BinaryOperator::Add => "+",
                    BinaryOperator::Subtract => "-",
                    BinaryOperator::Multiply => "*",
                    BinaryOperator::Power => "^",
                    BinaryOperator::Divide => "/",
                    BinaryOperator::DivideDot => "/.",
                    BinaryOperator::Modulo => "%",
                    BinaryOperator::ModuloDot => "%.",
                    BinaryOperator::Equal => "=",
                    BinaryOperator::NotEqual => "~",
                    BinaryOperator::LessThan => "<",
                    BinaryOperator::LessThanOrEqual => "<=",
                    BinaryOperator::GreaterThan => ">",
                    BinaryOperator::GreaterThanOrEqual => ">=",
                };
                format!(
                    "{} {} {}",
                    self.format_node(left, level),
                    op,
                    self.format_node(right, level)
                )
            }
            AstNode::UnaryOp { operator, operand } => {
                let op = match operator {
                    UnaryOperator::Negate => "-",
                    UnaryOperator::Count => "#",
                };
                format!("{}{}", op, self.format_node(operand, level))
            }
            AstNode::Assignment { name, value } => {
                format!("{}: {}", name, self.format_node(value, level))
            }
            AstNode::List(items) => {
                let body = self.join_semicolon(items, level);
                format!("({body})")
            }
            AstNode::Dict(pairs) => {
                let mut parts = Vec::new();
                for (k, v) in pairs {
                    parts.push(format!("`{}: {}", k, self.format_node(v, level)));
                }
                format!("({})", parts.join(";"))
            }
            AstNode::Call { name, args } => {
                let args_str = self.join_semicolon(args, level);
                format!("{name}[{args_str};]")
            }
            AstNode::CallAnonymous { object, args } => {
                let args_str = self.join_semicolon(args, level);
                format!("{}[{};]", self.format_node(object, level), args_str)
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
                format!(
                    "{}[{}]:{}",
                    self.format_node(object, level),
                    self.format_node(index, level),
                    self.format_node(value, level)
                )
            }
            AstNode::Function { params, body } => {
                let p = match params {
                    Some(ps) => format!("[{}]", ps.join(";")),
                    None => String::new(),
                };
                match **body {
                    AstNode::Block(ref stmts) => {
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
                    _ => format!("{{{}{}}}", p, self.format_node(body, level)),
                }
            }
            AstNode::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                let cond = self.format_node(condition, level);
                let t = self.format_node(true_branch, level + 1);
                if let Some(f) = false_branch {
                    let f = self.format_node(f, level + 1);
                    format!("$[{cond};\n{t};\n{f}]")
                } else {
                    format!("$.[{cond};\n{t}]")
                }
            }
            AstNode::WhileLoop { condition, body } => {
                let cond = self.format_node(condition, level);
                let body = self.format_node(body, level + 1);
                format!("W[{cond};\n{body}]")
            }
            AstNode::ForLoop { count, body } => {
                let cnt = self.format_node(count, level);
                let body = self.format_node(body, level + 1);
                format!("N[{cnt};\n{body}]")
            }
            AstNode::Break => "@b".to_string(),
            AstNode::Continue => "@c".to_string(),
            AstNode::Return(expr) => match expr {
                Some(e) => format!("@r {}", self.format_node(e, level)),
                None => "@r".to_string(),
            },
            AstNode::Assert(e) => format!("@a {}", self.format_node(e, level)),
            AstNode::Try(e) => format!("@t {}", self.format_node(e, level)),
            AstNode::Block(stmts) => {
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
    }

    fn format_source(&self, src: &str) -> WqResult<String> {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize()?;
        use crate::resolver::Resolver;
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

        for line in content.lines() {
            let trimmed = line.trim();

            // skip comments and empty lines (they are not preserved by the formatter)
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }

            if trimmed.starts_with("load ") || trimmed.starts_with("\\") {
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
