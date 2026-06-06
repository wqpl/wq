#![cfg(not(target_arch = "wasm32"))]

use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;

use crate::builtins::{
    BuiltinEnum as BE, BuiltinFnArgs, check_arity, check_arity_named, type_mismatch,
};
use crate::value::stream::{BufReadSeek, StreamHandle, WriteSeek};
use crate::value::{Excerpt, IntoWqValue, Value, WqResult, into_wq_string};
use crate::wqerror::{WqError, WqErrorType};

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
        let mut o = OpenOptions::new();
        o.read(self.is_read())
            .write(self.is_write())
            .append(self.append)
            .truncate(self.truncate)
            .create(self.create)
            .create_new(self.create_new);
        o
    }
    fn is_read(&self) -> bool {
        self.read
    }
    fn is_write(&self) -> bool {
        self.write || self.append
    }
}

pub(super) fn open(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity_named(BE::Open, [1], &args, &["r", "w", "a", "t", "c", "cn"])?;
    let path = args[0]
        .to_rust_string_with_note()
        .map_err(|e| e.src(BE::Open))?;

    let flags = openflags_from_named(&args)?;
    let options = flags.into_openoptions();
    let file = options.open(&path).map_err(|e| io_err(e, BE::Open))?;
    let reader = if flags.is_read() {
        Some(Box::new(BufReader::new(
            file.try_clone().map_err(|e| io_err(e, BE::Open))?,
        )) as Box<dyn BufReadSeek + Send>)
    } else {
        None
    };
    let writer = if flags.is_write() {
        Some(Box::new(file.try_clone().map_err(|e| io_err(e, BE::Open))?)
            as Box<dyn WriteSeek + Send>)
    } else {
        None
    };
    let handle = StreamHandle { reader, writer };
    Ok(Value::stream(handle))
}

fn openflags_from_named(args: &BuiltinFnArgs) -> WqResult<OpenFlags> {
    fn get_bool(args: &BuiltinFnArgs, name: &str) -> Result<bool, WqError> {
        match args.named(name) {
            None => Ok(false),
            Some(v) if let Some(b) = v.try_to_rust_bool() => Ok(b),
            Some(other) => Err(WqError::new(WqErrorType::Domain).src(BE::Open).msg(format!(
                "named arg '{}' must be a bool, got {}",
                name,
                other.excerpt()
            ))),
        }
    }

    let read = get_bool(args, "r")?;
    let write = get_bool(args, "w")?;
    let append = get_bool(args, "a")?;
    let truncate = get_bool(args, "t")?;
    let create = get_bool(args, "c")?;
    let create_new = get_bool(args, "cn")?;

    // Default to read-only when no flags are provided
    if !read && !write && !append && !truncate && !create && !create_new {
        return Ok(OpenFlags {
            read: true,
            ..Default::default()
        });
    }

    // Must ask for at least one of read/write/append
    if !(read || write || append) {
        return Err(io_err(
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "expected at least one of: r, w, a",
            ),
            BE::Open,
        ));
    }
    // Truncate requires write permission (append counts as write)
    if truncate && !(write || append) {
        return Err(io_err(
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "flag t (truncate) requires w (write) or a (append)",
            ),
            BE::Open,
        ));
    }
    Ok(OpenFlags {
        read,
        write,
        append,
        truncate,
        create,
        create_new,
    })
}

pub(super) fn fexists(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::FexistsQ, [1], &args)?;
    let path = args[0]
        .to_rust_string_with_note()
        .map_err(|e| e.src(BE::FexistsQ).at_arg(0))?;
    Ok(Value::Bool(Path::new(&path).exists()))
}

pub(super) fn mkdir(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Mkdir, [1], &args)?;
    let path = args[0]
        .to_rust_string_with_note()
        .map_err(|e| e.src(BE::Mkdir).at_arg(0))?;
    fs::create_dir_all(&path).map_err(|e| io_err(e, BE::Mkdir))?;
    Ok(Value::unit())
}

pub(super) fn fsize(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Fsize, [1], &args)?;
    let path = args[0]
        .to_rust_string_with_note()
        .map_err(|e| e.src(BE::Fsize).at_arg(0))?;
    let meta = fs::metadata(&path).map_err(|e| io_err(e, BE::Fsize))?;
    Ok(meta.len().into_wq_value())
}

pub(super) fn fwrite(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Fwrite, [2], &args)?;
    let Value::Stream(rc) = &args[0] else {
        return Err(type_mismatch(BE::Fwrite, 0, "stream", &args[0]));
    };
    let mut handle = rc.lock().unwrap();
    let Some(w) = handle.writer.as_mut() else {
        return Err(stream_not_writeable(BE::Fwrite));
    };
    let bytes = args[1]
        .try_to_vec_u8()
        .map_err(|e| e.src(BE::Fwrite).at_arg(1))?;
    w.write_all(&bytes).map_err(|e| io_err(e, BE::Fwrite))?;
    w.flush().map_err(|e| io_err(e, BE::Fwrite))?;
    Ok(Value::unit())
}

pub(super) fn fwritet(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Fwritet, [2], &args)?;
    let Value::Stream(rc) = &args[0] else {
        return Err(type_mismatch(BE::Fwritet, 0, "stream", &args[0]));
    };
    let mut handle = rc.lock().unwrap();
    let Some(w) = handle.writer.as_mut() else {
        return Err(stream_not_writeable(BE::Fwritet));
    };
    let s = args[1]
        .to_rust_string_with_note()
        .map_err(|e| e.src(BE::Fwritet).at_arg(1))?;
    w.write_all(s.as_bytes())
        .map_err(|e| io_err(e, BE::Fwritet))?;
    w.flush().map_err(|e| io_err(e, BE::Fwritet))?;
    Ok(Value::unit())
}

fn fread_impl(src: BE, stream: &Value, length: Option<&Value>) -> WqResult<Option<Vec<u8>>> {
    let Value::Stream(rc) = stream else {
        return Err(type_mismatch(src, 0, "stream", stream));
    };
    let mut handle = rc.lock().unwrap();
    let Some(reader) = handle.reader.as_mut() else {
        return Err(stream_not_readable(src));
    };
    // length-mode
    if let Some(length) = length {
        let n = match length {
            Value::Int(n) if *n >= 0 => *n as usize,
            other => return Err(type_mismatch(src, 1, "positive int", other)),
        };
        let mut tmp = vec![0u8; n];
        let read = reader.read(&mut tmp).map_err(|e| io_err(e, src))?;
        if read == 0 {
            // length mode and hit EOF
            return Ok(None);
        }
        tmp.truncate(read);
        return Ok(Some(tmp));
    }
    // no length -> read entire remainder
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).map_err(|e| io_err(e, src))?;
    Ok(Some(buf))
}

fn read_trimmed_line(reader: &mut dyn BufReadSeek, src: BE) -> WqResult<Option<String>> {
    let mut line = String::new();
    let n = reader.read_line(&mut line).map_err(|e| io_err(e, src))?;
    if n == 0 {
        return Ok(None);
    }
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    Ok(Some(line))
}

pub(super) fn fread(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Fread, [1, 2], &args)?;
    match args.len() {
        1 => match fread_impl(BE::Fread, &args[0], None)? {
            None => Ok(Value::unit()),
            Some(buf) => Ok(Value::IntList(Arc::new(
                buf.into_iter().map(|b| b.into()).collect(),
            ))),
        },
        2 => match fread_impl(BE::Fread, &args[0], Some(&args[1]))? {
            None => Ok(Value::unit()),
            Some(buf) => Ok(Value::IntList(Arc::new(
                buf.into_iter().map(|b| b.into()).collect(),
            ))),
        },
        _ => unreachable!(),
    }
}

pub(super) fn freadt(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Freadt, [1, 2], &args)?;
    match args.len() {
        1 => match fread_impl(BE::Freadt, &args[0], None)? {
            None => Ok(Value::unit()),
            Some(buf) => {
                let s = String::from_utf8(buf).map_err(|e| io_err(e, BE::Freadt))?;
                Ok(into_wq_string(s))
            }
        },
        2 => match fread_impl(BE::Freadt, &args[0], Some(&args[1]))? {
            None => Ok(Value::unit()),
            Some(buf) => {
                let s = String::from_utf8(buf).map_err(|e| io_err(e, BE::Freadt))?;
                Ok(into_wq_string(s))
            }
        },
        _ => unreachable!(),
    }
}

pub(super) fn freadtln(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Freadtln, [1], &args)?;
    let Value::Stream(rc) = &args[0] else {
        return Err(type_mismatch(BE::Freadtln, 0, "stream", &args[0]));
    };
    let mut handle = rc.lock().unwrap();
    let Some(reader) = handle.reader.as_mut() else {
        return Err(stream_not_readable(BE::Freadtln));
    };
    match read_trimmed_line(&mut **reader, BE::Freadtln)? {
        Some(line) => Ok(into_wq_string(line)),
        None => Ok(Value::unit()),
    }
}

pub(super) fn freadtlns(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Freadtlns, [1], &args)?;
    let Value::Stream(rc) = &args[0] else {
        return Err(type_mismatch(BE::Freadtlns, 0, "stream", &args[0]));
    };
    let mut handle = rc.lock().unwrap();
    let Some(reader) = handle.reader.as_mut() else {
        return Err(stream_not_readable(BE::Freadtlns));
    };
    let mut lines = Vec::new();
    while let Some(line) = read_trimmed_line(&mut **reader, BE::Freadtlns)? {
        lines.push(into_wq_string(line));
    }
    Ok(Value::List(Arc::new(lines)))
}

pub(super) fn fseek(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Fseek, [2, 3], &args)?;
    let offset_arg = &args[1];
    let offset = if let Value::Int(n) = *offset_arg {
        n
    } else {
        return Err(type_mismatch(BE::Fseek, 1, "int", &args[1]));
    };
    let whence = if args.len() == 3 {
        let whence_arg = &args[2];
        if let Value::Int(w) = *whence_arg
            && [0, 1, 2].contains(&w)
        {
            w
        } else {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BE::Fseek)
                .msg("expected valid whence")
                .at_arg(2)
                .attach_note("fseek whence: 0=start, 1=current, 2=end")
                .got1(whence_arg));
        }
    } else {
        0
    };
    if offset < 0 && whence == 0 {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BE::Fseek)
            .msg("offset must be non-negative when whence is 0")
            .at_arg(1)
            .got1(offset_arg));
    }
    if let Value::Stream(rc) = &args[0] {
        let mut handle = rc.lock().unwrap();
        let seek_from = match whence {
            0 => SeekFrom::Start(offset as u64),
            1 => SeekFrom::Current(offset),
            2 => SeekFrom::End(offset),
            _ => unreachable!(),
        };
        if let Some(w) = handle.writer.as_mut() {
            let pos = w.seek(seek_from).map_err(|e| io_err(e, BE::Fseek))?;
            if let Some(r) = handle.reader.as_mut() {
                r.seek(SeekFrom::Start(pos))
                    .map_err(|e| io_err(e, BE::Fseek))?;
            }
            Ok(pos.into_wq_value())
        } else if let Some(r) = handle.reader.as_mut() {
            let pos = r.seek(seek_from).map_err(|e| io_err(e, BE::Fseek))?;
            Ok(pos.into_wq_value())
        } else {
            Err(stream_not_seekable(BE::Fseek))
        }
    } else {
        Err(type_mismatch(BE::Fseek, 0, "stream", &args[0]))
    }
}

pub(super) fn ftell(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Ftell, [1], &args)?;
    if let Value::Stream(rc) = &args[0] {
        let mut handle = rc.lock().unwrap();
        // Prefer writer if present.
        // Both sides are Seek, so just ask for stream_position.
        if let Some(w) = handle.writer.as_mut() {
            let pos = w.stream_position().map_err(|e| io_err(e, BE::Ftell))?;
            return Ok(pos.into_wq_value());
        }
        if let Some(r) = handle.reader.as_mut() {
            let pos = r.stream_position().map_err(|e| io_err(e, BE::Ftell))?;
            return Ok(pos.into_wq_value());
        }
        Err(stream_not_seekable(BE::Ftell))
    } else {
        Err(type_mismatch(BE::Ftell, 0, "stream", &args[0]))
    }
}

pub(super) fn fclose(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Fclose, [1], &args)?;
    if let Value::Stream(rc) = &args[0] {
        let mut handle = rc.lock().unwrap();
        handle.reader = None;
        handle.writer = None;
        Ok(Value::unit())
    } else {
        Err(type_mismatch(BE::Fclose, 0, "stream", &args[0]))
    }
}

fn io_err(e: impl Error, src: BE) -> WqError {
    WqError::new(WqErrorType::Io).src(src).msg(e)
}

fn stream_not_readable(src: BE) -> WqError {
    WqError::new(WqErrorType::Io)
        .src(src)
        .msg("this stream is not readable")
}

fn stream_not_writeable(src: BE) -> WqError {
    WqError::new(WqErrorType::Io)
        .src(src)
        .msg("this stream is not writeable")
}

fn stream_not_seekable(src: BE) -> WqError {
    WqError::new(WqErrorType::Io)
        .src(src)
        .msg("this stream is not seekable")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::value::into_wq_string;

    fn tmpfile() -> String {
        let name: u64 = rand::random();
        let path = std::env::temp_dir().join(format!("wq_io_{name}"));
        path.to_string_lossy().to_string()
    }

    // #[test]
    // fn write_read_seek() {
    //     let mut vm = Vm::new(vec![]);
    //     let path = tmpfile();
    //     let h = open(&mut vm, &[str_val(&path), str_val("w+")]).unwrap();
    //     fwritet(&mut vm, &[h.clone(), str_val("hi")]).unwrap();
    //     fseek(&mut vm, &[h.clone(), Value::Int(0)]).unwrap();
    //     let txt = freadt(&mut vm, std::slice::from_ref(&h)).unwrap();
    //     assert_eq!(txt, str_val("hi"));
    //     let size = fsize( &[str_val(&path)]).unwrap();
    //     assert_eq!(size, Value::Int(2));
    //     fclose( &[h]).unwrap();
    //     fs::remove_file(&path).unwrap();
    // }

    #[test]
    fn pexists_and_mkdir() {
        let path = tmpfile();
        assert_eq!(
            fexists(BuiltinFnArgs::from(into_wq_string(&path))).unwrap(),
            Value::Bool(false)
        );
        mkdir(BuiltinFnArgs::from(into_wq_string(&path))).unwrap();
        assert_eq!(
            fexists(BuiltinFnArgs::from(into_wq_string(&path))).unwrap(),
            Value::Bool(true)
        );
        fs::remove_dir_all(&path).unwrap();
    }

    #[test]
    fn freadtlns_reads_remaining_lines() {
        let path = tmpfile();
        fs::write(&path, "a\nb\r\nc\n").unwrap();

        let handle = open(BuiltinFnArgs::from(into_wq_string(&path))).unwrap();
        assert_eq!(
            freadtlns(BuiltinFnArgs::from(handle.clone())).unwrap(),
            Value::List(Arc::new(vec![
                into_wq_string("a"),
                into_wq_string("b"),
                into_wq_string("c")
            ]))
        );
        assert_eq!(
            freadtlns(BuiltinFnArgs::from(handle.clone())).unwrap(),
            Value::List(Arc::new(vec![]))
        );
        fclose(BuiltinFnArgs::from(handle)).unwrap();
        fs::remove_file(&path).unwrap();
    }

    // #[test]
    // fn open_missing_file_error() {
    //     let mut vm = Vm::new(vec![]);
    //     let res = open( &[str_val("/no/such/file"), str_val("r")]);
    //     assert!(matches!(res, Err(WqError::Io(_))));
    // }

    // #[test]
    // fn ftell_basic() {
    //     let mut vm = Vm::new(vec![]);
    //     let path = std::env::temp_dir().join("wq_ftell_test.txt");
    //     let sv = |s: &str| Value::List(s.chars().map(Value::Char).collect());

    //     let h = open(&mut vm, &[sv(path.to_str().unwrap()),
    // sv("w+")]).unwrap();     fwritet(&mut vm, &[h.clone(),
    // sv("hello")]).unwrap();     let pos1 = ftell(&mut vm,
    // std::slice::from_ref(&h)).unwrap();     assert_eq!(pos1,
    // Value::Int(5));

    //     fseek(&mut vm, &[h.clone(), Value::Int(2)]).unwrap();
    //     let pos2 = ftell(&mut vm, std::slice::from_ref(&h)).unwrap();
    //     assert_eq!(pos2, Value::Int(2));

    //     fclose(&mut vm, &[h]).unwrap();
    //     let _ = std::fs::remove_file(path);
    // }
}
