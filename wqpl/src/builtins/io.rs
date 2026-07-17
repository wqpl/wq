#![cfg(not(target_arch = "wasm32"))]

use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::sync::Arc;

use num_traits::ToPrimitive as _;

use crate::builtins::{
    BuiltinEnum as BE, BuiltinFnArgs, check_arity, check_arity_named, type_mismatch,
};
use crate::value::stream::StreamHandle;
use crate::value::{IntoWqValue, Value, WqResult, expected_bytes1, expected_string1};
use crate::wqerror::{Requirement, WqError, WqErrorType};

const OPEN_FLAGS: &[&str] = &[
    "read",
    "write",
    "append",
    "truncate",
    "create",
    "create_new",
];

#[derive(Clone, Copy, Debug, Default)]
struct OpenFlags {
    read: bool,
    write: bool,
    append: bool,
    truncate: bool,
    create: bool,
    create_new: bool,
}

impl OpenFlags {
    fn into_openoptions(self) -> OpenOptions {
        let mut options = OpenOptions::new();
        options
            .read(self.read)
            .write(self.write)
            .append(self.append)
            .truncate(self.truncate)
            .create(self.create)
            .create_new(self.create_new);
        options
    }

    fn writable(self) -> bool {
        self.write || self.append
    }
}

pub(super) fn open(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity_named(BE::Open, [1], &args, OPEN_FLAGS)?;
    let path = args[0]
        .try_to_rust_string()
        .ok_or_else(|| expected_string1(&args[0]).src(BE::Open).at_arg(0))?;
    let flags = open_flags_from_named(&args)?;
    let file = flags
        .into_openoptions()
        .open(&path)
        .map_err(|error| io_err(error, BE::Open))?;
    Ok(Value::stream(StreamHandle {
        file: Some(file),
        readable: flags.read,
        writable: flags.writable(),
    }))
}

fn open_flags_from_named(args: &BuiltinFnArgs) -> WqResult<OpenFlags> {
    fn get_bool(args: &BuiltinFnArgs, name: &str) -> WqResult<bool> {
        match args.named(name) {
            None => Ok(false),
            Some(value) if let Some(flag) = value.try_to_rust_bool() => Ok(flag),
            Some(value) => Err(WqError::new(WqErrorType::Domain)
                .src(BE::Open)
                .expected(Requirement::BOOL)
                .at_named_arg(name)
                .got1(value)),
        }
    }

    if !args.has_named() {
        return Ok(OpenFlags {
            read: true,
            ..OpenFlags::default()
        });
    }

    let flags = OpenFlags {
        read: get_bool(args, "read")?,
        write: get_bool(args, "write")?,
        append: get_bool(args, "append")?,
        truncate: get_bool(args, "truncate")?,
        create: get_bool(args, "create")?,
        create_new: get_bool(args, "create_new")?,
    };
    if !(flags.read || flags.writable()) {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BE::Open)
            .msg("expected at least one of `read, `write, or `append to be true"));
    }
    if flags.truncate && !flags.writable() {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BE::Open)
            .msg("`truncate requires `write or `append"));
    }
    if (flags.create || flags.create_new) && !flags.writable() {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BE::Open)
            .msg("`create and `create_new require `write or `append"));
    }
    Ok(flags)
}

pub(super) fn path_exists(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::PathExistsQ, [1], &args)?;
    let path = args[0]
        .try_to_rust_string()
        .ok_or_else(|| expected_string1(&args[0]).src(BE::PathExistsQ).at_arg(0))?;
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(Value::Bool(true)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Value::Bool(false)),
        Err(error) => Err(io_err(error, BE::PathExistsQ)),
    }
}

pub(super) fn mkdir(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Mkdir, [1], &args)?;
    let path = args[0]
        .try_to_rust_string()
        .ok_or_else(|| expected_string1(&args[0]).src(BE::Mkdir).at_arg(0))?;
    fs::create_dir_all(&path).map_err(|error| io_err(error, BE::Mkdir))?;
    Ok(Value::empty_list())
}

pub(super) fn file_size(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::FileSize, [1], &args)?;
    let path = args[0]
        .try_to_rust_string()
        .ok_or_else(|| expected_string1(&args[0]).src(BE::FileSize).at_arg(0))?;
    let metadata = fs::metadata(path).map_err(|error| io_err(error, BE::FileSize))?;
    Ok(metadata.len().into_wq_value())
}

pub(super) fn write(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Write, [2], &args)?;
    let Value::Stream(stream) = &args[0] else {
        return Err(type_mismatch(BE::Write, 0, Requirement::STREAM, &args[0]));
    };
    let bytes = args[1]
        .try_to_rust_vec_u8()
        .ok_or_else(|| expected_bytes1(&args[1]).src(BE::Write).at_arg(1))?;
    let mut handle = stream.lock().map_err(|_| stream_lock_error(BE::Write))?;
    if handle.file.is_none() {
        return Err(stream_closed(BE::Write));
    }
    if !handle.writable {
        return Err(stream_not_writable(BE::Write));
    }
    let file = handle
        .file
        .as_mut()
        .ok_or_else(|| stream_closed(BE::Write))?;
    file.write_all(&bytes)
        .map_err(|error| io_err(error, BE::Write))?;
    file.flush().map_err(|error| io_err(error, BE::Write))?;
    Ok(Value::empty_list())
}

pub(super) fn read(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Read, [1, 2], &args)?;
    let Value::Stream(stream) = &args[0] else {
        return Err(type_mismatch(BE::Read, 0, Requirement::STREAM, &args[0]));
    };
    let length = args.get(1).map(read_length).transpose()?;
    let mut handle = stream.lock().map_err(|_| stream_lock_error(BE::Read))?;
    if handle.file.is_none() {
        return Err(stream_closed(BE::Read));
    }
    if !handle.readable {
        return Err(stream_not_readable(BE::Read));
    }
    if length == Some(0) {
        return Ok(Value::empty_list());
    }
    let file = handle
        .file
        .as_mut()
        .ok_or_else(|| stream_closed(BE::Read))?;
    let mut bytes = Vec::new();
    match length {
        Some(length) => {
            file.take(length as u64)
                .read_to_end(&mut bytes)
                .map_err(|error| io_err(error, BE::Read))?;
        }
        None => {
            file.read_to_end(&mut bytes)
                .map_err(|error| io_err(error, BE::Read))?;
        }
    }
    if bytes.is_empty() {
        Ok(Value::empty_list())
    } else {
        Ok(Value::IntList(Arc::new(
            bytes.into_iter().map(i64::from).collect(),
        )))
    }
}

fn read_length(value: &Value) -> WqResult<usize> {
    let length = match value {
        Value::Int(length) if *length >= 0 => usize::try_from(*length).ok(),
        Value::BigInt(length) => length.to_usize(),
        _ => None,
    };
    length.ok_or_else(|| {
        type_mismatch(
            BE::Read,
            1,
            Requirement::non_negative(Requirement::INT),
            value,
        )
    })
}

#[derive(Clone, Copy)]
enum SeekOrigin {
    Start,
    Current,
    End,
}

pub(super) fn seek(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Seek, [2, 3], &args)?;
    let origin = args
        .get(2)
        .map(parse_seek_origin)
        .transpose()?
        .unwrap_or(SeekOrigin::Start);
    let seek_from = parse_seek_offset(&args[1], origin)?;
    let Value::Stream(stream) = &args[0] else {
        return Err(type_mismatch(BE::Seek, 0, Requirement::STREAM, &args[0]));
    };
    let mut handle = stream.lock().map_err(|_| stream_lock_error(BE::Seek))?;
    let file = handle
        .file
        .as_mut()
        .ok_or_else(|| stream_closed(BE::Seek))?;
    let position = file
        .seek(seek_from)
        .map_err(|error| io_err(error, BE::Seek))?;
    Ok(position.into_wq_value())
}

fn parse_seek_origin(value: &Value) -> WqResult<SeekOrigin> {
    match value {
        Value::Tag(origin) if origin.as_ref() == "start" => Ok(SeekOrigin::Start),
        Value::Tag(origin) if origin.as_ref() == "current" => Ok(SeekOrigin::Current),
        Value::Tag(origin) if origin.as_ref() == "end" => Ok(SeekOrigin::End),
        _ => Err(WqError::new(WqErrorType::Domain)
            .src(BE::Seek)
            .expected(Requirement::one_of([
                Requirement::literal("`start"),
                Requirement::literal("`current"),
                Requirement::literal("`end"),
            ]))
            .at_arg(2)
            .got1(value)),
    }
}

fn parse_seek_offset(value: &Value, origin: SeekOrigin) -> WqResult<SeekFrom> {
    let out_of_range = || {
        WqError::new(WqErrorType::Domain)
            .src(BE::Seek)
            .msg("offset is out of range for the selected seek origin")
            .at_arg(1)
            .got1(value)
    };
    match origin {
        SeekOrigin::Start => match value {
            Value::Int(offset) => u64::try_from(*offset)
                .map(SeekFrom::Start)
                .map_err(|_| out_of_range()),
            Value::BigInt(offset) => offset
                .to_u64()
                .map(SeekFrom::Start)
                .ok_or_else(out_of_range),
            _ => Err(type_mismatch(BE::Seek, 1, Requirement::INT, value)),
        },
        SeekOrigin::Current | SeekOrigin::End => {
            let offset = match value {
                Value::Int(offset) => Some(*offset),
                Value::BigInt(offset) => offset.to_i64(),
                _ => return Err(type_mismatch(BE::Seek, 1, Requirement::INT, value)),
            }
            .ok_or_else(out_of_range)?;
            Ok(match origin {
                SeekOrigin::Current => SeekFrom::Current(offset),
                SeekOrigin::End => SeekFrom::End(offset),
                SeekOrigin::Start => unreachable!(),
            })
        }
    }
}

pub(super) fn tell(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Tell, [1], &args)?;
    let Value::Stream(stream) = &args[0] else {
        return Err(type_mismatch(BE::Tell, 0, Requirement::STREAM, &args[0]));
    };
    let mut handle = stream.lock().map_err(|_| stream_lock_error(BE::Tell))?;
    let file = handle
        .file
        .as_mut()
        .ok_or_else(|| stream_closed(BE::Tell))?;
    let position = file
        .stream_position()
        .map_err(|error| io_err(error, BE::Tell))?;
    Ok(position.into_wq_value())
}

pub(super) fn close(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Close, [1], &args)?;
    let Value::Stream(stream) = &args[0] else {
        return Err(type_mismatch(BE::Close, 0, Requirement::STREAM, &args[0]));
    };
    let mut handle = stream.lock().map_err(|_| stream_lock_error(BE::Close))?;
    if handle.writable
        && let Some(file) = handle.file.as_mut()
    {
        file.flush().map_err(|error| io_err(error, BE::Close))?;
    }
    handle.file = None;
    Ok(Value::empty_list())
}

fn io_err(error: impl Error, source: BE) -> WqError {
    WqError::new(WqErrorType::Io).src(source).msg(error)
}

fn stream_lock_error(source: BE) -> WqError {
    WqError::new(WqErrorType::Io)
        .src(source)
        .msg("this stream's lock is poisoned")
}

fn stream_closed(source: BE) -> WqError {
    WqError::new(WqErrorType::Io)
        .src(source)
        .msg("this stream is closed")
}

fn stream_not_readable(source: BE) -> WqError {
    WqError::new(WqErrorType::Io)
        .src(source)
        .msg("this stream is not readable")
}

fn stream_not_writable(source: BE) -> WqError {
    WqError::new(WqErrorType::Io)
        .src(source)
        .msg("this stream is not writable")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use num_bigint::BigInt;

    use super::*;
    use crate::value::into_wq_string;

    fn tmp_path() -> String {
        let name: u64 = rand::random();
        std::env::temp_dir()
            .join(format!("wq_io_{name}"))
            .to_string_lossy()
            .to_string()
    }

    fn open_named(path: &str, flags: &[(&str, bool)]) -> Value {
        open(BuiltinFnArgs::with_named(
            smallvec::smallvec![into_wq_string(path)],
            flags
                .iter()
                .map(|(name, value)| (Arc::from(*name), Value::Bool(*value)))
                .collect(),
        ))
        .expect("stream should open")
    }

    #[test]
    fn path_exists_and_mkdir() {
        let path = tmp_path();
        assert_eq!(
            path_exists(BuiltinFnArgs::from(into_wq_string(&path)))
                .expect("missing path check should succeed"),
            Value::Bool(false)
        );
        mkdir(BuiltinFnArgs::from(into_wq_string(&path))).expect("directory should be created");
        assert_eq!(
            path_exists(BuiltinFnArgs::from(into_wq_string(&path)))
                .expect("created path check should succeed"),
            Value::Bool(true)
        );
        fs::remove_dir_all(path).expect("test directory should be removed");
    }

    #[cfg(unix)]
    #[test]
    fn path_exists_recognizes_a_dangling_symlink() {
        use std::os::unix::fs::symlink;

        let path = tmp_path();
        symlink(format!("{path}_missing"), &path).expect("test symlink should be created");
        assert_eq!(
            path_exists(BuiltinFnArgs::from(into_wq_string(&path)))
                .expect("symlink check should succeed"),
            Value::Bool(true)
        );
        fs::remove_file(path).expect("test symlink should be removed");
    }

    #[test]
    fn explicit_false_open_flags_do_not_enable_default_reading() {
        let path = tmp_path();
        fs::write(&path, b"contents").expect("test file should be written");
        let args = BuiltinFnArgs::with_named(
            smallvec::smallvec![into_wq_string(&path)],
            vec![(Arc::from("read"), Value::Bool(false))],
        );

        let error = open(args).expect_err("an explicit false flag must suppress defaults");
        assert!(
            error
                .msg
                .as_deref()
                .is_some_and(|message| message.contains("at least one"))
        );
        fs::remove_file(path).expect("test file should be removed");
    }

    #[test]
    fn readable_writable_stream_has_one_logical_cursor() {
        let path = tmp_path();
        fs::write(&path, b"abcdef").expect("test file should be written");
        let handle = open_named(&path, &[("read", true), ("write", true)]);

        assert_eq!(
            read(BuiltinFnArgs::from(smallvec::smallvec![
                handle.clone(),
                Value::Int(1),
            ]))
            .expect("one byte should be read"),
            Value::IntList(Arc::new(vec![i64::from(b'a')]))
        );
        assert_eq!(
            tell(BuiltinFnArgs::from(handle.clone())).expect("position should be reported"),
            Value::Int(1)
        );
        write(BuiltinFnArgs::from(smallvec::smallvec![
            handle.clone(),
            Value::IntList(Arc::new(vec![i64::from(b'X')])),
        ]))
        .expect("one byte should be written");
        seek(BuiltinFnArgs::from(smallvec::smallvec![
            handle.clone(),
            Value::Int(0),
        ]))
        .expect("stream should seek to its start");
        assert_eq!(
            read(BuiltinFnArgs::from(handle.clone())).expect("remaining bytes should be read"),
            Value::IntList(Arc::new(
                b"aXcdef".iter().map(|byte| i64::from(*byte)).collect()
            ))
        );

        close(BuiltinFnArgs::from(handle)).expect("stream should close");
        fs::remove_file(path).expect("test file should be removed");
    }

    #[test]
    fn bounded_read_handles_zero_eof_and_cursor_position() {
        let path = tmp_path();
        fs::write(&path, b"abc").expect("test file should be written");
        let handle =
            open(BuiltinFnArgs::from(into_wq_string(&path))).expect("test file should open");

        assert!(
            read(BuiltinFnArgs::from(smallvec::smallvec![
                handle.clone(),
                Value::Int(0),
            ]))
            .expect("zero-count read should succeed")
            .is_unit()
        );
        assert_eq!(
            tell(BuiltinFnArgs::from(handle.clone())).expect("position should be reported"),
            Value::Int(0)
        );
        assert_eq!(
            read(BuiltinFnArgs::from(smallvec::smallvec![
                handle.clone(),
                Value::BigInt(Arc::new(BigInt::from(20))),
            ]))
            .expect("bounded read should succeed"),
            Value::IntList(Arc::new(vec![97, 98, 99]))
        );
        assert!(
            read(BuiltinFnArgs::from(handle.clone()))
                .expect("EOF read should succeed")
                .is_unit()
        );

        close(BuiltinFnArgs::from(handle)).expect("stream should close");
        fs::remove_file(path).expect("test file should be removed");
    }

    #[test]
    fn seek_uses_tag_origins() {
        let path = tmp_path();
        fs::write(&path, b"abcdef").expect("test file should be written");
        let handle =
            open(BuiltinFnArgs::from(into_wq_string(&path))).expect("test file should open");

        assert_eq!(
            seek(BuiltinFnArgs::from(smallvec::smallvec![
                handle.clone(),
                Value::Int(-2),
                Value::Tag(Arc::from("end")),
            ]))
            .expect("end-relative seek should succeed"),
            Value::Int(4)
        );
        assert_eq!(
            seek(BuiltinFnArgs::from(smallvec::smallvec![
                handle.clone(),
                Value::BigInt(Arc::new(BigInt::from(1))),
                Value::Tag(Arc::from("current")),
            ]))
            .expect("current-relative bigint seek should succeed"),
            Value::Int(5)
        );
        let error = seek(BuiltinFnArgs::from(smallvec::smallvec![
            handle.clone(),
            Value::Int(0),
            Value::Int(0),
        ]))
        .expect_err("numeric seek origins should be rejected");
        assert_eq!(error.err_type, WqErrorType::Domain);

        close(BuiltinFnArgs::from(handle)).expect("stream should close");
        fs::remove_file(path).expect("test file should be removed");
    }

    #[test]
    fn close_is_idempotent_and_later_operations_fail() {
        let path = tmp_path();
        fs::write(&path, b"abc").expect("test file should be written");
        let handle =
            open(BuiltinFnArgs::from(into_wq_string(&path))).expect("test file should open");

        close(BuiltinFnArgs::from(handle.clone())).expect("stream should close");
        close(BuiltinFnArgs::from(handle.clone())).expect("second close should succeed");
        let error = read(BuiltinFnArgs::from(smallvec::smallvec![
            handle,
            Value::Int(0),
        ]))
        .expect_err("even a zero-count read on a closed stream should fail");
        assert_eq!(error.msg.as_deref(), Some("this stream is closed"));
        fs::remove_file(path).expect("test file should be removed");
    }
}
