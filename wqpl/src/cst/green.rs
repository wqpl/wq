//! Immutable CST storage.
//!
//! Green nodes/tokens own source text, carry no parents or absolute offsets,
//! and compare by structure. `Arc` sharing keeps cloned or cached subtrees
//! cheap.
//!
//! Invariants:
//!
//! * `GreenNode::text_len()` is the sum of child lengths.
//! * Covered source bytes are stored verbatim in tokens.
//! * No covered source bytes are implicit.

use std::sync::Arc;

use super::kind::SyntaxKind;

/// A child node or token.
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
    /// Construct a token leaf.
    pub fn new(kind: SyntaxKind, text: impl Into<Box<str>>) -> Self {
        debug_assert!(
            kind.is_token(),
            "GreenToken constructed with non-token kind {kind:?}"
        );
        let text = text.into();
        u32::try_from(text.len()).expect("token text exceeds 4 GiB");
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

    /// Byte length of [`Self::text`].
    #[inline]
    pub fn text_len(&self) -> u32 {
        u32::try_from(self.0.text.len()).expect("token text length checked at construction")
    }

    /// Whether two tokens share the same `Arc` allocation.
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
    /// Construct a node from already-built children.
    pub fn new(kind: SyntaxKind, children: Vec<GreenChild>) -> Self {
        debug_assert!(
            kind.is_node(),
            "GreenNode constructed with non-node kind {kind:?}"
        );
        let text_len: u64 = children.iter().map(|c| u64::from(c.text_len())).sum();
        let text_len = u32::try_from(text_len).expect("green node text length exceeds 4 GiB");
        GreenNode(Arc::new(GreenNodeData {
            kind,
            text_len,
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

    /// `Arc` identity check. For content equality, use [`PartialEq`].
    pub fn ptr_eq(&self, other: &GreenNode) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Reconstruct the source text covered by this subtree.
    pub fn text(&self) -> String {
        let capacity = usize::try_from(self.text_len()).expect("u32 text length fits in usize");
        let mut out = String::with_capacity(capacity);
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
        // Whitespace is preserved.
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
        // Recursive write_text preserves nested source text.
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
        // Separate allocations with identical content compare equal.
        let a = root(vec![bin(vec![intlit("1"), plus(), intlit("2")])]);
        let b = root(vec![bin(vec![intlit("1"), plus(), intlit("2")])]);
        assert_eq!(a, b);
        // Different content is not equal.
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
