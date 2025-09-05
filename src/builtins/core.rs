use crate::{
    builtins::arity_error,
    repl::stdio::{
        StdinError, stdin_readline, stdin_with_highlight_off, stdout_print, stdout_println,
    },
    value::{Value, WqResult},
    wqerror::WqError,
};
use std::hash::{DefaultHasher, Hash, Hasher};
#[cfg(not(target_arch = "wasm32"))]
use std::process::Command;

pub fn print(args: &[Value]) -> WqResult<Value> {
    if args.is_empty() {
        return Ok(Value::unit());
    }
    for arg in args {
        if let Some(s) = arg.try_str() {
            stdout_print(s.as_str());
        } else {
            stdout_print(arg.to_string().as_str());
        }
    }
    Ok(Value::unit())
}

pub fn println(args: &[Value]) -> WqResult<Value> {
    if args.is_empty() {
        println!();
        return Ok(Value::unit());
    }
    for arg in args {
        if let Some(s) = arg.try_str() {
            stdout_println(s.as_str());
        } else {
            stdout_println(arg.to_string().as_str());
        }
    }
    Ok(Value::unit())
}

pub fn input(args: &[Value]) -> WqResult<Value> {
    if args.len() > 1 {
        return Err(arity_error("input", "0 or 1", args.len()));
    }

    let prompt = if args.len() == 1 {
        args[0]
            .try_str()
            .ok_or_else(|| WqError::DomainError("`input`: expected 'str' at arg0".into()))?
    } else {
        String::new()
    };

    let res = stdin_with_highlight_off(|| stdin_readline(&prompt));
    match res {
        Ok(line) => Ok(Value::List(line.chars().map(Value::Char).collect())),
        Err(StdinError::Eof) => Ok(Value::unit()),
        Err(StdinError::Interrupted) => Err(WqError::IoError("Input interrupted".into())),
        Err(StdinError::Other(e)) => Err(WqError::IoError(e)),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn exec(args: &[Value]) -> WqResult<Value> {
    if args.is_empty() {
        return Err(arity_error("exec", "at least 1", args.len()));
    }

    let parts: Vec<String> = args
        .iter()
        .map(Value::try_str)
        .collect::<Option<Vec<_>>>()
        .ok_or(WqError::DomainError("`exec`: expected 'str'".into()))?;

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
            .map_err(|e| WqError::ExecError(e.to_string()))?
    };

    #[cfg(not(windows))]
    let output = {
        if parts.len() == 1 && parts[0].contains(char::is_whitespace) {
            Command::new("sh")
                .arg("-c")
                .arg(&parts[0])
                .output()
                .map_err(|e| WqError::ExecError(e.to_string()))?
        } else {
            let mut cmd = Command::new(&parts[0]);
            if parts.len() > 1 {
                cmd.args(&parts[1..]);
            }
            cmd.output()
                .map_err(|e| WqError::ExecError(e.to_string()))?
        }
    };

    // shared logic
    if !output.status.success() {
        let code = output
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "unknown".into());
        return Err(WqError::ExecError(format!(
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
    args[0].chr()
}

pub fn ord(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("ord", "1", args.len()));
    }
    args[0].ord()
}

fn as_bool_arg(fname: &str, v: &Value) -> WqResult<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        _ => Err(WqError::DomainError(format!(
            "`{fname}`: expected bool ar arg1"
        ))),
    }
}

pub fn hex(args: &[Value]) -> WqResult<Value> {
    let with_prefix = match args.len() {
        1 => false,
        2 => as_bool_arg("hex", &args[1])?,
        n => return Err(arity_error("hex", "1 or 2", n)),
    };
    args[0].hex_with_prefix(with_prefix)
}

pub fn bin(args: &[Value]) -> WqResult<Value> {
    let with_prefix = match args.len() {
        1 => false,
        2 => as_bool_arg("bin", &args[1])?,
        n => return Err(arity_error("bin", "1 or 2", n)),
    };
    args[0].bin_with_prefix(with_prefix)
}

pub fn oct(args: &[Value]) -> WqResult<Value> {
    let with_prefix = match args.len() {
        1 => false,
        2 => as_bool_arg("oct", &args[1])?,
        n => return Err(arity_error("oct", "1 or 2", n)),
    };
    args[0].oct_with_prefix(with_prefix)
}

pub fn int(args: &[Value]) -> WqResult<Value> {
    // Parse args
    let (val, base_opt) = match args {
        [v] => (v, None),
        [v, Value::Int(b)] if (2..=36).contains(b) => (v, Some(*b as u32)),
        [_, _] => {
            return Err(WqError::DomainError(
                "`int`: base must be an integer in 2..=36".into(),
            ));
        }
        _ => return Err(arity_error("int", "1 or 2", args.len())),
    };

    let base = base_opt.unwrap_or(10);

    match val {
        // base not allowed when converting an integer
        Value::Int(n) => {
            if base_opt.is_some() {
                Err(WqError::DomainError(
                    "`int`: base not allowed when converting an integer".into(),
                ))
            } else {
                Ok(Value::Int(*n))
            }
        }
        v => {
            let s = v.try_str().ok_or_else(|| {
                WqError::DomainError("`int`: expected a char or a string (list of chars)".into())
            })?;
            parse_str_to_i64(&s, base)
        }
    }
}

// Sign first, optional base-specific prefix, underscores allowed.
// Parse magnitude as u128, then apply sign safely (i64::MIN supported).
fn parse_str_to_i64(s: &str, base: u32) -> WqResult<Value> {
    let s = s.trim();
    if s.is_empty() {
        return Err(WqError::DomainError("`int`: empty string".into()));
    }

    // sign
    let (neg, rest) = match s.as_bytes().first() {
        Some(b'+') => (false, &s[1..]),
        Some(b'-') => (true, &s[1..]),
        _ => (false, s),
    };

    // strip canonical prefixes only if base matches
    let rest = if (base == 16 && (rest.starts_with("0x") || rest.starts_with("0X")))
        || (base == 2 && (rest.starts_with("0b") || rest.starts_with("0B")))
        || (base == 8 && (rest.starts_with("0o") || rest.starts_with("0O")))
    {
        &rest[2..]
    } else {
        rest
    };

    // ignore underscores
    let digits: String = rest.chars().filter(|&c| c != '_').collect();
    if digits.is_empty() {
        return Err(WqError::DomainError("`int`: invalid literal".into()));
    }

    // magnitude as u128 to support i64::MIN
    let mag = u128::from_str_radix(&digits, base).map_err(|_| {
        WqError::DomainError(format!("`int`: invalid literal `{s}` for base {base}"))
    })?;

    let val = if neg {
        let limit = (i64::MAX as u128) + 1; // allowable magnitude for negative
        if mag > limit {
            return Err(WqError::DomainError("`int`: overflow".into()));
        }
        if mag == limit {
            i64::MIN
        } else {
            -(mag as i64)
        }
    } else {
        if mag > i64::MAX as u128 {
            return Err(WqError::DomainError("`int`: overflow".into()));
        }
        mag as i64
    };

    Ok(Value::Int(val))
}

pub fn raise(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("raise", "2", args.len()));
    }

    match (&args[0], &args[1]) {
        (_, Value::Int(code)) if *code <= 0 => Err(WqError::DomainError(
            "`raise`: expected positive int at arg1 (err code)".into(),
        )),
        (m, Value::Int(code)) => {
            let msg = m
                .try_str()
                .ok_or_else(|| WqError::DomainError("`raise`: expected 'str' at arg0".into()))?
                .to_string();

            let code = *code as i32;

            // Prefer a built-in error if the code matches; otherwise, use CustomError
            let err = WqError::from_code_with_msg(code, msg.clone())
                .unwrap_or(WqError::UnknownError(msg, code));

            Err(err)
        }
        _ => Err(WqError::DomainError(
            "`raise`: expected positive int at arg1 (err code)".into(),
        )),
    }
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
                self.type_name()
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
}
