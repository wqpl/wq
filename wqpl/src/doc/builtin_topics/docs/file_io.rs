use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const OPEN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Create a writable stream",
    code: "h:open[\"/tmp/wq-example.txt\";`w:T;`c:T;`t:T];fclose h",
    expectation: ExampleExpectation::NoRun("opens or creates a local file"),
}];

const FEXISTS_Q_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Check whether a path exists",
    code: "fexists? \"/tmp/wq-example.txt\"",
    expectation: ExampleExpectation::NoRun("depends on the local filesystem"),
}];

const MKDIR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Create a directory tree",
    code: "mkdir \"/tmp/wq-output/nested\"",
    expectation: ExampleExpectation::NoRun("creates local directories"),
}];

const FSIZE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Read a file's byte length",
    code: "fsize \"/tmp/wq-example.txt\"",
    expectation: ExampleExpectation::NoRun("depends on the local filesystem"),
}];

const FWRITE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Write raw bytes",
    code: "h:open[\"/tmp/wq-example.bin\";`w:T;`c:T;`t:T];fwrite[h;(65;66;67)];fclose h",
    expectation: ExampleExpectation::NoRun("writes a local file"),
}];

const FWRITET_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Write UTF-8 text",
    code: "h:open[\"/tmp/wq-example.txt\";`w:T;`c:T;`t:T];fwritet[h;\"hello\\n\"];fclose h",
    expectation: ExampleExpectation::NoRun("writes a local file"),
}];

const FREAD_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Read bytes from a stream",
    code: "h:open \"/tmp/wq-example.bin\";bytes:fread[h;3];fclose h;bytes",
    expectation: ExampleExpectation::NoRun("reads a local file"),
}];

const FREADT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Read text from a stream",
    code: "h:open \"/tmp/wq-example.txt\";text:freadt h;fclose h;text",
    expectation: ExampleExpectation::NoRun("reads a local file"),
}];

const FREADTLN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Read one line",
    code: "h:open \"/tmp/wq-example.txt\";line:freadtln h;fclose h;line",
    expectation: ExampleExpectation::NoRun("reads a local file"),
}];

const FREADTLNS_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Read all remaining lines",
    code: "h:open \"/tmp/wq-example.txt\";lines:freadtlns h;fclose h;lines",
    expectation: ExampleExpectation::NoRun("reads a local file"),
}];

const FSEEK_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Seek before reading",
    code: "h:open \"/tmp/wq-example.txt\";fseek[h;2];chunk:freadt[h;3];fclose h;chunk",
    expectation: ExampleExpectation::NoRun("reads a local file"),
}];

const FTELL_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Get the current byte position",
    code: "h:open \"/tmp/wq-example.txt\";freadt[h;4];pos:ftell h;fclose h;pos",
    expectation: ExampleExpectation::NoRun("reads a local file"),
}];

const FCLOSE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Close a stream handle",
    code: "h:open \"/tmp/wq-example.txt\";fclose h",
    expectation: ExampleExpectation::NoRun("closes a local stream"),
}];

pub(super) const OPEN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Open,
    summary: "Open a filesystem path as a stream.",
    details: "`open[path]` opens `path` read-only by default and returns a stream. Named flags are booleans: `r` enables reading, `w` enables writing, `a` appends, `t` truncates, `c` creates missing files, and `cn` creates only when the path is new. If any flag is supplied, at least one of `r`, `w`, or `a` must be true. Truncation requires `w` or `a`; `w` alone does not truncate. Streams can carry a reader, a writer, or both.",
    examples: OPEN_EXAMPLES,
    related: &["freadt", "fwritet", "fclose"],
};

pub(super) const FEXISTS_Q: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::FexistsQ,
    summary: "Return true when a path exists.",
    details: "`fexists?[path]` converts `path` from string-like data and checks whether the local filesystem has any entry there. It returns `T` for files, directories, and other existing filesystem nodes.",
    examples: FEXISTS_Q_EXAMPLES,
    related: &["mkdir", "fsize", "open"],
};

pub(super) const MKDIR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Mkdir,
    summary: "Create a directory and its missing parents.",
    details: "`mkdir[path]` creates the directory tree at `path`, like `mkdir -p`, and returns unit. It succeeds when the directory already exists and raises an IO error when the path cannot be created.",
    examples: MKDIR_EXAMPLES,
    related: &["fexists?", "open"],
};

pub(super) const FSIZE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Fsize,
    summary: "Return a path's metadata byte length.",
    details: "`fsize[path]` reads filesystem metadata and returns its byte length as an integer. For regular files this is the file size in bytes; missing paths or metadata errors raise IO errors.",
    examples: FSIZE_EXAMPLES,
    related: &["fexists?", "fread", "fwrite"],
};

pub(super) const FWRITE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Fwrite,
    summary: "Write bytes to a writable stream.",
    details: "`fwrite[stream;bytes]` requires a stream opened with `w` or `a`. `bytes` may be one int or bigint in `0..=255`, an int list, or a list of ints/bigints in that range. The bytes are written, the stream is flushed, and the result is unit.",
    examples: FWRITE_EXAMPLES,
    related: &["open", "fread", "fwritet", "fclose"],
};

pub(super) const FWRITET: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Fwritet,
    summary: "Write text to a writable stream.",
    details: "`fwritet[stream;text]` requires a stream opened with `w` or `a`. `text` is converted from string-like data, written as UTF-8 bytes, flushed, and the result is unit.",
    examples: FWRITET_EXAMPLES,
    related: &["open", "freadt", "fwrite", "fclose"],
};

pub(super) const FREAD: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Fread,
    summary: "Read raw bytes from a readable stream.",
    details: "`fread[stream]` reads all remaining bytes and returns them as an int list. `fread[stream;len]` reads up to `len` bytes, where `len` is a non-negative int. In length mode, EOF returns unit; without a length, EOF can return an empty byte list.",
    examples: FREAD_EXAMPLES,
    related: &["open", "fwrite", "freadt", "fseek"],
};

pub(super) const FREADT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Freadt,
    summary: "Read UTF-8 text from a readable stream.",
    details: "`freadt[stream]` reads all remaining bytes, validates them as UTF-8, and returns a string. `freadt[stream;len]` reads up to `len` bytes first. In length mode, EOF returns unit; invalid UTF-8 raises an IO error.",
    examples: FREADT_EXAMPLES,
    related: &["open", "fwritet", "fread", "freadtln"],
};

pub(super) const FREADTLN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Freadtln,
    summary: "Read one text line from a stream.",
    details: "`freadtln[stream]` reads one UTF-8 line, strips a trailing `\\n` or `\\r\\n`, and returns the line as a string. End-of-file returns unit.",
    examples: FREADTLN_EXAMPLES,
    related: &["freadtlns", "freadt", "open"],
};

pub(super) const FREADTLNS: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Freadtlns,
    summary: "Read all remaining text lines from a stream.",
    details: "`freadtlns[stream]` repeatedly reads UTF-8 lines until EOF, strips trailing line endings, and returns a list of strings. If no lines remain, it returns an empty list.",
    examples: FREADTLNS_EXAMPLES,
    related: &["freadtln", "freadt", "open"],
};

pub(super) const FSEEK: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Fseek,
    summary: "Move a stream's byte position.",
    details: "`fseek[stream;offset]` seeks to byte `offset` from the start and returns the new byte position. `fseek[stream;offset;whence]` uses `whence` 0 for start, 1 for current position, and 2 for end. Negative offsets are allowed only with `whence` 1 or 2. When a stream has both reader and writer sides, seeking keeps them at the same position.",
    examples: FSEEK_EXAMPLES,
    related: &["ftell", "fread", "freadt", "open"],
};

pub(super) const FTELL: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Ftell,
    summary: "Return a stream's current byte position.",
    details: "`ftell[stream]` returns the current byte position. If a stream has both reader and writer sides, the writer position is reported.",
    examples: FTELL_EXAMPLES,
    related: &["fseek", "fread", "fwritet"],
};

pub(super) const FCLOSE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Fclose,
    summary: "Close a stream.",
    details: "`fclose[stream]` drops the stream's reader and writer handles and returns unit. Later read, write, seek, or tell operations on that stream fail because the handle no longer has an active side.",
    examples: FCLOSE_EXAMPLES,
    related: &["open", "fread", "fwritet"],
};
