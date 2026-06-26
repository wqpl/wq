//! Concrete syntax tree (green / red) for wq.
//!
//! The CST is the foundation for three things this codebase needs and the
//! existing AST cannot give us:
//!
//! 1. **Span cleanness.** Every byte of source -- including every space, every
//!    comment, and every separator token -- has exactly one home in the tree.
//!    Spans are no longer parser bookkeeping that can drift out of sync; they
//!    are the byte length of a green node, computed once, propagated by
//!    construction.
//! 2. **Real formatting.** The trivia-preserving Wadler/Lindig pretty-printer
//!    in [`crate::format`] consumes a CST. It can therefore preserve user
//!    comments and blank-line groupings, normalize spacing without losing them,
//!    and re-flow long expressions to fit a target width.
//! 3. **Incremental parsing.** Green nodes are immutable, structurally hashed,
//!    and reference-counted. The LSP can splice an unchanged subtree from
//!    yesterday's parse into today's, and the [`GreenNodeBuilder`]'s optional
//!    cache means even a fresh parse will share storage with a previous one
//!    wherever the source did not change.
//!
//! ## Two-layer split
//!
//! * **Green** ([`GreenNode`], [`GreenToken`]) is the immutable storage layer.
//!   It owns the source bytes via leaf tokens; it never carries absolute
//!   positions or parent pointers, which is what makes subtree sharing free.
//! * **Red** ([`SyntaxNode`], [`SyntaxToken`]) is the positional, parent-aware
//!   view. Red nodes are created on demand as the tree is walked, and each one
//!   knows its absolute byte offset and its parent. This is what every
//!   downstream consumer (formatter, lowering, LSP requests) actually walks.
//!
//! See the individual submodules for the detailed contracts.
//!
//! ## Phase 1 scope
//!
//! This module currently provides only the data structures and the builder.
//! It is deliberately decoupled from the lexer: a Phase 2 adapter will read
//! lexer tokens and feed them into a [`GreenNodeBuilder`] (synthesizing
//! whitespace trivia from the gaps between lexer-produced tokens). Until that
//! adapter lands, this module compiles as a self-contained library that the
//! existing parser does not yet use.

mod builder;
mod green;
mod kind;
mod lex;
mod print;
mod red;

pub use builder::{Checkpoint, GreenNodeBuilder};
pub use green::{GreenChild, GreenNode, GreenToken};
pub use kind::SyntaxKind;
pub use lex::{
    build_flat_cst, build_flat_cst_from_tokens, build_flat_cst_recovery, syntax_kind_of_token,
};
pub use red::{
    ChildTokens, Children, ChildrenWithTokens, Descendants, DescendantsWithTokens, SyntaxElement,
    SyntaxNode, SyntaxToken, TextRange,
};

#[cfg(test)]
mod integration_tests {
    use super::*;

    /// Build a small but realistic CST shape and exercise the public API end
    /// to end. This is a smoke test: anything more focused belongs in the
    /// owning submodule.
    #[test]
    fn end_to_end_smoke() {
        // Construct `f[x; 1+2]` with literal whitespace preserved.
        let mut b = GreenNodeBuilder::new();
        b.start_node(SyntaxKind::Root);
        b.start_node(SyntaxKind::PostfixExpr);
        b.token(SyntaxKind::Ident, "f");
        b.start_node(SyntaxKind::ArgList);
        b.token(SyntaxKind::LBrack, "[");
        b.token(SyntaxKind::Ident, "x");
        b.token(SyntaxKind::Semicolon, ";");
        b.token(SyntaxKind::Whitespace, " ");
        b.start_node(SyntaxKind::BinaryExpr);
        b.token(SyntaxKind::IntLit, "1");
        b.token(SyntaxKind::Plus, "+");
        b.token(SyntaxKind::IntLit, "2");
        b.finish_node(); // BinaryExpr
        b.token(SyntaxKind::RBrack, "]");
        b.finish_node(); // ArgList
        b.finish_node(); // PostfixExpr
        b.finish_node(); // Root
        let green = b.finish();

        // Round-trip text.
        assert_eq!(green.text(), "f[x; 1+2]");

        // Walk the red tree.
        let root = SyntaxNode::new_root(green);
        assert_eq!(root.kind(), SyntaxKind::Root);
        assert_eq!(root.text_range(), TextRange::new(0, 9));

        let postfix = root.children().next().expect("postfix child");
        assert_eq!(postfix.kind(), SyntaxKind::PostfixExpr);
        let arglist = postfix.children().next().expect("arglist child");
        assert_eq!(arglist.kind(), SyntaxKind::ArgList);

        // Source layout: `f[x; 1+2]`
        //                  012345678
        // The BinaryExpr `1+2` starts at offset 5 (after "f[x; ").
        let bin = arglist.children().next().expect("bin child");
        assert_eq!(bin.kind(), SyntaxKind::BinaryExpr);
        assert_eq!(bin.text(), "1+2");
        assert_eq!(bin.text_range(), TextRange::new(5, 8));

        // token_at_offset on the `+` (offset 6).
        let plus = root.token_at_offset(6).expect("token at offset 6");
        assert_eq!(plus.kind(), SyntaxKind::Plus);
        assert_eq!(plus.text(), "+");
        assert_eq!(plus.text_range(), TextRange::new(6, 7));

        // Parent chain back to root.
        let parent_of_plus = plus.parent();
        assert_eq!(parent_of_plus.kind(), SyntaxKind::BinaryExpr);
        let grandparent = parent_of_plus.parent().expect("parent");
        assert_eq!(grandparent.kind(), SyntaxKind::ArgList);
    }

    #[test]
    fn cache_shares_storage_across_parses() {
        // Build the same expression twice via separate cached builders. With
        // structural-Eq green nodes and the builder's intern cache, the two
        // top-level subtrees share Arc storage *within each* builder. (Across
        // builders, Arc identity differs, but content equality holds.)
        let make = || {
            let mut b = GreenNodeBuilder::with_cache(64);
            b.start_node(SyntaxKind::Root);
            b.start_node(SyntaxKind::BinaryExpr);
            b.token(SyntaxKind::IntLit, "1");
            b.token(SyntaxKind::Plus, "+");
            b.token(SyntaxKind::IntLit, "2");
            b.finish_node();
            b.token(SyntaxKind::Semicolon, ";");
            b.start_node(SyntaxKind::BinaryExpr);
            b.token(SyntaxKind::IntLit, "1");
            b.token(SyntaxKind::Plus, "+");
            b.token(SyntaxKind::IntLit, "2");
            b.finish_node();
            b.finish_node();
            b.finish()
        };
        let a = make();
        let b = make();
        // Cross-builder content equality.
        assert_eq!(a, b);
        // Within builder, the two binary subtrees are interned to one Arc.
        let mut bins = Vec::new();
        for c in a.children() {
            if let GreenChild::Node(n) = c {
                bins.push(n.clone());
            }
        }
        assert_eq!(bins.len(), 2);
        assert!(bins[0].ptr_eq(&bins[1]));
    }
}
