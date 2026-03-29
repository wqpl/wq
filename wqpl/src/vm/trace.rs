use crate::wqdb::data::Span;

/// One probe captured while a `@d` expression is being evaluated.
///
/// Records are appended in execution (post-order) order.  The flush at the
/// closing `Debug` instruction rebuilds the parent/child structure using
/// `call_depth` (cross-frame relation) and span containment (intra-frame
/// nesting).
#[derive(Debug, Clone)]
pub(crate) struct TraceRecord {
    pub(crate) span: Span,
    pub(crate) value_excerpt: String,
    pub(crate) type_name: &'static str,
    pub(crate) call_depth: u32,
}
