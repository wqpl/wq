//! Immutable, position-free, content-addressable green tree.
//!
//! Green nodes/tokens are the storage layer of the CST. They:
//!
//! * Cover every byte of source (including whitespace and comments).
//! * Carry no absolute offsets — only their own length. Absolute positions are
//!   computed by the [`super::red`] layer on demand.
//! * Implement structural [`Hash`] and [`Eq`] so identical subtrees compare
//!   equal regardless of their `Arc` identity. This is the foundation for the
//!   subtree cache that the LSP will use to skip unchanged regions on edits.
//! * Are reference-counted via [`Arc`] so subtrees can be cheaply shared
//!   between multiple parses (and therefore multiple LSP snapshots).
//!
//! The green tree never holds parent pointers; that role belongs to the red
//! tree. Avoiding back-edges here is what makes [`Arc::clone`] of a subtree a
//! constant-time operation.
//!
//! ## Invariants
//!
//! * `GreenNode::text_len()` always equals the sum of its children's
//!   `text_len`s. The constructor enforces this; mutating constructors are not
//!   provided.
//! * Every byte of the source covered by a parse appears verbatim in some token
//!   text. There are no implicit gaps.
//! * `GreenToken::text()` is exactly the bytes the lexer (or the trivia
//!   synthesizer) consumed for that token. No normalization is performed.

use std::sync::Arc;

use super::kind::SyntaxKind;

/// One element of a [`GreenNode`]'s child list.
///
/// Kept as a sum-type rather than a single-tag-bit pointer because the wq
/// codebase already pulls in `Arc` heavily; saving a word per child would not
/// pay for the extra unsafe code.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GreenChild {
    Node(GreenNode),
    Token(GreenToken),
}

impl GreenChild {
    #[inline]
    pub fn kind(&self) -> SyntaxKind {
        match self {
            GreenChild::Node(n) => n.kind(),
            GreenChild::Token(t) => t.kind(),
        }
    }

    #[inline]
    pub fn text_len(&self) -> u32 {
        match self {
            GreenChild::Node(n) => n.text_len(),
            GreenChild::Token(t) => t.text_len(),
        }
    }

    pub fn as_node(&self) -> Option<&GreenNode> {
        match self {
            GreenChild::Node(n) => Some(n),
            GreenChild::Token(_) => None,
        }
    }

    pub fn as_token(&self) -> Option<&GreenToken> {
        match self {
            GreenChild::Token(t) => Some(t),
            GreenChild::Node(_) => None,
        }
    }
}

/// A leaf in the green tree. Owns the verbatim text it covers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GreenToken(Arc<GreenTokenData>);

#[derive(Debug, PartialEq, Eq, Hash)]
struct GreenTokenData {
    kind: SyntaxKind,
    text: Box<str>,
}

impl GreenToken {
    /// Construct a token leaf. The kind must be a token kind; constructing a
    /// token with a node kind is a programmer error and is checked in debug
    /// builds.
    pub fn new(kind: SyntaxKind, text: impl Into<Box<str>>) -> Self {
        debug_assert!(
            kind.is_token(),
            "GreenToken constructed with non-token kind {kind:?}"
        );
        let text = text.into();
        debug_assert!(
            u32::try_from(text.len()).is_ok(),
            "token text exceeds 4 GiB: {} bytes",
            text.len()
        );
        GreenToken(Arc::new(GreenTokenData { kind, text }))
    }

    #[inline]
    pub fn kind(&self) -> SyntaxKind {
        self.0.kind
    }

    #[inline]
    pub fn text(&self) -> &str {
        &self.0.text
    }

    /// Byte length of [`Self::text`]. Always fits in `u32`; see the constructor
    /// debug check.
    #[inline]
    pub fn text_len(&self) -> u32 {
        self.0.text.len() as u32
    }

    /// Whether two tokens point to the *same* `Arc` allocation. Use sparingly;
    /// most callers want structural [`PartialEq`] instead.
    pub fn ptr_eq(&self, other: &GreenToken) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// An internal node of the green tree.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GreenNode(Arc<GreenNodeData>);

#[derive(Debug, PartialEq, Eq, Hash)]
struct GreenNodeData {
    kind: SyntaxKind,
    text_len: u32,
    children: Vec<GreenChild>,
}

impl GreenNode {
    /// Construct a node from already-built children. The resulting node's
    /// `text_len` is the sum of its children's `text_len`s.
    pub fn new(kind: SyntaxKind, children: Vec<GreenChild>) -> Self {
        debug_assert!(
            kind.is_node(),
            "GreenNode constructed with non-node kind {kind:?}"
        );
        let text_len: u64 = children.iter().map(|c| c.text_len() as u64).sum();
        debug_assert!(
            text_len <= u32::MAX as u64,
            "green node text length exceeds 4 GiB"
        );
        GreenNode(Arc::new(GreenNodeData {
            kind,
            text_len: text_len as u32,
            children,
        }))
    }

    #[inline]
    pub fn kind(&self) -> SyntaxKind {
        self.0.kind
    }

    #[inline]
    pub fn text_len(&self) -> u32 {
        self.0.text_len
    }

    #[inline]
    pub fn children(&self) -> &[GreenChild] {
        &self.0.children
    }

    /// `Arc` identity check. Cheap, but rarely what you want — see the
    /// [`PartialEq`] derive for content equality.
    pub fn ptr_eq(&self, other: &GreenNode) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Reconstruct the source text covered by this subtree.
    ///
    /// Allocates `text_len()` bytes once and pushes every leaf's text into it
    /// in source order. Round-trip with the original source is guaranteed by
    /// invariant (every byte appears in some token).
    pub fn text(&self) -> String {
        let mut out = String::with_capacity(self.text_len() as usize);
        self.write_text(&mut out);
        out
    }

    /// Same as [`Self::text`], but appends to an existing buffer.
    pub fn write_text(&self, out: &mut String) {
        for child in &self.0.children {
            match child {
                GreenChild::Token(t) => out.push_str(t.text()),
                GreenChild::Node(n) => n.write_text(out),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(text: &str) -> GreenChild {
        GreenChild::Token(GreenToken::new(SyntaxKind::Whitespace, text))
    }

    fn ident(text: &str) -> GreenChild {
        GreenChild::Token(GreenToken::new(SyntaxKind::Ident, text))
    }

    fn intlit(text: &str) -> GreenChild {
        GreenChild::Token(GreenToken::new(SyntaxKind::IntLit, text))
    }

    fn plus() -> GreenChild {
        GreenChild::Token(GreenToken::new(SyntaxKind::Plus, "+"))
    }

    fn bin(children: Vec<GreenChild>) -> GreenChild {
        GreenChild::Node(GreenNode::new(SyntaxKind::BinaryExpr, children))
    }

    fn root(children: Vec<GreenChild>) -> GreenNode {
        GreenNode::new(SyntaxKind::Root, children)
    }

    #[test]
    fn token_round_trip() {
        let t = GreenToken::new(SyntaxKind::Ident, "foo");
        assert_eq!(t.text(), "foo");
        assert_eq!(t.text_len(), 3);
        assert_eq!(t.kind(), SyntaxKind::Ident);
    }

    #[test]
    fn node_text_concatenates_children() {
        // `1 + 2` with whitespace preserved.
        let n = root(vec![bin(vec![
            intlit("1"),
            ws(" "),
            plus(),
            ws(" "),
            intlit("2"),
        ])]);
        assert_eq!(n.text(), "1 + 2");
        assert_eq!(n.text_len(), 5);
    }

    #[test]
    fn deep_tree_round_trips() {
        // `((a + b) * c)` with original whitespace. Tests recursive write_text.
        let inner = bin(vec![ident("a"), ws(" "), plus(), ws(" "), ident("b")]);
        let outer = bin(vec![
            GreenChild::Token(GreenToken::new(SyntaxKind::LParen, "(")),
            inner,
            GreenChild::Token(GreenToken::new(SyntaxKind::RParen, ")")),
            ws(" "),
            GreenChild::Token(GreenToken::new(SyntaxKind::Star, "*")),
            ws(" "),
            ident("c"),
        ]);
        let r = root(vec![
            GreenChild::Token(GreenToken::new(SyntaxKind::LParen, "(")),
            outer,
            GreenChild::Token(GreenToken::new(SyntaxKind::RParen, ")")),
        ]);
        assert_eq!(r.text(), "((a + b) * c)");
    }

    #[test]
    fn structural_equality_independent_of_arc_identity() {
        // Two independently-constructed identical subtrees compare equal even
        // though they are separate allocations. This is what enables the
        // subtree cache.
        let a = root(vec![bin(vec![intlit("1"), plus(), intlit("2")])]);
        let b = root(vec![bin(vec![intlit("1"), plus(), intlit("2")])]);
        assert_eq!(a, b);
        // Different content => not equal.
        let c = root(vec![bin(vec![intlit("1"), plus(), intlit("3")])]);
        assert_ne!(a, c);
    }

    #[test]
    fn hash_matches_equality() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        fn hash_of<T: Hash>(t: &T) -> u64 {
            let mut h = DefaultHasher::new();
            t.hash(&mut h);
            h.finish()
        }
        let a = root(vec![bin(vec![intlit("1"), plus(), intlit("2")])]);
        let b = root(vec![bin(vec![intlit("1"), plus(), intlit("2")])]);
        assert_eq!(hash_of(&a), hash_of(&b));
    }

    #[test]
    fn arc_clone_is_structural_share() {
        let a = root(vec![bin(vec![intlit("1"), plus(), intlit("2")])]);
        let b = a.clone();
        assert!(a.ptr_eq(&b));
    }

    #[test]
    fn child_helpers_dispatch_correctly() {
        let t = ident("x");
        assert!(t.as_token().is_some());
        assert!(t.as_node().is_none());
        assert_eq!(t.kind(), SyntaxKind::Ident);
        assert_eq!(t.text_len(), 1);

        let n = bin(vec![ident("x"), plus(), ident("y")]);
        assert!(n.as_node().is_some());
        assert!(n.as_token().is_none());
        assert_eq!(n.kind(), SyntaxKind::BinaryExpr);
        assert_eq!(n.text_len(), 3);
    }

    #[test]
    fn empty_node_is_legal() {
        let n = GreenNode::new(SyntaxKind::Block, Vec::new());
        assert_eq!(n.text(), "");
        assert_eq!(n.text_len(), 0);
        assert!(n.children().is_empty());
    }
}
