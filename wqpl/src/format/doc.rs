//! Wadler/Lindig pretty-printing IR.
//!
//! The formatter lowers a CST into a tree of [`Doc`] values, then a renderer
//! turns that tree into a string subject to a target line width. The IR is
//! deliberately small -- every constructor maps onto a tiny piece of layout
//! semantics so the lowering passes stay declarative.
//!
//! ## Semantics in one paragraph
//!
//! A `Doc` describes possible layouts. The renderer chooses between them by
//! looking at each [`Doc::Group`] in turn: try the *flat* (single-line) form
//! first; if it fits in the remaining width, use it; otherwise fall back to
//! the *break* form. Line-break constructors give the lowering pass
//! fine-grained control over which whitespace becomes "space" in flat mode
//! and which becomes "newline" in break mode.
//!
//! ## Construction style
//!
//! All constructors take `Doc` arguments by value and return a `Doc`. There
//! is no `&mut` builder; chain calls instead. The `+` operator concatenates,
//! `nest` indents, `group` introduces a choice point. Together this reads as
//! a direct translation of the layout intent.
//!
//! ```ignore
//! use crate::format::doc::Doc;
//! // `(1; 2; 3)` that may break:
//! //   (
//! //     1;
//! //     2;
//! //     3
//! //   )
//! Doc::group(
//!     Doc::text("(")
//!         + Doc::nest(
//!             2,
//!             Doc::line_soft()
//!                 + Doc::join(Doc::text(";") + Doc::line(), [
//!                     Doc::text("1"),
//!                     Doc::text("2"),
//!                     Doc::text("3"),
//!                 ]),
//!         )
//!         + Doc::line_soft()
//!         + Doc::text(")"),
//! )
//! ```

use std::borrow::Cow;
use std::ops::Add;

/// One node of the pretty-printing tree.
///
/// `Box` is used so the tree is not infinite-size; `Concat` is binary so
/// associativity does not affect rendering. Most lowering helpers build a
/// `Concat` via the [`Add`] impl, which makes long sequences read naturally.
#[derive(Debug, Clone)]
pub enum Doc {
    /// Empty document. Useful as a no-op when conditionally including a
    /// piece of layout.
    Nil,

    /// Verbatim text. The renderer pushes this without re-flowing.
    Text(Cow<'static, str>),

    /// Two documents concatenated, in order.
    Concat(Box<Doc>, Box<Doc>),

    /// Indent the inner document by `n` columns whenever it breaks across
    /// multiple lines. Has no effect on flat layouts.
    Nest(i32, Box<Doc>),

    /// Try the inner document flat first; if it doesn't fit on the current
    /// line, render it broken. Groups can be nested; the choice is made
    /// independently for each.
    Group(Box<Doc>),

    /// In flat mode: render as one space. In break mode: newline + current
    /// indentation. This is the workhorse "soft separator".
    Line,

    /// In flat mode: render as nothing. In break mode: newline + current
    /// indentation. Use this when no space is wanted in the single-line
    /// form (e.g. immediately inside a bracket).
    LineSoft,

    /// Always render a newline + current indentation, regardless of mode.
    /// Use sparingly: it forces the enclosing group to break.
    LineHard,

    /// Forced blank line: emits a literal blank line above the next
    /// non-trivia output. Preserved blank lines between statements use this.
    /// The renderer collapses runs of `Blank` to a single blank line.
    Blank,
}

impl Doc {
    // ----- atomic constructors -----

    pub fn nil() -> Doc {
        Doc::Nil
    }

    pub fn text<S: Into<Cow<'static, str>>>(s: S) -> Doc {
        Doc::Text(s.into())
    }

    pub fn line() -> Doc {
        Doc::Line
    }

    pub fn line_soft() -> Doc {
        Doc::LineSoft
    }

    pub fn line_hard() -> Doc {
        Doc::LineHard
    }

    pub fn blank() -> Doc {
        Doc::Blank
    }

    // ----- combinators -----

    pub fn concat(a: Doc, b: Doc) -> Doc {
        match (a, b) {
            (Doc::Nil, b) => b,
            (a, Doc::Nil) => a,
            (a, b) => Doc::Concat(Box::new(a), Box::new(b)),
        }
    }

    pub fn group(inner: Doc) -> Doc {
        match inner {
            Doc::Nil => Doc::Nil,
            inner => Doc::Group(Box::new(inner)),
        }
    }

    pub fn nest(n: i32, inner: Doc) -> Doc {
        match inner {
            Doc::Nil => Doc::Nil,
            inner => Doc::Nest(n, Box::new(inner)),
        }
    }

    // ----- composite helpers -----

    /// Join `items` by interleaving `sep` between consecutive elements.
    /// Empty `items` yields [`Doc::Nil`].
    pub fn join<I>(sep: Doc, items: I) -> Doc
    where
        I: IntoIterator<Item = Doc>,
    {
        let mut iter = items.into_iter();
        let first = match iter.next() {
            Some(d) => d,
            None => return Doc::Nil,
        };
        let mut out = first;
        for item in iter {
            out = out + sep.clone() + item;
        }
        out
    }

    /// Convenience: wrap `inner` between `open` and `close`, indenting and
    /// surrounding with soft-lines. The whole thing is grouped.
    ///
    /// `(1; 2)` flat or
    /// ```text
    /// (
    ///   1; 2
    /// )
    /// ```
    /// broken.
    pub fn bracket(open: Doc, inner: Doc, close: Doc, indent: i32, nlcd: bool) -> Doc {
        let close_doc = if nlcd {
            Doc::line_soft() + close
        } else {
            close
        };
        Doc::group(open + Doc::nest(indent, Doc::line_soft() + inner) + close_doc)
    }

    /// True when this doc is `Nil`. The renderer treats `Nil` and `Concat(Nil,
    /// Nil)` identically, but the simplifying [`Doc::concat`] keeps the
    /// tree small.
    pub fn is_nil(&self) -> bool {
        matches!(self, Doc::Nil)
    }

    /// True when this doc, anywhere in its subtree, contains a forced
    /// newline (`LineHard` or `Blank`).
    ///
    /// Used by the renderer to decide whether a `Group` is permitted to
    /// flatten. A group that contains a forced break can never be rendered
    /// on a single line; entering flat mode would produce a layout where
    /// surrounding [`Doc::Line`] / [`Doc::LineSoft`] separators collapse
    /// but the embedded `LineHard` still emits a newline -- the worst of
    /// both worlds.
    ///
    /// O(N) in the size of the subtree; called once per `Group` decision.
    pub fn has_forced_break(&self) -> bool {
        match self {
            Doc::Nil | Doc::Text(_) | Doc::Line | Doc::LineSoft => false,
            Doc::LineHard | Doc::Blank => true,
            Doc::Concat(a, b) => a.has_forced_break() || b.has_forced_break(),
            Doc::Nest(_, inner) | Doc::Group(inner) => inner.has_forced_break(),
        }
    }
}

impl Add for Doc {
    type Output = Doc;
    fn add(self, rhs: Doc) -> Doc {
        Doc::concat(self, rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concat_drops_nil() {
        let d = Doc::text("a") + Doc::nil() + Doc::text("b");
        match d {
            Doc::Concat(l, r) => {
                assert!(matches!(*l, Doc::Text(_)));
                assert!(matches!(*r, Doc::Text(_)));
            }
            other => panic!("expected Concat, got {other:?}"),
        }
    }

    #[test]
    fn nil_is_nil() {
        assert!(Doc::Nil.is_nil());
        assert!(!Doc::text("x").is_nil());
        assert!(Doc::concat(Doc::Nil, Doc::Nil).is_nil());
    }

    #[test]
    fn join_empty() {
        let d = Doc::join(Doc::line(), std::iter::empty::<Doc>());
        assert!(d.is_nil());
    }

    #[test]
    fn join_single_element_has_no_sep() {
        let d = Doc::join(Doc::text(","), [Doc::text("a")]);
        // No separator, just the single text.
        match d {
            Doc::Text(s) => assert_eq!(s, "a"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn group_of_nil_is_nil() {
        let d = Doc::group(Doc::Nil);
        assert!(d.is_nil());
    }

    #[test]
    fn nest_of_nil_is_nil() {
        let d = Doc::nest(2, Doc::Nil);
        assert!(d.is_nil());
    }

    #[test]
    fn bracket_smoke() {
        // Build the canonical bracketed form; verify shape (renderer-level
        // behavior is tested in the render module).
        let d = Doc::bracket(
            Doc::text("("),
            Doc::join(
                Doc::text(";") + Doc::line(),
                [Doc::text("1"), Doc::text("2")],
            ),
            Doc::text(")"),
            2,
            false,
        );
        assert!(matches!(d, Doc::Group(_)));
    }
}
