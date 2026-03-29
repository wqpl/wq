use std::io::{BufRead, Seek, Write};

/// handle for a streaming io source
pub(crate) trait BufReadSeek: BufRead + Seek {}
impl<T: BufRead + Seek> BufReadSeek for T {}

pub(crate) trait WriteSeek: Write + Seek {}
impl<T: Write + Seek> WriteSeek for T {}

pub struct StreamHandle {
    pub(crate) reader: Option<Box<dyn BufReadSeek + Send>>,
    pub(crate) writer: Option<Box<dyn WriteSeek + Send>>,
}

impl std::fmt::Debug for StreamHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // e.g. Some("std::io::BufReader<std::fs::File>")
        let reader_ty = self
            .reader
            .as_ref()
            .map(|r| std::any::type_name_of_val(&**r));
        let writer_ty = self
            .writer
            .as_ref()
            .map(|w| std::any::type_name_of_val(&**w));
        f.debug_struct("StreamHandle")
            .field("reader", &reader_ty)
            .field("writer", &writer_ty)
            .finish()
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        self.reader = None;
        self.writer = None;
    }
}
