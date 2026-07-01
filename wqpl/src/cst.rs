//! Concrete syntax tree for wq.
//!
//! The CST preserves source exactly while exposing structure for formatting,
//! debug output, and editor features.
//!
//! * Green nodes/tokens are immutable, position-free storage. Their lengths are
//!   derived from children, and identical subtrees can be shared.
//! * Red nodes/tokens are positioned views over green storage. They add byte
//!   offsets, parents, and traversal helpers.
//! * The parser builds structured CSTs directly. The lexer adapter also builds
//!   flat token trees for lexer-focused tests and tooling.
//!
//! `GreenNode::text()` round-trips the covered source byte-for-byte.

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

    /// Smoke-test the public API on a small expression.
    #[test]
    fn end_to_end_smoke() {
        // Build `f[x; 1+2]` with literal whitespace preserved.
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

        assert_eq!(green.text(), "f[x; 1+2]");

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

        // `+` sits at offset 6.
        let plus = root.token_at_offset(6).expect("token at offset 6");
        assert_eq!(plus.kind(), SyntaxKind::Plus);
        assert_eq!(plus.text(), "+");
        assert_eq!(plus.text_range(), TextRange::new(6, 7));

        // Parent chain.
        let parent_of_plus = plus.parent();
        assert_eq!(parent_of_plus.kind(), SyntaxKind::BinaryExpr);
        let grandparent = parent_of_plus.parent().expect("parent");
        assert_eq!(grandparent.kind(), SyntaxKind::ArgList);
    }

    #[test]
    fn cache_shares_storage_across_parses() {
        // Each builder interns its duplicate `1+2` nodes; separate builders
        // still produce equal top-level trees.
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
        assert_eq!(a, b);
        // Within one builder, duplicate binary subtrees share storage.
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
