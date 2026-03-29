//! Incremental constructor for green trees.
//!
//! [`GreenNodeBuilder`] is the only sanctioned way to build a [`GreenNode`]
//! from a stream of tokens and structural decisions. It exposes the standard
//! "stack of partial nodes" interface used by recursive-descent parsers:
//!
//! ```ignore
//! let mut b = GreenNodeBuilder::new();
//! b.start_node(SyntaxKind::Root);
//! b.start_node(SyntaxKind::BinaryExpr);
//! b.token(SyntaxKind::IntLit, "1");
//! b.token(SyntaxKind::Plus, "+");
//! b.token(SyntaxKind::IntLit, "2");
//! b.finish_node();
//! b.finish_node();
//! let root: GreenNode = b.finish();
//! ```
//!
//! The [`Checkpoint`] mechanism handles left-recursive shapes: record a
//! checkpoint *before* parsing what might turn out to be the LHS of a binary
//! expression; if it does, retroactively wrap the children since the
//! checkpoint with [`GreenNodeBuilder::start_node_at`].
//!
//! ## Subtree caching
//!
//! The builder optionally interns finished nodes by structural identity. When
//! [`GreenNodeBuilder::with_cache`] is used, completing a node whose
//! `(kind, children)` exactly matches an already-cached one returns the cached
//! `Arc` instead of allocating fresh. This is what enables the LSP's
//! whole-file-reparse + subtree-keyed-cache strategy: re-parses produce green
//! trees that share storage with their predecessors wherever the source did
//! not change.
//!
//! The cache is bounded by `with_cache(limit)` so a malicious or pathological
//! input cannot grow it without bound. Caching is opt-in because for short
//! one-shot parses (CLI `exec`) it is pure overhead.

use std::collections::HashMap;

use super::green::{GreenChild, GreenNode, GreenToken};
use super::kind::SyntaxKind;

/// Recorded position within a [`GreenNodeBuilder`]'s child stack. Use with
/// [`GreenNodeBuilder::start_node_at`].
///
/// Carries enough information to detect misuse (mismatched parent) in debug
/// builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint {
    /// Index of the currently-open node when the checkpoint was taken. Used
    /// to detect cross-frame checkpoint misuse.
    parent_depth: usize,
    /// Number of children already pushed into the open node when the
    /// checkpoint was taken.
    children_at: usize,
}

#[derive(Debug)]
struct Frame {
    kind: SyntaxKind,
    children: Vec<GreenChild>,
}

/// Builder state. Construct via [`GreenNodeBuilder::new`] or
/// [`GreenNodeBuilder::with_cache`].
#[derive(Debug)]
pub struct GreenNodeBuilder {
    stack: Vec<Frame>,
    /// Top-level frames that have been finished. After [`Self::finish`], this
    /// must contain exactly one element which becomes the root.
    finished: Vec<GreenChild>,
    /// Optional intern table for finished nodes. `None` disables caching.
    cache: Option<NodeCache>,
}

#[derive(Debug)]
struct NodeCache {
    map: HashMap<NodeKey, GreenNode>,
    /// Maximum number of entries before insertion is skipped. We never evict;
    /// the cache simply stops growing. For the LSP's per-document use this is
    /// fine: each document has its own builder, and the cache lives only as
    /// long as the parse.
    limit: usize,
}

/// Owned cache key. We could key by `&GreenNodeData` directly, but `GreenNode`
/// already implements structural [`Hash`] and [`Eq`], so we just key on the
/// node itself.
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

    /// Enable subtree-level interning. `limit` caps the number of distinct
    /// finished nodes that will be remembered.
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

    /// Open a new node of the given kind. Subsequent [`Self::token`] /
    /// [`Self::start_node`] / [`Self::finish_node`] calls operate on it.
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

    /// Append a token to the currently-open node. Tokens at the top level
    /// (no node open) are an error and are ignored in release builds.
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

    /// Append an already-built green node to the currently-open node. Useful
    /// for the LSP's incremental cache: a previously-parsed subtree can be
    /// spliced into the new tree without re-tokenizing it.
    pub fn append_node(&mut self, node: GreenNode) {
        let child = GreenChild::Node(node);
        match self.stack.last_mut() {
            Some(frame) => frame.children.push(child),
            None => self.finished.push(child),
        }
    }

    /// Close the currently-open node and add it to its parent (or to the
    /// "finished" list if there is no parent).
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

    /// Take a checkpoint at the current position so a wrapping node can be
    /// retroactively introduced.
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

    /// Wrap all children pushed since `checkpoint` into a new node of the
    /// given kind. The new node remains open until [`Self::finish_node`] is
    /// called for it.
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

    /// Finish building. Consumes the builder; returns the single root node.
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
        // Simulate `1 + 2` parsed left-recursively: parser sees `1`, then
        // discovers `+ 2` and retroactively wraps everything since the
        // checkpoint into a BinaryExpr.
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
        // With limit 0 the cache never inserts, so distinct allocations stay
        // distinct.
        assert!(!bins[0].ptr_eq(&bins[1]));
        // But they are still structurally equal.
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
        // Build an isolated `5+5` BinaryExpr we can splice in.
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

        // Splice the subtree into a fresh root via `append_node`.
        let mut spliced = GreenNodeBuilder::new();
        spliced.start_node(SyntaxKind::Root);
        spliced.append_node(bin.clone());
        spliced.finish_node();
        let root = spliced.finish();

        assert_eq!(root.text(), "5+5");
        // The node inside the spliced root must be Arc-identical to the one
        // we appended (no copy, no re-allocation).
        let appended = root
            .children()
            .iter()
            .find_map(|c| c.as_node().cloned())
            .expect("appended");
        assert!(appended.ptr_eq(&bin));
    }
}
