use super::arity_error;
use crate::{
    builtins::value_to_string,
    value::{BufReadSeek, StreamHandle, Value, WqError, WqResult, WriteSeek},
};

use encoding_rs::Encoding;

use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
};

fn value_to_bytes(v: &Value) -> WqResult<Vec<u8>> {
    match v {
        Value::IntList(arr) => Ok(arr.iter().map(|&n| n as u8).collect()),
        Value::List(items) => items
            .iter()
            .map(|x| {
                if let Value::Int(i) = x {
                    Ok(*i as u8)
                } else {
                    Err(WqError::DomainError(format!(
                        "Cannot interpret {v} as bytes",
                    )))
                }
            })
            .collect(),
        Value::Int(n) => Ok(vec![*n as u8]),
        _ => Err(WqError::DomainError(format!(
            "Cannot interpret {v} as bytes",
        ))),
    }
}

pub fn open(args: &[Value]) -> WqResult<Value> {
    if args.is_empty() || args.len() > 2 {
        return Err(arity_error("open", "1 or 2", args.len()));
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
        _ => {
            return Err(WqError::DomainError(format!(
                "`open`: invalid open mode `{mode}`"
            )));
        }
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

pub fn fexists(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("fexists?", "1", args.len()));
    }
    let path = value_to_string(&args[0])?;
    Ok(Value::Bool(Path::new(&path).exists()))
}

pub fn mkdir(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("mkdir", "1", args.len()));
    }
    let path = value_to_string(&args[0])?;
    fs::create_dir_all(&path).map_err(|e| WqError::IoError(e.to_string()))?;
    Ok(Value::Null)
}

pub fn fsize(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("fsize", "1", args.len()));
    }
    let path = value_to_string(&args[0])?;
    let meta = fs::metadata(&path).map_err(|e| WqError::IoError(e.to_string()))?;
    Ok(Value::Int(meta.len() as i64))
}

pub fn fwrite(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("fwrite", "2", args.len()));
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
            Err(WqError::IoError("`fwrite`: stream not writable".into()))
        }
    } else {
        Err(WqError::TypeError(format!(
            "`fwrite`: expected stream at arg0, got {}",
            args[0].type_name_verbose()
        )))
    }
}

pub fn fwritet(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("fwritet", "2", args.len()));
    }
    let s = value_to_string(&args[1])?;
    fwrite(&[
        args[0].clone(),
        Value::IntList(s.into_bytes().into_iter().map(|b| b as i64).collect()),
    ])
}

pub fn fread(args: &[Value]) -> WqResult<Value> {
    if args.is_empty() || args.len() > 2 {
        return Err(arity_error("fread", "1 or 2", args.len()));
    }
    if let Value::Stream(rc) = &args[0] {
        let mut handle = rc.lock().unwrap();
        if let Some(reader) = handle.reader.as_mut() {
            let mut buf = Vec::new();
            if args.len() == 2 {
                if let Value::Int(n) = args[1] {
                    if n < 0 {
                        return Err(WqError::DomainError("`fread`: negative length".into()));
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
                    return Err(WqError::TypeError(format!(
                        "`fread`: invalid length, expected int, got {}",
                        args[1].type_name_verbose()
                    )));
                }
            } else {
                reader
                    .read_to_end(&mut buf)
                    .map_err(|e| WqError::IoError(e.to_string()))?;
            }
            Ok(Value::IntList(buf.into_iter().map(|b| b as i64).collect()))
        } else {
            Err(WqError::IoError("`fread`: stream not readable".into()))
        }
    } else {
        Err(WqError::TypeError(format!(
            "`fread`: expected stream at arg0, got {}",
            args[0].type_name_verbose()
        )))
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
        return Err(arity_error("freadtln", "1", args.len()));
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
        Err(WqError::TypeError(format!(
            "`freadtln`: expected stream at arg0, got {}",
            args[0].type_name_verbose()
        )))
    }
}

pub fn fseek(args: &[Value]) -> WqResult<Value> {
    if args.len() < 2 || args.len() > 3 {
        return Err(arity_error("fseek", "2 or 3", args.len()));
    }
    let offset = if let Value::Int(n) = args[1] {
        n
    } else {
        return Err(WqError::TypeError(format!(
            "`fseek`: invalid offset, expected int, got {}",
            args[1].type_name_verbose()
        )));
    };
    let whence = if args.len() == 3 {
        if let Value::Int(w) = args[2] {
            w
        } else {
            return Err(WqError::TypeError(format!(
                "`fseek`: invalid whence, expected int, got {}",
                args[2].type_name_verbose()
            )));
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
            _ => {
                return Err(WqError::DomainError(
                    "`fseek`: invalid whence, expected 0, 1, or 2".into(),
                ));
            }
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
            Err(WqError::IoError("`fseek`: stream not seekable".into()))
        }
    } else {
        Err(WqError::TypeError(format!(
            "`fseek`: expected stream at arg0, got {}",
            args[0].type_name_verbose()
        )))
    }
}

pub fn ftell(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("ftell", "1", args.len()));
    }

    if let Value::Stream(rc) = &args[0] {
        let mut handle = rc.lock().unwrap();

        // Prefer writer if present.
        // Both sides are Seek, so just ask for stream_position.
        if let Some(w) = handle.writer.as_mut() {
            let pos = w
                .stream_position()
                .map_err(|e| WqError::IoError(e.to_string()))?;
            return Ok(Value::Int(pos as i64));
        }
        if let Some(r) = handle.reader.as_mut() {
            let pos = r
                .stream_position()
                .map_err(|e| WqError::IoError(e.to_string()))?;
            return Ok(Value::Int(pos as i64));
        }

        Err(WqError::IoError("`ftell`: stream not seekable".into()))
    } else {
        Err(WqError::TypeError(format!(
            "`ftell`: expected stream at arg0, got {}",
            args[0].type_name_verbose()
        )))
    }
}

pub fn fclose(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("fclose", "1", args.len()));
    }
    if let Value::Stream(rc) = &args[0] {
        let mut handle = rc.lock().unwrap();
        handle.reader = None;
        handle.writer = None;
        handle.child = None;
        Ok(Value::Null)
    } else {
        Err(WqError::TypeError(format!(
            "`fclose`: expected stream at arg0, got {}",
            args[0].type_name_verbose()
        )))
    }
}

fn find_encoding(label: &str) -> Option<&'static Encoding> {
    Encoding::for_label(label.as_bytes())
}

pub fn decode(args: &[Value]) -> WqResult<Value> {
    if args.len() < 2 || args.len() > 3 {
        return Err(arity_error("decode", "2 or 3", args.len()));
    }
    let bytes = value_to_bytes(&args[0])?;
    let codec = value_to_string(&args[1])?;
    let mode = if args.len() == 3 {
        value_to_string(&args[2])?
    } else {
        "s".to_string()
    };
    let enc = find_encoding(&codec)
        .ok_or_else(|| WqError::DomainError("`decode`: unsupported codec".into()))?;
    let (text, had_errors) = enc.decode_without_bom_handling(&bytes);
    let s = match mode.as_str() {
        "s" => {
            if had_errors {
                return Err(WqError::RuntimeError(
                    "`decode`: strict mode decode error".into(),
                ));
            }
            text
        }
        "r" => text,
        _ => {
            return Err(WqError::DomainError(
                "`decode`: invalid mode, expected \"s\" or \"r\"".into(),
            ));
        }
    };
    Ok(Value::List(s.chars().map(Value::Char).collect()))
}

pub fn encode(args: &[Value]) -> WqResult<Value> {
    if args.len() < 2 || args.len() > 3 {
        return Err(arity_error("encode", "2 or 3", args.len()));
    }
    let s = value_to_string(&args[0])?;
    let codec = value_to_string(&args[1])?;
    let mode = if args.len() == 3 {
        value_to_string(&args[2])?
    } else {
        "s".into()
    };
    let enc = find_encoding(&codec)
        .ok_or_else(|| WqError::DomainError("`encode`: unsupported codec".into()))?;

    let out: Vec<u8> = match mode.as_str() {
        "s" => {
            let (cow, _enc_used, had_errors) = enc.encode(&s);
            if had_errors {
                return Err(WqError::RuntimeError(
                    "`encode`: strict mode encode error".into(),
                ));
            }
            cow.into_owned()
        }
        "r" => {
            let (cow, _enc_used, _had_errors) = enc.encode(&s);
            cow.into_owned()
        }
        _ => {
            return Err(WqError::DomainError(
                "`encode`: invalid mode, expected \"s\" or \"r\"".into(),
            ));
        }
    };

    Ok(Value::IntList(out.into_iter().map(|b| b as i64).collect()))
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
        assert_eq!(fexists(&[str_val(&path)]).unwrap(), Value::Bool(false));
        mkdir(&[str_val(&path)]).unwrap();
        assert_eq!(fexists(&[str_val(&path)]).unwrap(), Value::Bool(true));
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
    #[test]
    fn ftell_basic() {
        let path = std::env::temp_dir().join("wq_ftell_test.txt");
        let sv = |s: &str| Value::List(s.chars().map(Value::Char).collect());

        let h = open(&[sv(path.to_str().unwrap()), sv("w+")]).unwrap();
        fwritet(&[h.clone(), sv("hello")]).unwrap();
        let pos1 = ftell(&[h.clone()]).unwrap();
        assert_eq!(pos1, Value::Int(5));

        fseek(&[h.clone(), Value::Int(2)]).unwrap();
        let pos2 = ftell(&[h.clone()]).unwrap();
        assert_eq!(pos2, Value::Int(2));

        fclose(&[h]).unwrap();
        let _ = std::fs::remove_file(path);
    }
}
