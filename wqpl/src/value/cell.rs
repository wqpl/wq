use std::sync::{Arc, Mutex};

use crate::value::Value;

/// Heap cell shared between frames and closures for captured locals.
pub(crate) type ValueCell = Arc<Mutex<Value>>;

/// Shared empty capture list for non-closure functions.
pub(crate) fn empty_cells() -> Arc<[ValueCell]> {
    use std::sync::LazyLock;
    static EMPTY: LazyLock<Arc<[ValueCell]>> = LazyLock::new(|| Arc::from(Vec::new()));
    EMPTY.clone()
}
