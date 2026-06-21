//! Public entry point for the CST-driven Wadler/Lindig formatter.
//!
//! The actual layout work lives in three siblings:
//!
//! * [`mod@doc`] — the pretty-printing IR.
//! * [`mod@lower`] — CST → [`Doc`] lowering, one branch per
//!   [`crate::cst::SyntaxKind`].
//! * [`mod@render`] — best-fit renderer.
//!
//! This file owns the public surface: [`Formatter`], [`FormatConfig`], and
//! the script-aware [`Formatter::format_script`] driver.

use crate::lex::Lexer;
use crate::parse::Parser;
use crate::value::WqResult;

mod doc;
mod lower;
mod render;
mod wrap;

/// Re-export the public IR + renderer for callers that want to use them
/// directly (e.g. tests, future tools). Most users should go through the
/// [`Formatter`] entry point instead.
pub use doc::Doc;
pub use render::render as render_doc;

#[derive(Debug, Clone)]
pub struct FormatConfig {
    pub indent_size: usize,
    /// Place the closing `]` / `}` of multi-line constructs on its own
    /// line, indented to the parent's column.
    pub nlcd: bool,
    /// Force single-line layouts wherever possible. Strictly weaker than
    /// the per-group flat/break decision: even with a huge `max_width`
    /// some constructs (multi-statement blocks) emit hard newlines that
    /// `one_line_wizard` collapses to `;`.
    pub one_line_wizard: bool,
    /// Target line width for the Wadler/Lindig renderer. Honored when
    /// `one_line_wizard` is false; otherwise everything collapses
    /// regardless. Defaults to 100, the convention picked at the start of
    /// the project.
    pub max_width: usize,
    /// Preserve source spelling and only insert parser-safe wrapping newlines
    /// when a line exceeds `max_width`.
    pub wrap_only: bool,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            indent_size: 2,
            nlcd: false,
            one_line_wizard: false,
            max_width: 100,
            wrap_only: false,
        }
    }
}

pub struct Formatter {
    opts: FormatConfig,
}

impl Formatter {
    pub fn new(opts: FormatConfig) -> Self {
        Self { opts }
    }

    /// Format one expression-block's worth of wq source.
    ///
    /// Pipeline: lex → parser → either wrap-only source preservation, or CST
    /// root → Doc IR → width-aware render. The AST is parsed alongside as a
    /// witness for the CST construction's correctness; it is not consumed here.
    fn format_source(&self, src: &str) -> WqResult<String> {
        use crate::cst::SyntaxNode;
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize()?;
        let wrap_tokens = self.opts.wrap_only.then(|| tokens.clone());
        let mut parser = Parser::new(tokens, src.to_string());
        if self.opts.wrap_only {
            let _ast = parser.parse()?;
            return Ok(wrap::wrap_source(
                src,
                wrap_tokens
                    .as_ref()
                    .expect("wrap_only captured tokens before parser construction"),
                self.opts.max_width,
                self.opts.indent_size,
            ));
        }
        parser.enable_cst();
        let _ast = parser.parse()?;
        let green = parser
            .take_cst()
            .expect("enable_cst was just called, so take_cst yields Some");
        let root = SyntaxNode::new_root(green);
        let doc = lower::lower(&root, &self.opts);
        let width = if self.opts.one_line_wizard {
            usize::MAX
        } else {
            self.opts.max_width
        };
        Ok(render::render(&doc, width))
    }

    /// Format a script that may contain meta commands like `load <path>`.
    ///
    /// Meta lines (starting with `!`, or a shebang `#!` on line 1) are
    /// passed through verbatim. Everything else — including comments and
    /// blank lines — is forwarded to [`Self::format_source`], which
    /// preserves them via CST trivia attachment.
    pub fn format_script(&self, content: &str) -> WqResult<String> {
        let mut result = String::new();
        let mut buffer = String::new();
        let mut buffer_has_payload = false;
        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            // Meta-command lines (shell-style `!cmd`, or a shebang on line 1)
            // bypass the formatter — they aren't wq syntax.
            if trimmed.starts_with("!") || (i == 0 && trimmed.starts_with("#!")) {
                if buffer_has_payload {
                    result.push_str(&self.format_source(&buffer)?);
                    result.push('\n');
                    buffer.clear();
                    buffer_has_payload = false;
                }
                result.push_str(trimmed);
                result.push('\n');
            } else {
                buffer.push_str(line);
                buffer.push('\n');
                if !trimmed.is_empty() {
                    buffer_has_payload = true;
                }
            }
        }
        if buffer_has_payload {
            result.push_str(&self.format_source(&buffer)?);
        }
        Ok(result.trim_end().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_wizard_ignores_width_for_bracketed_forms() {
        let fmt = Formatter::new(FormatConfig {
            one_line_wizard: true,
            max_width: 1,
            ..FormatConfig::default()
        });

        let out = fmt
            .format_script(
                "{[idx;val]$.[or[idx<0;idx>=sz];raise[\"index out of bounds\"]];max[old_cap/%2;4];(`a:1;`b:2)}",
            )
            .expect("format succeeds");

        assert_eq!(
            out,
            "{[idx;val]$.[or[idx<0;idx>=sz];raise \"index out of bounds\"];max[old_cap/%2;4];(`a:1;`b:2)}"
        );
    }

    #[test]
    fn formatter_preserves_postfix_depth_modifier() {
        let fmt = Formatter::new(FormatConfig::default());
        let out = fmt
            .format_script("(1;2)|has?@1[2]\ntil(2;2;2)|has?@2 2")
            .expect("format succeeds");

        assert_eq!(out, "(1;2)|has?@1 2\ntil (2;2;2)|has?@2 2");
    }

    #[test]
    fn wrap_only_preserves_source_except_inserted_breaks() {
        let fmt = Formatter::new(FormatConfig {
            wrap_only: true,
            max_width: 8,
            ..FormatConfig::default()
        });
        let out = fmt.format_script("f[(1; 2; 3; 4; 5)]").expect("format succeeds");

        assert_eq!(out, "f[(1; 2;\n    3;\n    4;\n    5)]");
    }

    #[test]
    fn wrap_only_does_not_apply_full_formatting() {
        let fmt = Formatter::new(FormatConfig {
            wrap_only: true,
            max_width: 80,
            ..FormatConfig::default()
        });
        let out = fmt.format_script("f[(1; 2; 3)]").expect("format succeeds");

        assert_eq!(out, "f[(1; 2; 3)]");
    }
}
