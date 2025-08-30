use crate::{
    builtins::{arity_error, value_to_string},
    repl::{StdinError, stdin_readline, stdout_print, stdout_println},
    value::{Value, WqError, WqResult},
};
use std::hash::{DefaultHasher, Hash, Hasher};
#[cfg(not(target_arch = "wasm32"))]
use std::process::Command;

pub fn echo(args: &[Value]) -> WqResult<Value> {
    fn print_value(value: &Value, newline: bool) {
        match value {
            Value::List(items) if items.iter().all(|v| matches!(v, Value::Char(_))) => {
                let s: String = items
                    .iter()
                    .map(|v| {
                        if let Value::Char(c) = v {
                            *c
                        } else {
                            unreachable!()
                        }
                    })
                    .collect();
                if newline {
                    stdout_println(&s);
                } else {
                    stdout_print(&s);
                }
            }
            Value::Char(c) => {
                let s = c.to_string();
                if newline {
                    stdout_println(&s);
                } else {
                    stdout_print(&s);
                }
            }
            other => {
                let s = other.to_string();
                if newline {
                    stdout_println(&s);
                } else {
                    stdout_print(&s);
                }
            }
        }
    }

    if args.is_empty() {
        println!();
        return Ok(Value::Null);
    }

    if args.len() == 2 {
        if let Value::Bool(b) = args[1] {
            print_value(&args[0], b);
            return Ok(Value::Null);
        }
    }

    for arg in args {
        print_value(arg, true);
    }

    Ok(Value::Null)
}

pub fn input(args: &[Value]) -> WqResult<Value> {
    if args.len() > 1 {
        return Err(arity_error("input", "0 or 1", args.len()));
    }

    let prompt = if args.len() == 1 {
        value_to_string(&args[0])?
    } else {
        String::new()
    };

    match stdin_readline(&prompt) {
        Ok(line) => Ok(Value::List(line.chars().map(Value::Char).collect())),
        Err(StdinError::Eof) => Ok(Value::Null),
        Err(StdinError::Interrupted) => Err(WqError::IoError("Input interrupted".into())),
        Err(StdinError::Other(e)) => Err(WqError::IoError(e)),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn exec(args: &[Value]) -> WqResult<Value> {
    use crate::builtins::values_to_strings;
    if args.is_empty() {
        return Err(arity_error("exec", "at least 1", args.len()));
    }

    let parts = values_to_strings(args)?;

    #[cfg(windows)]
    let output = {
        let command = parts
            .iter()
            .map(|p| {
                if p.contains(char::is_whitespace) {
                    let escaped = p.replace("'", "''");
                    format!("'{escaped}'")
                } else {
                    p.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        Command::new("powershell")
            .arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(&command)
            .output()
            .map_err(|e| WqError::RuntimeError(e.to_string()))?
    };

    #[cfg(not(windows))]
    let output = {
        if parts.len() == 1 && parts[0].contains(char::is_whitespace) {
            Command::new("sh")
                .arg("-c")
                .arg(&parts[0])
                .output()
                .map_err(|e| WqError::RuntimeError(e.to_string()))?
        } else {
            let mut cmd = Command::new(&parts[0]);
            if parts.len() > 1 {
                cmd.args(&parts[1..]);
            }
            cmd.output()
                .map_err(|e| WqError::RuntimeError(e.to_string()))?
        }
    };

    // shared logic
    if !output.status.success() {
        let code = output
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "unknown".into());
        return Err(WqError::RuntimeError(format!(
            "`exec`: exec failed (exit {code})"
        )));
    }
    // decode
    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<Value> = text
        .lines()
        .map(|line| {
            let chars = line.chars().map(Value::Char).collect();
            Value::List(chars)
        })
        .collect();

    Ok(Value::List(lines))
}

pub fn wq_match(args: &[Value]) -> WqResult<Value> {
    if args.is_empty() {
        return Ok(Value::Bool(true));
    }

    let first = &args[0];
    let all_equal = args.iter().all(|x| x == first);

    Ok(Value::Bool(all_equal))
}

pub fn chr(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("chr", "1", args.len()));
    }
    match &args[0] {
        Value::Int(n) => {
            let ch = char::from_u32(*n as u32)
                .ok_or_else(|| WqError::DomainError("`chr`: invalid char code".into()))?;
            Ok(Value::Char(ch))
        }
        Value::IntList(items) => {
            let mut out = Vec::new();
            for &n in items {
                let ch = char::from_u32(n as u32)
                    .ok_or_else(|| WqError::DomainError("`chr`: invalid char code".into()))?;
                out.push(Value::Char(ch));
            }
            Ok(Value::List(out))
        }
        Value::List(items) => {
            let mut out = Vec::new();
            for v in items {
                if let Value::Int(n) = v {
                    let ch = char::from_u32(*n as u32)
                        .ok_or_else(|| WqError::DomainError("`chr`: invalid char code".into()))?;
                    out.push(Value::Char(ch));
                } else {
                    return Err(WqError::TypeError(
                        "`chr`: expected int or a list of ints".to_string(),
                    ));
                }
            }
            Ok(Value::List(out))
        }
        _ => Err(WqError::TypeError(
            "`chr`: expected int or a list of ints".to_string(),
        )),
    }
}

pub fn ord(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("ord", "1", args.len()));
    }

    match &args[0] {
        Value::Char(c) => Ok(Value::Int(*c as i64)),
        Value::List(items) => {
            let mut out = Vec::new();
            for v in items {
                match v {
                    Value::Char(ch) => out.push(*ch as i64),
                    _ => {
                        return Err(WqError::TypeError(
                            "`ord`: expected char or a list of chars".into(),
                        ));
                    }
                }
            }
            Ok(Value::IntList(out))
        }
        Value::Symbol(s) => Ok(Value::IntList(s.chars().map(|c| c as i64).collect())),
        _ => Err(WqError::TypeError(
            "`ord`: expected char or a list of chars".into(),
        )),
    }
}

pub fn type_of_verbose(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("type", "1", args.len()));
    }
    Ok(Value::List(
        args[0]
            .type_name_verbose()
            .to_string()
            .chars()
            .map(Value::Char)
            .collect(),
    ))
}

pub fn type_of_simple(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("type", "1", args.len()));
    }
    Ok(Value::List(
        args[0]
            .type_name_simple()
            .to_string()
            .chars()
            .map(Value::Char)
            .collect(),
    ))
}

pub fn to_symbol(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("symbol", "1", args.len()));
    }

    let input = &args[0];
    let name = match input {
        Value::Symbol(s) => s.clone(),
        Value::Char(c) => (*c).to_string(),
        Value::List(items) if input.is_str() => {
            let mut s = String::new();
            for v in items {
                if let Value::Char(c) = v {
                    s.push(*c);
                }
            }
            s
        }
        _ => {
            return Err(WqError::TypeError(format!(
                "`symbol`: expected 'str', got {}",
                input.type_name_verbose()
            )));
        }
    };

    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '?')
    {
        return Err(WqError::DomainError(format!("invalid symbol name: {name}")));
    }

    Ok(Value::symbol(name))
}

pub fn is_null(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("null?", "1", args.len()));
    }
    match args[0] {
        Value::Null => Ok(Value::Bool(true)),
        _ => Ok(Value::Bool(false)),
    }
}

pub fn is_list(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("list?", "1", args.len()));
    }
    Ok(Value::Bool(args[0].is_list()))
}

pub fn is_dict(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("dict?", "1", args.len()));
    }
    Ok(Value::Bool(args[0].is_dict()))
}

pub fn is_atom(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("atom?", "1", args.len()));
    }
    Ok(Value::Bool(args[0].is_atom()))
}
pub fn is_int(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("int?", "1", args.len()));
    }
    match args[0] {
        Value::Int(_) => Ok(Value::Bool(true)),
        _ => Ok(Value::Bool(false)),
    }
}
pub fn is_float(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("float?", "1", args.len()));
    }
    match args[0] {
        Value::Float(_) => Ok(Value::Bool(true)),
        _ => Ok(Value::Bool(false)),
    }
}
pub fn is_number(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("number?", "1", args.len()));
    }
    match args[0] {
        Value::Int(_) | Value::Float(_) => Ok(Value::Bool(true)),
        _ => Ok(Value::Bool(false)),
    }
}
pub fn is_str(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("str?", "1", args.len()));
    }
    Ok(Value::Bool(args[0].is_str()))
}

trait TryHash {
    fn try_hash<H: Hasher>(&self, state: &mut H) -> WqResult<()>;
}

impl TryHash for Value {
    fn try_hash<H: Hasher>(&self, state: &mut H) -> WqResult<()> {
        match self {
            Value::Int(i) => {
                0u8.hash(state); // type tag
                i.hash(state);
                Ok(())
            }
            Value::Char(c) => {
                1u8.hash(state);
                c.hash(state);
                Ok(())
            }
            Value::Bool(b) => {
                2u8.hash(state);
                b.hash(state);
                Ok(())
            }
            Value::Symbol(s) => {
                3u8.hash(state);
                s.hash(state);
                Ok(())
            }
            Value::Float(f) => {
                // floats are hashable unless nan; +0.0 and -0.0 hash differently
                if f.is_nan() {
                    return Err(WqError::DomainError("`hash`: cannot hash nan".into()));
                }
                4u8.hash(state);
                f.to_bits().hash(state);
                Ok(())
            }
            Value::List(xs) => {
                5u8.hash(state);
                xs.len().hash(state);
                for x in xs {
                    // Abort on the first unhashable element
                    x.try_hash(state)?
                }
                Ok(())
            }
            Value::IntList(xs) => {
                6u8.hash(state);
                xs.hash(state);
                Ok(())
            }
            Value::Dict(map) => {
                7u8.hash(state);

                // make hashing order-independent
                let mut entry_hashes: Vec<u64> = Vec::with_capacity(map.len());
                for (k, v) in map {
                    let mut entry_hasher = DefaultHasher::new();
                    k.hash(&mut entry_hasher);
                    v.try_hash(&mut entry_hasher)?;
                    entry_hashes.push(entry_hasher.finish());
                }
                entry_hashes.sort_unstable();
                entry_hashes.hash(state);

                Ok(())
            }
            _ => Err(WqError::DomainError(format!(
                "`hash`: cannot hash {}",
                self.type_name_verbose()
            ))),
        }
    }
}

pub fn hash(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("hash", "1", args.len()));
    }

    let mut hasher = DefaultHasher::new();
    args[0].try_hash(&mut hasher)?;
    Ok(Value::Int(hasher.finish() as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chr_single_int() {
        let result = chr(&[Value::Int(65)]).unwrap();
        assert_eq!(result, Value::Char('A'));
    }

    #[test]
    fn chr_int_list() {
        let result = chr(&[Value::IntList(vec![65, 66])]).unwrap();
        assert_eq!(
            result,
            Value::List(vec![Value::Char('A'), Value::Char('B')])
        );
    }

    #[test]
    fn ord_single_char() {
        let result = ord(&[Value::Char('A')]).unwrap();
        assert_eq!(result, Value::Int(65));
    }

    #[test]
    fn ord_char_list() {
        let input = Value::List(vec![Value::Char('A'), Value::Char('B')]);
        let result = ord(&[input]).unwrap();
        assert_eq!(result, Value::IntList(vec![65, 66]));
    }

    #[test]
    fn symbol_accepts_question_mark() {
        let val = Value::List("a?".chars().map(Value::Char).collect());
        let result = to_symbol(&[val]).unwrap();
        assert_eq!(result, Value::symbol("a?".to_string()));
    }
}
