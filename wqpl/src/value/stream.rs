use std::fs::File;

/// Handle for a streaming I/O source with one logical cursor.
pub struct StreamHandle {
    pub(crate) file: Option<File>,
    pub(crate) readable: bool,
    pub(crate) writable: bool,
}

impl std::fmt::Debug for StreamHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamHandle")
            .field("open", &self.file.is_some())
            .field("readable", &self.readable)
            .field("writable", &self.writable)
            .finish()
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        self.file = None;
    }
}
