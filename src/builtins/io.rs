use super::arity_error;
use crate::{
    builtins::values_to_strings,
    value::valuei::{BufReadSeek, StreamHandle, Value, WqError, WqResult, WriteSeek},
};

use once_cell::sync::Lazy;
use rustyline::{DefaultEditor, error::ReadlineError};
use std::sync::Mutex;

use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
};

pub static RUSTYLINE: Lazy<Mutex<Option<DefaultEditor>>> = Lazy::new(|| Mutex::new(None));

fn value_to_bytes(v: &Value) -> WqResult<Vec<u8>> {
    match v {
        Value::IntList(arr) => Ok(arr.iter().map(|&n| n as u8).collect()),
        Value::List(items) => items
            .iter()
            .map(|x| {
                if let Value::Int(i) = x {
                    Ok(*i as u8)
                } else {
                    Err(WqError::TypeError("invalid byte".into()))
                }
            })
            .collect(),
        Value::Int(n) => Ok(vec![*n as u8]),
        _ => Err(WqError::TypeError("invalid byte".into())),
    }
}

fn value_to_string(v: &Value) -> WqResult<String> {
    Ok(values_to_strings(&[v.clone()])?.pop().unwrap())
}

pub fn open(args: &[Value]) -> WqResult<Value> {
    if args.is_empty() || args.len() > 2 {
        return Err(arity_error("open", "1 or 2 arguments", args.len()));
    }
    let path = value_to_string(&args[0])?;
    let mode = if args.len() == 2 {
        value_to_string(&args[1])?
    } else {
        "r".to_string()
    };

    let mut options = OpenOptions::new();
    let mut read = false;
    let mut write = false;
    match mode.as_str() {
        "r" => {
            options.read(true);
            read = true;
        }
        "r+" => {
            options.read(true).write(true);
            read = true;
            write = true;
        }
        "w" => {
            options.write(true).create(true).truncate(true);
            write = true;
        }
        "w+" => {
            options.read(true).write(true).create(true).truncate(true);
            read = true;
            write = true;
        }
        "a" => {
            options.write(true).create(true).append(true);
            write = true;
        }
        "a+" => {
            options.read(true).write(true).create(true).append(true);
            read = true;
            write = true;
        }
        _ => return Err(WqError::DomainError("invalid open mode".into())),
    }

    let file = options
        .open(&path)
        .map_err(|e| WqError::IoError(e.to_string()))?;

    let reader = if read {
        Some(Box::new(BufReader::new(
            file.try_clone()
                .map_err(|e| WqError::IoError(e.to_string()))?,
        )) as Box<dyn BufReadSeek + Send>)
    } else {
        None
    };

    let writer = if write {
        Some(Box::new(
            file.try_clone()
                .map_err(|e| WqError::IoError(e.to_string()))?,
        ) as Box<dyn WriteSeek + Send>)
    } else {
        None
    };

    let handle = StreamHandle {
        reader,
        writer,
        child: None,
    };
    Ok(Value::stream(handle))
}

pub fn pexists(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("exists?", "1 argument", args.len()));
    }
    let path = value_to_string(&args[0])?;
    Ok(Value::Bool(Path::new(&path).exists()))
}

pub fn mkdir(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("mkdir", "1 argument", args.len()));
    }
    let path = value_to_string(&args[0])?;
    fs::create_dir_all(&path).map_err(|e| WqError::IoError(e.to_string()))?;
    Ok(Value::Null)
}

pub fn fsize(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("fsize", "1 argument", args.len()));
    }
    let path = value_to_string(&args[0])?;
    let meta = fs::metadata(&path).map_err(|e| WqError::IoError(e.to_string()))?;
    Ok(Value::Int(meta.len() as i64))
}

pub fn fwrite(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("fwrite", "2 arguments", args.len()));
    }
    if let Value::Stream(rc) = &args[0] {
        let mut handle = rc.lock().unwrap();
        if let Some(w) = handle.writer.as_mut() {
            let bytes = value_to_bytes(&args[1])?;
            w.write_all(&bytes)
                .map_err(|e| WqError::IoError(e.to_string()))?;
            w.flush().map_err(|e| WqError::IoError(e.to_string()))?;
            Ok(Value::Null)
        } else {
            Err(WqError::IoError("stream not writable".into()))
        }
    } else {
        Err(WqError::TypeError("fwrite expects a stream".into()))
    }
}

pub fn fwritet(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("fwritet", "2 arguments", args.len()));
    }
    let s = value_to_string(&args[1])?;
    fwrite(&[
        args[0].clone(),
        Value::IntList(s.into_bytes().into_iter().map(|b| b as i64).collect()),
    ])
}

pub fn fread(args: &[Value]) -> WqResult<Value> {
    if args.is_empty() || args.len() > 2 {
        return Err(arity_error("fread", "1 or 2 arguments", args.len()));
    }
    if let Value::Stream(rc) = &args[0] {
        let mut handle = rc.lock().unwrap();
        if let Some(reader) = handle.reader.as_mut() {
            let mut buf = Vec::new();
            if args.len() == 2 {
                if let Value::Int(n) = args[1] {
                    if n < 0 {
                        return Err(WqError::DomainError(
                            "fread length must be non-negative".into(),
                        ));
                    }
                    let mut tmp = vec![0u8; n as usize];
                    let read = reader
                        .read(&mut tmp)
                        .map_err(|e| WqError::IoError(e.to_string()))?;
                    if read == 0 {
                        // length mode and hit EOF => mimic line-read behavior: return null
                        return Ok(Value::Null);
                    }
                    buf.extend_from_slice(&tmp[..read]);
                } else {
                    return Err(WqError::TypeError("invalid fread length".into()));
                }
            } else {
                reader
                    .read_to_end(&mut buf)
                    .map_err(|e| WqError::IoError(e.to_string()))?;
            }
            Ok(Value::IntList(buf.into_iter().map(|b| b as i64).collect()))
        } else {
            Err(WqError::IoError("stream not readable".into()))
        }
    } else {
        Err(WqError::TypeError("fread expects a stream".into()))
    }
}

pub fn freadt(args: &[Value]) -> WqResult<Value> {
    let bytes = fread(args)?;
    match bytes {
        Value::Null => Ok(Value::Null),
        Value::IntList(b) => {
            let s = String::from_utf8(b.into_iter().map(|i| i as u8).collect())
                .map_err(|e| WqError::IoError(e.to_string()))?;
            Ok(Value::List(s.chars().map(Value::Char).collect()))
        }
        _ => unreachable!(),
    }
}

pub fn freadtln(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("freadtln", "1 argument", args.len()));
    }
    if let Value::Stream(rc) = &args[0] {
        let mut handle = rc.lock().unwrap();
        if let Some(reader) = handle.reader.as_mut() {
            let mut line = String::new();
            let n = reader
                .read_line(&mut line)
                .map_err(|e| WqError::IoError(e.to_string()))?;
            if n == 0 {
                Ok(Value::Null)
            } else {
                if line.ends_with('\n') {
                    line.pop();
                    if line.ends_with('\r') {
                        line.pop();
                    }
                }
                Ok(Value::List(line.chars().map(Value::Char).collect()))
            }
        } else {
            Err(WqError::IoError("stream not readable".into()))
        }
    } else {
        Err(WqError::TypeError("freadtln expects a stream".into()))
    }
}

pub fn fseek(args: &[Value]) -> WqResult<Value> {
    if args.len() < 2 || args.len() > 3 {
        return Err(arity_error("fseek", "2 or 3 arguments", args.len()));
    }
    let offset = if let Value::Int(n) = args[1] {
        n
    } else {
        return Err(WqError::TypeError("offset must be int".into()));
    };
    let whence = if args.len() == 3 {
        if let Value::Int(w) = args[2] {
            w
        } else {
            return Err(WqError::TypeError("whence".into()));
        }
    } else {
        0
    };
    if let Value::Stream(rc) = &args[0] {
        let mut handle = rc.lock().unwrap();
        let seek_from = match whence {
            0 => SeekFrom::Start(offset as u64),
            1 => SeekFrom::Current(offset),
            2 => SeekFrom::End(offset),
            _ => return Err(WqError::DomainError("invalid whence".into())),
        };

        if let Some(w) = handle.writer.as_mut() {
            let pos = w
                .seek(seek_from)
                .map_err(|e| WqError::IoError(e.to_string()))?;
            if let Some(r) = handle.reader.as_mut() {
                r.seek(SeekFrom::Start(pos))
                    .map_err(|e| WqError::IoError(e.to_string()))?;
            }
            Ok(Value::Int(pos as i64))
        } else if let Some(r) = handle.reader.as_mut() {
            let pos = r
                .seek(seek_from)
                .map_err(|e| WqError::IoError(e.to_string()))?;
            Ok(Value::Int(pos as i64))
        } else {
            Err(WqError::IoError("stream not seekable".into()))
        }
    } else {
        Err(WqError::TypeError("fseek expects a stream".into()))
    }
}

pub fn fclose(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("fclose", "1 argument", args.len()));
    }
    if let Value::Stream(rc) = &args[0] {
        let mut handle = rc.lock().unwrap();
        handle.reader = None;
        handle.writer = None;
        handle.child = None;
        Ok(Value::Null)
    } else {
        Err(WqError::TypeError("fclose expects a stream".into()))
    }
}

pub fn decode(args: &[Value]) -> WqResult<Value> {
    if args.len() < 2 || args.len() > 3 {
        return Err(arity_error("decode", "2 or 3 arguments", args.len()));
    }
    let bytes = value_to_bytes(&args[0])?;
    let codec = value_to_string(&args[1])?;
    let mode = if args.len() == 3 {
        value_to_string(&args[2])?
    } else {
        "s".to_string()
    };
    if codec.to_lowercase() != "utf-8" {
        return Err(WqError::DomainError("unsupported codec".into()));
    }
    let s = match mode.as_str() {
        "s" => String::from_utf8(bytes).map_err(|e| WqError::RuntimeError(e.to_string()))?,
        "i" => String::from_utf8_lossy(&bytes).replace('\u{FFFD}', ""),
        "r" => String::from_utf8_lossy(&bytes).into_owned(),
        _ => return Err(WqError::DomainError("invalid mode".into())),
    };
    Ok(Value::List(s.chars().map(Value::Char).collect()))
}

pub fn encode(args: &[Value]) -> WqResult<Value> {
    if args.len() < 2 || args.len() > 3 {
        return Err(arity_error("encode", "2 or 3 arguments", args.len()));
    }
    let s = value_to_string(&args[0])?;
    let codec = value_to_string(&args[1])?;
    if codec.to_lowercase() != "utf-8" {
        return Err(WqError::DomainError("unsupported codec".into()));
    }
    let bytes = s.into_bytes();
    Ok(Value::IntList(
        bytes.into_iter().map(|b| b as i64).collect(),
    ))
}

pub fn input(args: &[Value]) -> WqResult<Value> {
    if args.len() > 1 {
        return Err(arity_error("input", "0 or 1 argument", args.len()));
    }

    let prompt = if args.len() == 1 {
        value_to_string(&args[0])?
    } else {
        String::new()
    };

    let mut rl_guard = RUSTYLINE.lock().unwrap();
    if rl_guard.is_none() {
        *rl_guard = Some(DefaultEditor::new().map_err(|e| WqError::IoError(e.to_string()))?);
    }
    match rl_guard.as_mut().unwrap().readline(&prompt) {
        Ok(line) => Ok(Value::List(line.chars().map(Value::Char).collect())),
        Err(ReadlineError::Interrupted) => Err(WqError::IoError("input interrupted".to_string())),
        Err(ReadlineError::Eof) => Ok(Value::Null),
        Err(e) => Err(WqError::IoError(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn str_val(s: &str) -> Value {
        Value::List(s.chars().map(Value::Char).collect())
    }

    fn tmpfile() -> String {
        let name: u64 = rand::random();
        let path = std::env::temp_dir().join(format!("wq_io_{name}"));
        path.to_string_lossy().to_string()
    }

    #[test]
    fn write_read_seek() {
        let path = tmpfile();
        let h = open(&[str_val(&path), str_val("w+")]).unwrap();
        fwritet(&[h.clone(), str_val("hi")]).unwrap();
        fseek(&[h.clone(), Value::Int(0)]).unwrap();
        let txt = freadt(&[h.clone()]).unwrap();
        assert_eq!(txt, str_val("hi"));
        let size = fsize(&[str_val(&path)]).unwrap();
        assert_eq!(size, Value::Int(2));
        fclose(&[h]).unwrap();
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn pexists_and_mkdir() {
        let path = tmpfile();
        assert_eq!(pexists(&[str_val(&path)]).unwrap(), Value::Bool(false));
        mkdir(&[str_val(&path)]).unwrap();
        assert_eq!(pexists(&[str_val(&path)]).unwrap(), Value::Bool(true));
        fs::remove_dir_all(&path).unwrap();
    }

    #[test]
    fn encode_decode_roundtrip() {
        let s = str_val("héllo");
        let b = encode(&[s.clone(), str_val("utf-8")]).unwrap();
        let d = decode(&[b, str_val("utf-8")]).unwrap();
        assert_eq!(d, s);
    }

    #[test]
    fn open_missing_file_error() {
        let res = open(&[str_val("/no/such/file"), str_val("r")]);
        assert!(matches!(res, Err(WqError::IoError(_))));
    }
}
