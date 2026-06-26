//! Red layer: positioned, parent-aware view over a [`super::green::GreenNode`].
//!
//! The red tree is what LSP requests, formatter passes, and lowering all walk.
//! Each red node carries:
//!
//! * a clone of its green node (cheap -- it's an `Arc`),
//! * a back-pointer to its red parent (also cheap; same `Arc` mechanism),
//! * its absolute byte offset within the source document, and
//! * its index in the parent's child list (needed for sibling navigation).
//!
//! Red nodes are created lazily as the tree is traversed. Re-walking a subtree
//! produces new red nodes for the same green data, but with the same identity
//! semantics. To compare red identity (i.e. "is this the same node instance"),
//! use [`SyntaxNode::is_same`]; to compare structurally, compare the underlying
//! [`SyntaxNode::green`] values.
//!
//! The `Arc` parent chain prevents us from recursing on construction: making a
//! root is O(1), and a deeply nested child only allocates one [`Arc`] per
//! ancestor on first access.

use std::sync::Arc;

use super::green::{GreenChild, GreenNode, GreenToken};
use super::kind::SyntaxKind;

/// Half-open byte interval `[start, end)` within the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextRange {
    start: u32,
    end: u32,
}

impl TextRange {
    pub fn new(start: u32, end: u32) -> Self {
        debug_assert!(
            start <= end,
            "TextRange::new: start > end ({start} > {end})"
        );
        TextRange { start, end }
    }

    pub fn empty(at: u32) -> Self {
        TextRange { start: at, end: at }
    }

    #[inline]
    pub fn start(self) -> u32 {
        self.start
    }

    #[inline]
    pub fn end(self) -> u32 {
        self.end
    }

    #[inline]
    pub fn len(self) -> u32 {
        self.end - self.start
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    #[inline]
    pub fn contains(self, offset: u32) -> bool {
        self.start <= offset && offset < self.end
    }

    /// True when `offset` is inside the range *or* exactly at its end. Used by
    /// `token_at_offset` so a cursor at end-of-token still finds something.
    #[inline]
    pub fn contains_inclusive(self, offset: u32) -> bool {
        self.start <= offset && offset <= self.end
    }
}

impl std::fmt::Display for TextRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

#[derive(Debug, Clone)]
pub struct SyntaxNode {
    inner: Arc<SyntaxNodeData>,
}

#[derive(Debug)]
struct SyntaxNodeData {
    green: GreenNode,
    parent: Option<SyntaxNode>,
    /// Index of this node within `parent.green().children()`. `0` for root.
    index_in_parent: u32,
    /// Absolute byte offset within the source document.
    abs_offset: u32,
}

impl SyntaxNode {
    /// Make a fresh red root over a green tree.
    pub fn new_root(green: GreenNode) -> Self {
        SyntaxNode {
            inner: Arc::new(SyntaxNodeData {
                green,
                parent: None,
                index_in_parent: 0,
                abs_offset: 0,
            }),
        }
    }

    #[inline]
    pub fn kind(&self) -> SyntaxKind {
        self.inner.green.kind()
    }

    #[inline]
    pub fn green(&self) -> &GreenNode {
        &self.inner.green
    }

    #[inline]
    pub fn parent(&self) -> Option<&SyntaxNode> {
        self.inner.parent.as_ref()
    }

    /// Index of this node within its parent's child list. Zero for root.
    #[inline]
    pub fn index_in_parent(&self) -> u32 {
        self.inner.index_in_parent
    }

    #[inline]
    pub fn abs_offset(&self) -> u32 {
        self.inner.abs_offset
    }

    pub fn text_range(&self) -> TextRange {
        TextRange::new(
            self.inner.abs_offset,
            self.inner.abs_offset + self.inner.green.text_len(),
        )
    }

    /// Reconstruct the source text covered by this subtree.
    pub fn text(&self) -> String {
        self.inner.green.text()
    }

    /// Whether `self` and `other` are the same red node instance (pointer
    /// equality on the inner `Arc`).
    ///
    /// Use this for "is the user's cursor still on the node we cached?" type
    /// checks. For structural equality, compare [`Self::green`] values.
    pub fn is_same(&self, other: &SyntaxNode) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Iterate over both child nodes and tokens, in source order.
    pub fn children_with_tokens(&self) -> ChildrenWithTokens {
        ChildrenWithTokens {
            parent: self.clone(),
            index: 0,
            offset: self.inner.abs_offset,
        }
    }

    /// Iterate over child *nodes* only (no tokens).
    pub fn children(&self) -> Children {
        Children {
            iter: self.children_with_tokens(),
        }
    }

    /// Iterate over child *tokens* only (no nested nodes).
    pub fn child_tokens(&self) -> ChildTokens {
        ChildTokens {
            iter: self.children_with_tokens(),
        }
    }

    /// First token in this subtree, in source order. None only if the subtree
    /// has no leaves at all (which only happens for explicitly empty nodes).
    pub fn first_token(&self) -> Option<SyntaxToken> {
        for elem in self.children_with_tokens() {
            match elem {
                SyntaxElement::Token(t) => return Some(t),
                SyntaxElement::Node(n) => {
                    if let Some(t) = n.first_token() {
                        return Some(t);
                    }
                }
            }
        }
        None
    }

    /// Last token in this subtree, in source order.
    pub fn last_token(&self) -> Option<SyntaxToken> {
        let elems: Vec<SyntaxElement> = self.children_with_tokens().collect();
        for elem in elems.into_iter().rev() {
            match elem {
                SyntaxElement::Token(t) => return Some(t),
                SyntaxElement::Node(n) => {
                    if let Some(t) = n.last_token() {
                        return Some(t);
                    }
                }
            }
        }
        None
    }

    /// Return the token covering `offset`, descending into child nodes as
    /// needed.
    ///
    /// Semantics:
    ///
    /// * For any offset *strictly inside* a token's range, that token is
    ///   returned (`start <= offset < end`).
    /// * For an offset that falls exactly on a boundary between two tokens, the
    ///   right-hand (later) token wins, because `[start, end)` ranges make a
    ///   cursor positioned at `start` belong to the new token.
    /// * For an offset equal to the parent's end (cursor at end-of-source or
    ///   end-of-subtree), the last token in the subtree is returned, so
    ///   end-of-line cursor lookups still find the previous token.
    /// * For an offset outside the parent, [`None`].
    pub fn token_at_offset(&self, offset: u32) -> Option<SyntaxToken> {
        let range = self.text_range();
        if !range.contains_inclusive(offset) {
            return None;
        }
        // Strict containment first.
        for elem in self.children_with_tokens() {
            let er = elem.text_range();
            if er.contains(offset) {
                return match elem {
                    SyntaxElement::Token(t) => Some(t),
                    SyntaxElement::Node(n) => n.token_at_offset(offset),
                };
            }
        }
        // Boundary case: offset == self.end. Fall back to the rightmost
        // token of the rightmost element (if any) that ends at `offset`.
        if offset == range.end() {
            return self.last_token();
        }
        None
    }

    /// Pre-order traversal of every descendant node (excluding `self`).
    pub fn descendants(&self) -> Descendants {
        Descendants {
            stack: vec![self.children_with_tokens()],
        }
    }

    /// Pre-order traversal of every descendant element (nodes and tokens),
    /// including the descendants of nested nodes.
    pub fn descendants_with_tokens(&self) -> DescendantsWithTokens {
        DescendantsWithTokens {
            stack: vec![self.children_with_tokens()],
        }
    }
}

/// Token leaf of the red tree.
#[derive(Debug, Clone)]
pub struct SyntaxToken {
    parent: SyntaxNode,
    green: GreenToken,
    index_in_parent: u32,
    abs_offset: u32,
}

impl SyntaxToken {
    #[inline]
    pub fn kind(&self) -> SyntaxKind {
        self.green.kind()
    }

    #[inline]
    pub fn text(&self) -> &str {
        self.green.text()
    }

    #[inline]
    pub fn green(&self) -> &GreenToken {
        &self.green
    }

    #[inline]
    pub fn parent(&self) -> &SyntaxNode {
        &self.parent
    }

    #[inline]
    pub fn index_in_parent(&self) -> u32 {
        self.index_in_parent
    }

    #[inline]
    pub fn abs_offset(&self) -> u32 {
        self.abs_offset
    }

    pub fn text_range(&self) -> TextRange {
        TextRange::new(self.abs_offset, self.abs_offset + self.green.text_len())
    }
}

/// One entry of [`SyntaxNode::children_with_tokens`].
#[derive(Debug, Clone)]
pub enum SyntaxElement {
    Node(SyntaxNode),
    Token(SyntaxToken),
}

impl SyntaxElement {
    pub fn kind(&self) -> SyntaxKind {
        match self {
            SyntaxElement::Node(n) => n.kind(),
            SyntaxElement::Token(t) => t.kind(),
        }
    }

    pub fn text_range(&self) -> TextRange {
        match self {
            SyntaxElement::Node(n) => n.text_range(),
            SyntaxElement::Token(t) => t.text_range(),
        }
    }

    pub fn into_node(self) -> Option<SyntaxNode> {
        match self {
            SyntaxElement::Node(n) => Some(n),
            SyntaxElement::Token(_) => None,
        }
    }

    pub fn into_token(self) -> Option<SyntaxToken> {
        match self {
            SyntaxElement::Token(t) => Some(t),
            SyntaxElement::Node(_) => None,
        }
    }

    pub fn as_node(&self) -> Option<&SyntaxNode> {
        match self {
            SyntaxElement::Node(n) => Some(n),
            SyntaxElement::Token(_) => None,
        }
    }

    pub fn as_token(&self) -> Option<&SyntaxToken> {
        match self {
            SyntaxElement::Token(t) => Some(t),
            SyntaxElement::Node(_) => None,
        }
    }
}

/// Iterator returned by [`SyntaxNode::children_with_tokens`].
pub struct ChildrenWithTokens {
    parent: SyntaxNode,
    index: u32,
    offset: u32,
}

impl Iterator for ChildrenWithTokens {
    type Item = SyntaxElement;

    fn next(&mut self) -> Option<SyntaxElement> {
        let children = self.parent.inner.green.children();
        let idx = self.index as usize;
        let child = children.get(idx)?;
        let item = match child {
            GreenChild::Node(g) => SyntaxElement::Node(SyntaxNode {
                inner: Arc::new(SyntaxNodeData {
                    green: g.clone(),
                    parent: Some(self.parent.clone()),
                    index_in_parent: self.index,
                    abs_offset: self.offset,
                }),
            }),
            GreenChild::Token(g) => SyntaxElement::Token(SyntaxToken {
                parent: self.parent.clone(),
                green: g.clone(),
                index_in_parent: self.index,
                abs_offset: self.offset,
            }),
        };
        self.offset += child.text_len();
        self.index += 1;
        Some(item)
    }
}

/// Iterator returned by [`SyntaxNode::children`].
pub struct Children {
    iter: ChildrenWithTokens,
}

impl Iterator for Children {
    type Item = SyntaxNode;

    fn next(&mut self) -> Option<SyntaxNode> {
        for elem in self.iter.by_ref() {
            if let SyntaxElement::Node(n) = elem {
                return Some(n);
            }
        }
        None
    }
}

/// Iterator returned by [`SyntaxNode::child_tokens`].
pub struct ChildTokens {
    iter: ChildrenWithTokens,
}

impl Iterator for ChildTokens {
    type Item = SyntaxToken;

    fn next(&mut self) -> Option<SyntaxToken> {
        for elem in self.iter.by_ref() {
            if let SyntaxElement::Token(t) = elem {
                return Some(t);
            }
        }
        None
    }
}

/// Iterator returned by [`SyntaxNode::descendants`]. Pre-order, depth-first.
pub struct Descendants {
    stack: Vec<ChildrenWithTokens>,
}

impl Iterator for Descendants {
    type Item = SyntaxNode;

    fn next(&mut self) -> Option<SyntaxNode> {
        while let Some(top) = self.stack.last_mut() {
            match top.next() {
                Some(SyntaxElement::Node(n)) => {
                    self.stack.push(n.children_with_tokens());
                    return Some(n);
                }
                Some(SyntaxElement::Token(_)) => continue,
                None => {
                    self.stack.pop();
                }
            }
        }
        None
    }
}

/// Iterator returned by [`SyntaxNode::descendants_with_tokens`]. Pre-order.
pub struct DescendantsWithTokens {
    stack: Vec<ChildrenWithTokens>,
}

impl Iterator for DescendantsWithTokens {
    type Item = SyntaxElement;

    fn next(&mut self) -> Option<SyntaxElement> {
        while let Some(top) = self.stack.last_mut() {
            match top.next() {
                Some(elem @ SyntaxElement::Node(_)) => {
                    if let SyntaxElement::Node(ref n) = elem {
                        self.stack.push(n.children_with_tokens());
                    }
                    return Some(elem);
                }
                Some(elem @ SyntaxElement::Token(_)) => return Some(elem),
                None => {
                    self.stack.pop();
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::super::green::{GreenChild, GreenNode, GreenToken};
    use super::*;

    fn tok(kind: SyntaxKind, text: &str) -> GreenChild {
        GreenChild::Token(GreenToken::new(kind, text))
    }

    fn node(kind: SyntaxKind, children: Vec<GreenChild>) -> GreenChild {
        GreenChild::Node(GreenNode::new(kind, children))
    }

    fn root_with(children: Vec<GreenChild>) -> SyntaxNode {
        SyntaxNode::new_root(GreenNode::new(SyntaxKind::Root, children))
    }

    #[test]
    fn root_offsets_are_zero() {
        let r = root_with(vec![tok(SyntaxKind::IntLit, "42")]);
        assert_eq!(r.abs_offset(), 0);
        assert_eq!(r.text_range(), TextRange::new(0, 2));
    }

    #[test]
    fn child_offsets_accumulate() {
        // "1 + 2"
        let r = root_with(vec![
            tok(SyntaxKind::IntLit, "1"),
            tok(SyntaxKind::Whitespace, " "),
            tok(SyntaxKind::Plus, "+"),
            tok(SyntaxKind::Whitespace, " "),
            tok(SyntaxKind::IntLit, "2"),
        ]);
        let kids: Vec<_> = r.children_with_tokens().collect();
        assert_eq!(kids.len(), 5);
        let ranges: Vec<_> = kids.iter().map(|e| (e.kind(), e.text_range())).collect();
        assert_eq!(
            ranges,
            vec![
                (SyntaxKind::IntLit, TextRange::new(0, 1)),
                (SyntaxKind::Whitespace, TextRange::new(1, 2)),
                (SyntaxKind::Plus, TextRange::new(2, 3)),
                (SyntaxKind::Whitespace, TextRange::new(3, 4)),
                (SyntaxKind::IntLit, TextRange::new(4, 5)),
            ]
        );
    }

    #[test]
    fn nested_offsets_propagate() {
        // "x [1 ; 2]" Postfix call shape
        let bracket = node(
            SyntaxKind::ArgList,
            vec![
                tok(SyntaxKind::LBrack, "["),
                tok(SyntaxKind::IntLit, "1"),
                tok(SyntaxKind::Whitespace, " "),
                tok(SyntaxKind::Semicolon, ";"),
                tok(SyntaxKind::Whitespace, " "),
                tok(SyntaxKind::IntLit, "2"),
                tok(SyntaxKind::RBrack, "]"),
            ],
        );
        let postfix = node(
            SyntaxKind::PostfixExpr,
            vec![
                tok(SyntaxKind::Ident, "x"),
                tok(SyntaxKind::Whitespace, " "),
                bracket,
            ],
        );
        let r = root_with(vec![postfix]);
        assert_eq!(r.text(), "x [1 ; 2]");

        let postfix_node = r
            .children_with_tokens()
            .next()
            .and_then(|e| e.into_node())
            .expect("postfix node");
        assert_eq!(postfix_node.kind(), SyntaxKind::PostfixExpr);
        assert_eq!(postfix_node.abs_offset(), 0);

        let arglist = postfix_node
            .children_with_tokens()
            .nth(2)
            .and_then(|e| e.into_node())
            .expect("arglist node");
        assert_eq!(arglist.abs_offset(), 2);
        assert_eq!(arglist.text(), "[1 ; 2]");
        assert_eq!(arglist.text_range(), TextRange::new(2, 9));
    }

    #[test]
    fn parent_chain() {
        let inner_node = node(
            SyntaxKind::BinaryExpr,
            vec![
                tok(SyntaxKind::IntLit, "1"),
                tok(SyntaxKind::Plus, "+"),
                tok(SyntaxKind::IntLit, "2"),
            ],
        );
        let r = root_with(vec![inner_node]);
        let inner = r.children().next().unwrap();
        assert_eq!(inner.kind(), SyntaxKind::BinaryExpr);
        let parent = inner.parent().expect("inner has parent");
        assert!(parent.is_same(&r));
        assert!(r.parent().is_none());
    }

    #[test]
    fn first_and_last_token() {
        let inner_node = node(
            SyntaxKind::BinaryExpr,
            vec![
                tok(SyntaxKind::IntLit, "1"),
                tok(SyntaxKind::Plus, "+"),
                tok(SyntaxKind::IntLit, "2"),
            ],
        );
        let r = root_with(vec![inner_node]);
        let first = r.first_token().unwrap();
        let last = r.last_token().unwrap();
        assert_eq!(first.text(), "1");
        assert_eq!(last.text(), "2");
        assert_eq!(first.text_range(), TextRange::new(0, 1));
        assert_eq!(last.text_range(), TextRange::new(2, 3));
    }

    #[test]
    fn token_at_offset_finds_correct_leaf() {
        // "foo + bar"
        //   012345678
        let r = root_with(vec![
            tok(SyntaxKind::Ident, "foo"),
            tok(SyntaxKind::Whitespace, " "),
            tok(SyntaxKind::Plus, "+"),
            tok(SyntaxKind::Whitespace, " "),
            tok(SyntaxKind::Ident, "bar"),
        ]);
        // Cursor inside "foo"
        let t = r.token_at_offset(1).unwrap();
        assert_eq!(t.text(), "foo");
        // Cursor on the '+'
        let t = r.token_at_offset(4).unwrap();
        assert_eq!(t.text(), "+");
        // Cursor at end of source
        let t = r.token_at_offset(9).unwrap();
        assert_eq!(t.text(), "bar");
        // Cursor past end
        assert!(r.token_at_offset(10).is_none());
    }

    #[test]
    fn descendants_visit_every_internal_node() {
        let inner = node(
            SyntaxKind::BinaryExpr,
            vec![
                tok(SyntaxKind::IntLit, "1"),
                tok(SyntaxKind::Plus, "+"),
                node(
                    SyntaxKind::BinaryExpr,
                    vec![
                        tok(SyntaxKind::IntLit, "2"),
                        tok(SyntaxKind::Star, "*"),
                        tok(SyntaxKind::IntLit, "3"),
                    ],
                ),
            ],
        );
        let r = root_with(vec![inner]);
        let kinds: Vec<SyntaxKind> = r.descendants().map(|n| n.kind()).collect();
        assert_eq!(kinds, vec![SyntaxKind::BinaryExpr, SyntaxKind::BinaryExpr]);
    }

    #[test]
    fn descendants_with_tokens_includes_leaves() {
        let r = root_with(vec![
            tok(SyntaxKind::IntLit, "1"),
            tok(SyntaxKind::Plus, "+"),
            tok(SyntaxKind::IntLit, "2"),
        ]);
        let kinds: Vec<SyntaxKind> = r.descendants_with_tokens().map(|e| e.kind()).collect();
        assert_eq!(
            kinds,
            vec![SyntaxKind::IntLit, SyntaxKind::Plus, SyntaxKind::IntLit]
        );
    }

    #[test]
    fn red_clone_is_same() {
        let r = root_with(vec![tok(SyntaxKind::IntLit, "1")]);
        let r2 = r.clone();
        assert!(r.is_same(&r2));
    }

    #[test]
    fn is_same_distinguishes_different_walks() {
        // Two separate root-walks of the same green tree produce different
        // red instances (because each `children_with_tokens` call constructs
        // fresh `SyntaxNode`s for child entries). `is_same` reflects that.
        let inner_g = GreenNode::new(
            SyntaxKind::BinaryExpr,
            vec![
                tok(SyntaxKind::IntLit, "1"),
                tok(SyntaxKind::Plus, "+"),
                tok(SyntaxKind::IntLit, "2"),
            ],
        );
        let r = SyntaxNode::new_root(GreenNode::new(
            SyntaxKind::Root,
            vec![GreenChild::Node(inner_g)],
        ));
        let a = r.children().next().unwrap();
        let b = r.children().next().unwrap();
        assert!(!a.is_same(&b));
        // But they are structurally equal.
        assert_eq!(a.green(), b.green());
    }

    #[test]
    fn empty_root_text_range() {
        let r = SyntaxNode::new_root(GreenNode::new(SyntaxKind::Root, Vec::new()));
        assert_eq!(r.text_range(), TextRange::empty(0));
        assert!(r.first_token().is_none());
        assert!(r.last_token().is_none());
    }
}
