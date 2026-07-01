//! Green tree builder.
//!
//! [`GreenNodeBuilder`] consumes parser events: start node, emit token, finish
//! node. [`Checkpoint`] lets the parser wrap already-emitted children, which is
//! useful for left-associative expressions.
//!
//! [`GreenNodeBuilder::with_cache`] interns finished nodes by structural
//! identity up to a fixed limit. [`GreenNodeBuilder::append_node`] lets callers
//! reuse unchanged subtrees.

use std::collections::HashMap;

use super::green::{GreenChild, GreenNode, GreenToken};
use super::kind::SyntaxKind;

/// Position in the open-node stack used by [`GreenNodeBuilder::start_node_at`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint {
    /// Open-node depth when the checkpoint was taken.
    parent_depth: usize,
    /// Child count in that node at checkpoint time.
    children_at: usize,
}

#[derive(Debug)]
struct Frame {
    kind: SyntaxKind,
    children: Vec<GreenChild>,
}

/// Stack-based builder for [`GreenNode`] trees.
#[derive(Debug)]
pub struct GreenNodeBuilder {
    stack: Vec<Frame>,
    /// Finished top-level children. [`Self::finish`] requires exactly one root.
    finished: Vec<GreenChild>,
    /// Optional intern table for finished nodes.
    cache: Option<NodeCache>,
}

#[derive(Debug)]
struct NodeCache {
    map: HashMap<NodeKey, GreenNode>,
    /// Maximum number of entries. We do not evict; insertion stops at the
    /// limit.
    limit: usize,
}

/// Owned cache key. [`GreenNode`] already hashes and compares structurally.
type NodeKey = GreenNode;

impl Default for GreenNodeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl GreenNodeBuilder {
    pub fn new() -> Self {
        GreenNodeBuilder {
            stack: Vec::new(),
            finished: Vec::new(),
            cache: None,
        }
    }

    /// Enable subtree interning, capped at `limit` distinct nodes.
    pub fn with_cache(limit: usize) -> Self {
        GreenNodeBuilder {
            stack: Vec::new(),
            finished: Vec::new(),
            cache: Some(NodeCache {
                map: HashMap::new(),
                limit,
            }),
        }
    }

    /// Open a node. Later builder calls target this node until it is finished.
    pub fn start_node(&mut self, kind: SyntaxKind) {
        debug_assert!(
            kind.is_node(),
            "start_node called with non-node kind {kind:?}"
        );
        self.stack.push(Frame {
            kind,
            children: Vec::new(),
        });
    }

    /// Append a token to the currently-open node.
    pub fn token(&mut self, kind: SyntaxKind, text: impl Into<Box<str>>) {
        debug_assert!(kind.is_token(), "token called with non-token kind {kind:?}");
        let token = GreenToken::new(kind, text);
        let child = GreenChild::Token(token);
        match self.stack.last_mut() {
            Some(frame) => frame.children.push(child),
            None => {
                debug_assert!(false, "token pushed without an open node");
                self.finished.push(child);
            }
        }
    }

    /// Append an already-built green node to the currently-open node.
    pub fn append_node(&mut self, node: GreenNode) {
        let child = GreenChild::Node(node);
        match self.stack.last_mut() {
            Some(frame) => frame.children.push(child),
            None => self.finished.push(child),
        }
    }

    /// Close the current node and attach it to its parent or the root list.
    pub fn finish_node(&mut self) {
        let frame = self
            .stack
            .pop()
            .expect("finish_node called without a matching start_node");
        let node = self.intern(GreenNode::new(frame.kind, frame.children));
        let child = GreenChild::Node(node);
        match self.stack.last_mut() {
            Some(parent) => parent.children.push(child),
            None => self.finished.push(child),
        }
    }

    /// Mark the current child position for a retroactive wrapper.
    pub fn checkpoint(&mut self) -> Checkpoint {
        let frame = self
            .stack
            .last()
            .expect("checkpoint called without an open node");
        Checkpoint {
            parent_depth: self.stack.len() - 1,
            children_at: frame.children.len(),
        }
    }

    /// Wrap children since `checkpoint` in a new open node of `kind`.
    ///
    /// # Panics
    ///
    /// Panics if `checkpoint` was taken in a different open node than the
    /// current one (i.e. the parser unwound past the checkpoint without
    /// closing the wrapping node).
    pub fn start_node_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        debug_assert!(
            kind.is_node(),
            "start_node_at called with non-node kind {kind:?}"
        );
        let depth = self.stack.len().checked_sub(1).expect("no open node");
        assert!(
            depth == checkpoint.parent_depth,
            "start_node_at: checkpoint refers to a different parent frame \
             (checkpoint at depth {}, current depth {})",
            checkpoint.parent_depth,
            depth,
        );
        let frame = self.stack.last_mut().expect("no open node");
        assert!(
            checkpoint.children_at <= frame.children.len(),
            "start_node_at: checkpoint past current children",
        );
        let drained: Vec<GreenChild> = frame.children.drain(checkpoint.children_at..).collect();
        self.stack.push(Frame {
            kind,
            children: drained,
        });
    }

    /// Finish and return the single root node.
    ///
    /// # Panics
    ///
    /// Panics if there are unclosed open nodes, or if the number of top-level
    /// children is not exactly 1.
    pub fn finish(self) -> GreenNode {
        assert!(
            self.stack.is_empty(),
            "finish called with {} open nodes still on the stack",
            self.stack.len()
        );
        assert_eq!(
            self.finished.len(),
            1,
            "expected exactly one root node, got {}",
            self.finished.len()
        );
        match self.finished.into_iter().next().expect("checked above") {
            GreenChild::Node(n) => n,
            GreenChild::Token(_) => panic!("root must be a node, not a token"),
        }
    }

    fn intern(&mut self, node: GreenNode) -> GreenNode {
        let cache = match &mut self.cache {
            Some(c) => c,
            None => return node,
        };
        if let Some(existing) = cache.map.get(&node) {
            return existing.clone();
        }
        if cache.map.len() < cache.limit {
            cache.map.insert(node.clone(), node.clone());
        }
        node
    }
}

#[cfg(test)]
mod tests {
    use super::super::green::GreenChild;
    use super::*;

    #[test]
    fn roundtrip_simple() {
        let mut b = GreenNodeBuilder::new();
        b.start_node(SyntaxKind::Root);
        b.start_node(SyntaxKind::BinaryExpr);
        b.token(SyntaxKind::IntLit, "1");
        b.token(SyntaxKind::Plus, "+");
        b.token(SyntaxKind::IntLit, "2");
        b.finish_node();
        b.finish_node();
        let root = b.finish();
        assert_eq!(root.kind(), SyntaxKind::Root);
        assert_eq!(root.text(), "1+2");
    }

    #[test]
    fn checkpoint_wraps_left_recursive() {
        // Simulate left-recursive parsing of `1 + 2`.
        let mut b = GreenNodeBuilder::new();
        b.start_node(SyntaxKind::Root);
        let cp = b.checkpoint();
        b.token(SyntaxKind::IntLit, "1");
        b.start_node_at(cp, SyntaxKind::BinaryExpr);
        b.token(SyntaxKind::Plus, "+");
        b.token(SyntaxKind::IntLit, "2");
        b.finish_node(); // BinaryExpr
        b.finish_node(); // Root
        let root = b.finish();
        assert_eq!(root.text(), "1+2");
        let bin = root
            .children()
            .iter()
            .find_map(|c| c.as_node().cloned())
            .unwrap();
        assert_eq!(bin.kind(), SyntaxKind::BinaryExpr);
        let leaves: Vec<_> = bin
            .children()
            .iter()
            .filter_map(|c| c.as_token().cloned())
            .collect();
        assert_eq!(
            leaves.iter().map(|t| t.text()).collect::<Vec<_>>(),
            vec!["1", "+", "2"]
        );
    }

    #[test]
    fn cache_dedups_identical_subtrees() {
        let mut b = GreenNodeBuilder::with_cache(64);
        b.start_node(SyntaxKind::Root);

        // First `(1+2)` subtree.
        b.start_node(SyntaxKind::BinaryExpr);
        b.token(SyntaxKind::IntLit, "1");
        b.token(SyntaxKind::Plus, "+");
        b.token(SyntaxKind::IntLit, "2");
        b.finish_node();
        b.token(SyntaxKind::Semicolon, ";");
        // Second `(1+2)` subtree.
        b.start_node(SyntaxKind::BinaryExpr);
        b.token(SyntaxKind::IntLit, "1");
        b.token(SyntaxKind::Plus, "+");
        b.token(SyntaxKind::IntLit, "2");
        b.finish_node();
        b.finish_node();
        let root = b.finish();

        let mut bins = Vec::new();
        for c in root.children() {
            if let GreenChild::Node(n) = c {
                bins.push(n.clone());
            }
        }
        assert_eq!(bins.len(), 2);
        assert!(
            bins[0].ptr_eq(&bins[1]),
            "cache should share identical subtrees"
        );
    }

    #[test]
    fn cache_limit_is_respected() {
        let mut b = GreenNodeBuilder::with_cache(0);
        b.start_node(SyntaxKind::Root);
        b.start_node(SyntaxKind::BinaryExpr);
        b.token(SyntaxKind::IntLit, "1");
        b.token(SyntaxKind::Plus, "+");
        b.token(SyntaxKind::IntLit, "2");
        b.finish_node();
        b.start_node(SyntaxKind::BinaryExpr);
        b.token(SyntaxKind::IntLit, "1");
        b.token(SyntaxKind::Plus, "+");
        b.token(SyntaxKind::IntLit, "2");
        b.finish_node();
        b.finish_node();
        let root = b.finish();
        let bins: Vec<_> = root
            .children()
            .iter()
            .filter_map(|c| c.as_node().cloned())
            .collect();
        // With limit 0 the cache never inserts.
        assert!(!bins[0].ptr_eq(&bins[1]));
        // Content equality still holds.
        assert_eq!(bins[0], bins[1]);
    }

    #[test]
    #[should_panic(expected = "expected exactly one root node")]
    fn finish_requires_exactly_one_root() {
        let mut b = GreenNodeBuilder::new();
        b.start_node(SyntaxKind::Root);
        b.token(SyntaxKind::IntLit, "1");
        b.finish_node();
        b.start_node(SyntaxKind::Root);
        b.finish_node();
        let _ = b.finish();
    }

    #[test]
    #[should_panic(expected = "open nodes still on the stack")]
    fn finish_requires_balanced_stack() {
        let mut b = GreenNodeBuilder::new();
        b.start_node(SyntaxKind::Root);
        b.start_node(SyntaxKind::BinaryExpr);
        let _ = b.finish();
    }

    #[test]
    fn append_node_inlines_subtree() {
        // Build an isolated `5+5` BinaryExpr.
        let mut tmp = GreenNodeBuilder::new();
        tmp.start_node(SyntaxKind::Root);
        tmp.start_node(SyntaxKind::BinaryExpr);
        tmp.token(SyntaxKind::IntLit, "5");
        tmp.token(SyntaxKind::Plus, "+");
        tmp.token(SyntaxKind::IntLit, "5");
        tmp.finish_node();
        tmp.finish_node();
        let tmp_root = tmp.finish();
        let bin = tmp_root
            .children()
            .iter()
            .find_map(|c| c.as_node().cloned())
            .expect("bin");

        // Splice the subtree into a fresh root.
        let mut spliced = GreenNodeBuilder::new();
        spliced.start_node(SyntaxKind::Root);
        spliced.append_node(bin.clone());
        spliced.finish_node();
        let root = spliced.finish();

        assert_eq!(root.text(), "5+5");
        // `append_node` preserves the original Arc.
        let appended = root
            .children()
            .iter()
            .find_map(|c| c.as_node().cloned())
            .expect("appended");
        assert!(appended.ptr_eq(&bin));
    }
}
