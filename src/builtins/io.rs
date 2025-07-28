use crate::{
    builtins::values_to_strings,
    value::valuei::{StreamHandle, Value, WqError, WqResult},
};

use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Read, Write},
};

use once_cell::sync::Lazy;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::sync::Mutex;

pub static RUSTYLINE: Lazy<Mutex<Option<DefaultEditor>>> = Lazy::new(|| Mutex::new(None));

pub fn fopen(args: &[Value]) -> WqResult<Value> {
    if args.is_empty() || args.len() > 2 {
        return Err(WqError::ArityError(
            "fopen expects 1 or 2 arguments".to_string(),
        ));
    }
    let path = values_to_strings(&[args[0].clone()])?.pop().unwrap();
    let mode = if args.len() == 2 {
        values_to_strings(&[args[1].clone()])?.pop().unwrap()
    } else {
        "r".to_string()
    };
    let mut options = OpenOptions::new();
    let mut reader = None;
    let mut writer = None;
    match mode.as_str() {
        "r" => {
            options.read(true);
            let file = options
                .open(&path)
                .map_err(|e| WqError::RuntimeError(e.to_string()))?;
            reader = Some(Box::new(BufReader::new(file)) as Box<dyn BufRead + Send>);
        }
        "w" => {
            options.write(true).create(true).truncate(true);
            let file = options
                .open(&path)
                .map_err(|e| WqError::RuntimeError(e.to_string()))?;
            writer = Some(Box::new(file) as Box<dyn Write + Send>);
        }
        "a" => {
            options.write(true).create(true).append(true);
            let file = options
                .open(&path)
                .map_err(|e| WqError::RuntimeError(e.to_string()))?;
            writer = Some(Box::new(file) as Box<dyn Write + Send>);
        }
        "rw" | "wr" => {
            options.read(true).write(true).create(true);
            let file = options
                .open(&path)
                .map_err(|e| WqError::RuntimeError(e.to_string()))?;
            reader = Some(Box::new(BufReader::new(
                file.try_clone()
                    .map_err(|e| WqError::RuntimeError(e.to_string()))?,
            )) as Box<dyn BufRead + Send>);
            writer = Some(Box::new(file) as Box<dyn Write + Send>);
        }
        _ => {
            return Err(WqError::DomainError("invalid mode".into()));
        }
    }
    let handle = StreamHandle {
        reader,
        writer,
        child: None,
    };
    Ok(Value::stream(handle))
}

pub fn fread(args: &[Value]) -> WqResult<Value> {
    if args.is_empty() || args.len() > 2 {
        return Err(WqError::ArityError(
            "fread expects 1 or 2 arguments".to_string(),
        ));
    }
    if let Value::Stream(rc) = &args[0] {
        let mut handle = rc.lock().unwrap();
        if let Some(reader) = handle.reader.as_mut() {
            if args.len() == 2 {
                if let Value::Int(n) = args[1] {
                    if n < 0 {
                        return Err(WqError::RuntimeError(
                            "fread length must be non-negative".to_string(),
                        ));
                    }
                    let mut buf = vec![0u8; n as usize];
                    let read = reader
                        .read(&mut buf)
                        .map_err(|e| WqError::RuntimeError(e.to_string()))?;
                    buf.truncate(read);
                    Ok(Value::IntList(buf.into_iter().map(|b| b as i64).collect()))
                } else {
                    Err(WqError::TypeError("fread length".into()))
                }
            } else {
                let mut line = String::new();
                let n = reader
                    .read_line(&mut line)
                    .map_err(|e| WqError::RuntimeError(e.to_string()))?;
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
            }
        } else {
            Err(WqError::RuntimeError("stream not readable".into()))
        }
    } else {
        Err(WqError::TypeError("fread expects a stream".into()))
    }
}

fn value_to_bytes(v: &Value) -> WqResult<Vec<u8>> {
    match v {
        Value::IntList(arr) => Ok(arr.iter().map(|&n| n as u8).collect()),
        Value::List(items) => items
            .iter()
            .map(|x| {
                if let Value::Int(i) = x {
                    Ok(*i as u8)
                } else {
                    Err(WqError::TypeError("byte".into()))
                }
            })
            .collect(),
        Value::Int(n) => Ok(vec![*n as u8]),
        _ => Err(WqError::TypeError("bytes".into())),
    }
}

pub fn fwrite(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::ArityError(
            "fwrite expects 2 arguments".to_string(),
        ));
    }
    if let Value::Stream(rc) = &args[0] {
        let mut handle = rc.lock().unwrap();
        if let Some(w) = handle.writer.as_mut() {
            let bytes = value_to_bytes(&args[1])?;
            w.write_all(&bytes)
                .map_err(|e| WqError::RuntimeError(e.to_string()))?;
            Ok(Value::Null)
        } else {
            Err(WqError::RuntimeError("stream not writable".into()))
        }
    } else {
        Err(WqError::TypeError("fwrite expects a stream".into()))
    }
}

pub fn fclose(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError("fclose expects 1 argument".to_string()));
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

pub fn fsize(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError("fsize expects 1 argument".to_string()));
    }
    let path = values_to_strings(&[args[0].clone()])?.pop().unwrap();
    let meta = std::fs::metadata(&path).map_err(|e| WqError::RuntimeError(e.to_string()))?;
    Ok(Value::Int(meta.len() as i64))
}

pub fn input(args: &[Value]) -> WqResult<Value> {
    if args.len() > 1 {
        return Err(WqError::ArityError(
            "input expects 0 or 1 argument".to_string(),
        ));
    }

    let prompt = if args.len() == 1 {
        values_to_strings(&[args[0].clone()])?
            .into_iter()
            .next()
            .unwrap()
    } else {
        String::new()
    };

    let mut rl_guard = RUSTYLINE.lock().unwrap();
    if rl_guard.is_none() {
        *rl_guard = Some(DefaultEditor::new().map_err(|e| WqError::RuntimeError(e.to_string()))?);
    }
    match rl_guard.as_mut().unwrap().readline(&prompt) {
        Ok(line) => Ok(Value::List(line.chars().map(Value::Char).collect())),
        Err(ReadlineError::Interrupted) => {
            Err(WqError::RuntimeError("input interrupted".to_string()))
        }
        Err(ReadlineError::Eof) => Ok(Value::Null),
        Err(e) => Err(WqError::RuntimeError(e.to_string())),
    }
}
