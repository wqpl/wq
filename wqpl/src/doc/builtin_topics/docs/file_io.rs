use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const OPEN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Create a writable stream",
    code: "h:open[\"/tmp/wq-example.txt\";`write:T;`create:T;`truncate:T];close h",
    expectation: ExampleExpectation::NoRun("opens or creates a local file"),
}];

const PATH_EXISTS_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Check whether a path exists",
    code: "path_exists? \"/tmp/wq-example.txt\"",
    expectation: ExampleExpectation::NoRun("depends on the local filesystem"),
}];

const MKDIR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Create a directory tree",
    code: "mkdir \"/tmp/wq-output/nested\"",
    expectation: ExampleExpectation::NoRun("creates local directories"),
}];

const FILE_SIZE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Read a file's byte length",
    code: "file_size \"/tmp/wq-example.txt\"",
    expectation: ExampleExpectation::NoRun("depends on the local filesystem"),
}];

const WRITE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Write encoded bytes",
    code: "h:open[\"/tmp/wq-example.txt\";`write:T;`create:T;`truncate:T];write[h;encode[\"hello\\n\";\"utf-8\"]];close h",
    expectation: ExampleExpectation::NoRun("writes a local file"),
}];

const READ_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Read and decode a whole file",
    code: "h:open \"/tmp/wq-example.txt\";contents:decode[read h;\"utf-8\"];close h;contents",
    expectation: ExampleExpectation::NoRun("reads a local file"),
}];

const SEEK_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Read the last three bytes",
    code: "h:open \"/tmp/wq-example.bin\";seek[h;-3;`end];bytes:read h;close h;bytes",
    expectation: ExampleExpectation::NoRun("reads a local file"),
}];

const TELL_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Get the current byte position",
    code: "h:open \"/tmp/wq-example.bin\";read[h;4];position:tell h;close h;position",
    expectation: ExampleExpectation::NoRun("reads a local file"),
}];

const CLOSE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Close a stream handle",
    code: "h:open \"/tmp/wq-example.txt\";close h",
    expectation: ExampleExpectation::NoRun("closes a local stream"),
}];

pub(super) const OPEN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Open,
    summary: "Open a filesystem path as a stream.",
    details: "`open[path]` opens `path` read-only by default.
The named bool flags are `read`, `write`, `append`, `truncate`, `create`, and `create_new`.
Supplying any named flag disables the read-only default, so at least one of `read`, `write`, or `append` must then be true.
`truncate` requires writing, and `create` and `create_new` require writing or appending.
A stream has one byte position shared by reads, writes, `seek`, and `tell`.",
    examples: OPEN_EXAMPLES,
    related: &["read", "write", "close"],
};

pub(super) const PATH_EXISTS: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::PathExistsQ,
    summary: "Return true when a filesystem path exists.",
    details: "`path_exists?[path]` returns `T` for any filesystem entry, including a dangling symbolic link.
It returns `F` only when the path is not found.
Permission and other filesystem failures raise an IO error.",
    examples: PATH_EXISTS_EXAMPLES,
    related: &["mkdir", "file_size", "open"],
};

pub(super) const MKDIR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Mkdir,
    summary: "Create a directory and its missing parents.",
    details: "`mkdir[path]` creates the directory tree at `path` and returns unit.
It succeeds when the directory already exists and raises an IO error when the path cannot be created.",
    examples: MKDIR_EXAMPLES,
    related: &["path_exists?", "open"],
};

pub(super) const FILE_SIZE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::FileSize,
    summary: "Return a path's metadata byte length.",
    details: "`file_size[path]` reads filesystem metadata and returns its byte length as an int.
For regular files this is the file size in bytes.
Missing paths and metadata failures raise IO errors.",
    examples: FILE_SIZE_EXAMPLES,
    related: &["path_exists?", "read", "write"],
};

pub(super) const WRITE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Write,
    summary: "Write bytes to a writable stream.",
    details:
        "`write[stream;bytes]` accepts one int from 0 through 255 or a list of ints in that range.
It writes every byte, flushes the stream, and returns unit.
Encode strings explicitly with `encode` before writing them.",
    examples: WRITE_EXAMPLES,
    related: &["open", "read", "encode", "close"],
};

pub(super) const READ: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Read,
    summary: "Read bytes from a readable stream.",
    details: "`read[stream]` reads all remaining bytes.
`read[stream;count]` reads up to the non-negative int `count`; a count of zero does not advance the stream.
The result is a list of byte ints, or unit at EOF.
Decode a complete byte buffer explicitly with `decode`.
`read` does not perform incremental character decoding.",
    examples: READ_EXAMPLES,
    related: &["open", "write", "decode", "seek"],
};

pub(super) const SEEK: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Seek,
    summary: "Move a stream's byte position.",
    details: "`seek[stream;offset]` seeks from the start.
The optional origin tag is `start`, `current`, or `end`.
Start offsets must be non-negative; current and end offsets may be negative.
The new byte position is returned.",
    examples: SEEK_EXAMPLES,
    related: &["tell", "read", "write", "open"],
};

pub(super) const TELL: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Tell,
    summary: "Return a stream's current byte position.",
    details: "`tell[stream]` reports the single byte position used by reading, writing, and seeking.",
    examples: TELL_EXAMPLES,
    related: &["seek", "read", "write"],
};

pub(super) const CLOSE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Close,
    summary: "Close a stream.",
    details: "`close[stream]` flushes a writable stream, closes it, and returns unit.
Closing an already closed stream also succeeds.
Later read, write, seek, or tell operations raise an IO error.",
    examples: CLOSE_EXAMPLES,
    related: &["open", "read", "write"],
};
