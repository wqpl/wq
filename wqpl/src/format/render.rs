//! Best-fit pretty-printer for the [`Doc`] IR.
//!
//! Standard Wadler / Lindig algorithm:
//!
//! 1. Walk the document in source order, maintaining a stack of pending
//!    `(indent, mode, doc)` triples.
//! 2. At each [`Doc::Group`], decide between *flat* (single-line) and *break*
//!    (multi-line) by asking [`Self::fits`] whether the group's flat rendering
//!    would still fit within `width` after the current column.
//! 3. In flat mode, [`Doc::Line`] becomes a space and [`Doc::LineSoft`] becomes
//!    nothing. In break mode, both become `\n` + indent.
//!
//! The implementation is iterative (no recursion on the doc structure) so
//! deeply nested documents do not blow the stack.

use unicode_width::UnicodeWidthStr as _;

use super::doc::Doc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Render the current subdocument flat, on one line.
    Flat,
    /// Render the current subdocument broken, using newlines.
    Break,
}

#[derive(Debug, Clone)]
struct Frame<'a> {
    indent: i32,
    mode: Mode,
    doc: &'a Doc,
}

/// Render `doc` to a string targeting `width` columns.
///
/// `width` is advisory -- the renderer never breaks a single [`Doc::Text`]
/// even if it exceeds the budget. Idempotence on already-formatted output is
/// a property of the lowering pass, not the renderer: render produces a
/// deterministic byte-exact output for a given input.
///
/// ## Indent emission strategy
///
/// Indent is emitted **lazily**, just before the first non-newline content
/// of a new line. This is what keeps blank lines from carrying trailing
/// whitespace: a `LineHard` followed by another `LineHard` (because of a
/// `Blank` marker, or because of stacked forced breaks) writes only `\n\n`,
/// not `\n  \n` -- the indent is set as pending and overwritten by the next
/// newline before it ever lands in the output.
pub fn render(doc: &Doc, width: usize) -> String {
    let mut s = RenderState::new();
    let mut stack: Vec<Frame<'_>> = vec![Frame {
        indent: 0,
        mode: Mode::Break,
        doc,
    }];

    while let Some(Frame { indent, mode, doc }) = stack.pop() {
        match doc {
            Doc::Nil => {}
            Doc::Text(text) => {
                s.flush_pending_blank();
                s.flush_pending_indent();
                s.push_text(text);
            }
            Doc::Concat(a, b) => {
                stack.push(Frame {
                    indent,
                    mode,
                    doc: b,
                });
                stack.push(Frame {
                    indent,
                    mode,
                    doc: a,
                });
            }
            Doc::Nest(n, inner) => {
                stack.push(Frame {
                    indent: indent + n,
                    mode,
                    doc: inner,
                });
            }
            Doc::Group(inner) => {
                // If any descendant is a forced break (LineHard / Blank),
                // the group must break -- flat is impossible. Otherwise try
                // flat first, fall back to break if it doesn't fit.
                if inner.has_forced_break() {
                    stack.push(Frame {
                        indent,
                        mode: Mode::Break,
                        doc: inner,
                    });
                } else {
                    let flat_frame = Frame {
                        indent,
                        mode: Mode::Flat,
                        doc: inner,
                    };
                    if mode == Mode::Flat
                        || fits(width.saturating_sub(s.col), &stack, flat_frame.clone())
                    {
                        stack.push(flat_frame);
                    } else {
                        stack.push(Frame {
                            indent,
                            mode: Mode::Break,
                            doc: inner,
                        });
                    }
                }
            }
            Doc::Line => match mode {
                Mode::Flat => {
                    s.flush_pending_blank();
                    s.flush_pending_indent();
                    s.push_text(" ");
                }
                Mode::Break => s.emit_newline(indent),
            },
            Doc::LineSoft => match mode {
                Mode::Flat => {
                    // Flushing pending blank in flat mode would force a
                    // newline, which contradicts flat mode. Drop the blank
                    // hint in this case.
                    s.pending_blank = false;
                }
                Mode::Break => s.emit_newline(indent),
            },
            Doc::LineHard => s.emit_newline(indent),
            Doc::Blank => {
                // Defer until we see a non-trivia token; emitted as an
                // *extra* `\n` before the next newline.
                s.pending_blank = true;
            }
        }
    }
    s.out
}

struct RenderState {
    out: String,
    /// Column of the next character to be written. Tracks both already
    /// emitted text and any pending indent.
    col: usize,
    /// Whether a blank line has been requested but not yet emitted. Cleared
    /// by [`Self::emit_newline`] (which honours it) or
    /// [`Self::flush_pending_blank`] (which honours it before
    /// about-to-be-written text).
    pending_blank: bool,
    /// Indent (column count) to emit lazily before the next non-newline
    /// content. `None` means "no indent pending" -- typically we are
    /// mid-line and have already emitted any necessary indent. Lazy
    /// emission avoids trailing whitespace on lines that turn out to be
    /// blank.
    pending_indent: Option<u32>,
}

impl RenderState {
    fn new() -> Self {
        Self {
            out: String::new(),
            col: 0,
            pending_blank: false,
            pending_indent: None,
        }
    }

    fn push_text(&mut self, s: &str) {
        self.out.push_str(s);
        self.col += visual_width(s);
    }

    /// If a blank line is pending, emit it now -- but as a *single* extra
    /// newline before whatever follows (the caller will then emit its own
    /// content). No indent is added: the caller's
    /// [`Self::flush_pending_indent`] handles indent for the next line.
    fn flush_pending_blank(&mut self) {
        if !self.pending_blank {
            return;
        }
        self.pending_blank = false;
        if self.out.is_empty() {
            // Don't open the file with a blank line.
            return;
        }
        if !self.out.ends_with('\n') {
            self.out.push('\n');
        }
        self.out.push('\n');
        // Reset col but keep pending_indent intact -- the caller's next
        // emission will spawn the indent.
        self.col = 0;
        // We just emitted a newline, so we owe an indent for the
        // upcoming content line.
        if self.pending_indent.is_none() {
            // Conservative: indent 0 if nothing requested. Callers that
            // care set pending_indent via emit_newline first.
            self.pending_indent = Some(0);
        }
    }

    fn flush_pending_indent(&mut self) {
        if let Some(n) = self.pending_indent.take() {
            for _ in 0..n {
                self.out.push(' ');
            }
            self.col += n as usize;
        }
    }

    fn emit_newline(&mut self, indent: i32) {
        // Discard any indent we owed to the previous line -- it would have
        // become trailing whitespace on a line that has no other content.
        // The new line's indent is set below as `pending_indent`.
        self.pending_indent = None;
        if self.pending_blank {
            self.pending_blank = false;
            self.out.push('\n');
        }
        self.out.push('\n');
        let n = indent.max(0) as u32;
        self.pending_indent = Some(n);
        self.col = 0;
    }
}

/// Terminal display width used to decide whether a flat layout fits.
fn visual_width(s: &str) -> usize {
    s.width()
}

/// Check whether the doc starting at `frame`, plus the remaining stack,
/// would fit in `remaining` columns if every visited `Group` were rendered
/// flat. Stops as soon as a forced newline is encountered (which would push
/// the rest to a new line, so the current line's budget is irrelevant after
/// it).
fn fits(mut remaining: usize, stack_below: &[Frame<'_>], start: Frame<'_>) -> bool {
    // We need a local "what we would render next" stack that includes
    // `start` on top of `stack_below`. To avoid cloning the whole base
    // stack, we keep a top-of-stack list and an index into the base.
    let mut top: Vec<Frame<'_>> = vec![start];
    let mut base_idx = stack_below.len();
    loop {
        let frame = match top.pop() {
            Some(f) => f,
            None => {
                if base_idx == 0 {
                    return true;
                }
                base_idx -= 1;
                stack_below[base_idx].clone()
            }
        };
        let Frame { indent, mode, doc } = frame;
        match doc {
            Doc::Nil => {}
            Doc::Text(s) => {
                let w = visual_width(s);
                if w > remaining {
                    return false;
                }
                remaining -= w;
            }
            Doc::Concat(a, b) => {
                top.push(Frame {
                    indent,
                    mode,
                    doc: b,
                });
                top.push(Frame {
                    indent,
                    mode,
                    doc: a,
                });
            }
            Doc::Nest(n, inner) => {
                top.push(Frame {
                    indent: indent + n,
                    mode,
                    doc: inner,
                });
            }
            Doc::Group(inner) => {
                // When measuring fit, treat nested groups as flat -- Wadler's
                // standard simplification. This may approve a layout that
                // the actual renderer breaks (because the renderer reasons
                // group by group), but that is the conservative side: any
                // group that fits when flat will keep its place if flat,
                // and otherwise it breaks anyway.
                top.push(Frame {
                    indent,
                    mode: Mode::Flat,
                    doc: inner,
                });
            }
            Doc::Line => match mode {
                Mode::Flat => {
                    if remaining == 0 {
                        return false;
                    }
                    remaining -= 1;
                }
                Mode::Break => return true,
            },
            Doc::LineSoft => match mode {
                Mode::Flat => {}
                Mode::Break => return true,
            },
            // A forced newline ends the current line. By the time we hit
            // it, the budget so far has been respected, so return "fits".
            // Groups that contain a forced break never get this far --
            // they are detected by `Doc::has_forced_break` *before* the
            // flat/break choice is made, and forced into break mode.
            Doc::LineHard | Doc::Blank => return true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::doc::Doc;
    use super::*;

    fn r(doc: Doc, width: usize) -> String {
        render(&doc, width)
    }

    #[test]
    fn text_passes_through() {
        assert_eq!(r(Doc::text("hello"), 10), "hello");
    }

    #[test]
    fn concat_renders_in_order() {
        let d = Doc::text("a") + Doc::text("b") + Doc::text("c");
        assert_eq!(r(d, 10), "abc");
    }

    #[test]
    fn line_flat_is_space_break_is_newline() {
        // Group with a Line -- fits flat: becomes "a b".
        let flat = Doc::group(Doc::text("a") + Doc::line() + Doc::text("b"));
        assert_eq!(r(flat.clone(), 10), "a b");
        // Doesn't fit flat: becomes "a\nb".
        assert_eq!(r(flat, 2), "a\nb");
    }

    #[test]
    fn nest_indents_break_only() {
        let inner = Doc::nest(2, Doc::line() + Doc::text("body")) + Doc::line() + Doc::text("end");
        let group = Doc::group(Doc::text("{") + inner + Doc::text("}"));
        // Wide budget: flat.
        assert_eq!(r(group.clone(), 80), "{ body end}");
        // Narrow budget: break, with indent.
        assert_eq!(r(group, 5), "{\n  body\nend}");
    }

    #[test]
    fn soft_line_is_nothing_flat() {
        let d = Doc::group(
            Doc::text("(")
                + Doc::nest(2, Doc::line_soft() + Doc::text("x"))
                + Doc::line_soft()
                + Doc::text(")"),
        );
        // Flat: "(x)".
        assert_eq!(r(d.clone(), 80), "(x)");
        // Narrow: "(\n  x\n)".
        assert_eq!(r(d, 1), "(\n  x\n)");
    }

    #[test]
    fn nested_groups_decide_independently() {
        // Outer group fits flat, inner doesn't on its own.
        let inner = Doc::group(
            Doc::text("(")
                + Doc::join(
                    Doc::text(",") + Doc::line(),
                    [
                        Doc::text("aaaaaaa"),
                        Doc::text("bbbbbbb"),
                        Doc::text("ccccccc"),
                    ],
                )
                + Doc::text(")"),
        );
        let outer = Doc::group(Doc::text("f") + Doc::text(" ") + inner);
        // 80-col fits flat.
        assert_eq!(r(outer.clone(), 80), "f (aaaaaaa, bbbbbbb, ccccccc)");
        // 20-col forces the inner to break; outer's `f ` stays.
        assert_eq!(r(outer, 20), "f (aaaaaaa,\nbbbbbbb,\nccccccc)");
    }

    #[test]
    fn line_hard_always_breaks() {
        let d = Doc::text("a") + Doc::line_hard() + Doc::text("b");
        assert_eq!(r(d, 80), "a\nb");
    }

    #[test]
    fn blank_emits_extra_newline() {
        let d = Doc::text("a") + Doc::line_hard() + Doc::blank() + Doc::text("b");
        assert_eq!(r(d, 80), "a\n\nb");
    }

    #[test]
    fn join_with_separator() {
        let d = Doc::join(
            Doc::text(", "),
            [Doc::text("a"), Doc::text("b"), Doc::text("c")],
        );
        assert_eq!(r(d, 80), "a, b, c");
    }

    #[test]
    fn bracket_helper_flat_vs_break() {
        let body = Doc::join(
            Doc::text(";") + Doc::line(),
            [Doc::text("aaa"), Doc::text("bbb"), Doc::text("ccc")],
        );
        let bracketed = Doc::bracket(Doc::text("("), body, Doc::text(")"), 2, false);
        // Wide fits flat.
        assert_eq!(r(bracketed.clone(), 80), "(aaa; bbb; ccc)");
        // Narrow breaks; close stays on last line (nlcd=false).
        assert_eq!(r(bracketed, 5), "(\n  aaa;\n  bbb;\n  ccc)");
    }

    #[test]
    fn empty_doc_renders_empty() {
        assert_eq!(r(Doc::Nil, 80), "");
    }

    #[test]
    fn long_text_is_not_split() {
        // Even if `width` is tiny, a single Text is emitted verbatim.
        assert_eq!(r(Doc::text("verylongidentifier"), 5), "verylongidentifier");
    }

    #[test]
    fn nested_nest_accumulates() {
        let d = Doc::nest(
            2,
            Doc::line() + Doc::nest(3, Doc::line() + Doc::text("x")) + Doc::line() + Doc::text("y"),
        );
        // Force break with a small width.
        let g = Doc::group(d);
        let out = r(g, 1);
        // Lazy indent: every newline drops the previous line's pending
        // indent, so blank lines stay clean. Nest(3) inside Nest(2) gives
        // indent 5 before "x"; indent 2 before "y".
        assert_eq!(out, "\n\n     x\n  y");
    }

    #[test]
    fn blank_lines_carry_no_trailing_whitespace() {
        // Two stmts with a blank line between them, inside an indented
        // block: the blank line must be exactly `\n\n` plus the next
        // line's indent -- no spaces dangling on the otherwise empty
        // middle line.
        let d = Doc::nest(
            2,
            Doc::line_hard() + Doc::text("a") + Doc::line_hard() + Doc::blank() + Doc::text("b"),
        );
        let out = r(d, 80);
        assert_eq!(out, "\n  a\n\n  b");
    }

    #[test]
    fn fit_decision_per_group() {
        // Two groups in sequence; first fits, second doesn't.
        let g1 = Doc::group(Doc::text("aa") + Doc::line() + Doc::text("bb"));
        let g2 = Doc::group(Doc::text("cccccccccc") + Doc::line() + Doc::text("dddddddddd"));
        let d = g1 + Doc::line() + g2;
        let out = r(d, 12);
        // g1 fits flat "aa bb", separator newline, g2 breaks.
        assert_eq!(out, "aa bb\ncccccccccc\ndddddddddd");
    }
}
