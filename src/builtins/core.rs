use crate::{
    builtins::{
        BuiltinEnum,
        wqerror_helper::{check_arity, type_mismatch},
    },
    stdio::{StdinError, stdin_readline, stdin_with_highlight_off, stdout_print, stdout_println},
    value::{Excerpt, Value, WqResult, into_wq_str},
    vm::Vm,
    wqerror::{WqError, WqErrorType},
};

use num_bigint::BigInt;
use num_traits::Num;

pub fn print(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    if args.is_empty() {
        return Ok(Value::unit());
    }
    for arg in args {
        if let Ok(s) = arg.try_to_string() {
            stdout_print(s);
        } else {
            stdout_print(arg.to_string());
        }
    }
    Ok(Value::unit())
}

pub fn echo(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    if args.is_empty() {
        stdout_println("");
        return Ok(Value::unit());
    }
    for arg in args {
        if let Ok(s) = arg.try_to_string() {
            stdout_println(s);
        } else {
            stdout_println(arg.to_string());
        }
    }
    Ok(Value::unit())
}

pub fn input(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BuiltinEnum::Input, [0, 1], args)?;
    let prompt = if args.len() == 1 {
        args[0]
            .try_to_string()
            .map_err(|e| e.src(BuiltinEnum::Input))?
    } else {
        String::new()
    };
    let res = stdin_with_highlight_off(|| stdin_readline(&prompt));
    match res {
        Ok(line) => Ok(into_wq_str(line)),
        Err(StdinError::Eof) => Ok(Value::unit()),
        Err(StdinError::Interrupted) => Ok(Value::unit()),
        Err(StdinError::Other(e)) => Err(WqError::new(WqErrorType::Io)
            .src(BuiltinEnum::Input)
            .attach_note(format!("original error: {}", e))),
    }
}

pub fn bfn(vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BuiltinEnum::Bfn, [0], args)?;
    let mut funcs = vm.builtins.list_functions();
    funcs.sort();
    let funcstr = funcs.into_iter().map(into_wq_str).collect();
    Ok(Value::List(funcstr))
}

pub fn chr(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BuiltinEnum::Chr, [1], args)?;
    args[0]
        .chr()
        .map_err(|e| e.into_wqerror().src(BuiltinEnum::Chr))
}

pub fn ord(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BuiltinEnum::Ord, [1], args)?;
    args[0]
        .ord()
        .map_err(|e| e.into_wqerror().src(BuiltinEnum::Ord))
}

pub fn int(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    fn unexpected_base() -> WqError {
        WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Int)
            .msg("unexpected base")
            .attach_note("base should not be provided when converting int")
            .at_arg(1)
    }

    check_arity(BuiltinEnum::Int, [1, 2], args)?;
    // Parse args
    let (val, base_opt) = match args {
        [v] => (v, None),
        [v, Value::Int(b)] if (2..=36).contains(b) => (v, Some(u32::try_from(*b).unwrap())),
        [_, v2] => {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::Int)
                .msg("expected valid base")
                .attach_note("valid base is int in 2..=36")
                .at_arg(1)
                .attach_note(format!("got {}", v2.excerpt())));
        }
        _ => unreachable!(),
    };
    let base = base_opt.unwrap_or(10);
    match val {
        Value::Int(n) => {
            if base_opt.is_some() {
                Err(unexpected_base())
            } else {
                Ok(Value::Int(*n))
            }
        }
        Value::BigInt(n) => {
            if base_opt.is_some() {
                Err(unexpected_base())
            } else {
                Ok(Value::BigInt(n.clone()))
            }
        }
        v => {
            let s = v.try_to_string().map_err(|e| e.src(BuiltinEnum::Int))?;
            let s = s.trim();
            if s.is_empty() {
                return Ok(Value::unit());
            }
            // sign
            let (neg, rest) = match s.as_bytes().first() {
                Some(b'+') => (false, &s[1..]),
                Some(b'-') => (true, &s[1..]),
                _ => (false, s),
            };
            // canonical prefixes (only if base matches)
            let rest = match base {
                16 => rest
                    .strip_prefix("0x")
                    .or_else(|| rest.strip_prefix("0X"))
                    .unwrap_or(rest),
                2 => rest
                    .strip_prefix("0b")
                    .or_else(|| rest.strip_prefix("0B"))
                    .unwrap_or(rest),
                8 => rest
                    .strip_prefix("0o")
                    .or_else(|| rest.strip_prefix("0O"))
                    .unwrap_or(rest),
                _ => rest,
            };
            // ignore underscores
            let digits: String = rest.chars().filter(|&c| c != '_').collect();
            if digits.is_empty() {
                return Err(WqError::new(WqErrorType::Domain)
                    .src(BuiltinEnum::Int)
                    .msg("expected valid int literal")
                    .at_arg(0)
                    .attach_note("digits are required after optional sign or prefix"));
            }
            let mut mag = BigInt::from_str_radix(&digits, base).map_err(|e| {
                WqError::new(WqErrorType::Domain)
                    .src(BuiltinEnum::Int)
                    .msg("expected valid int literal")
                    .at_arg(0)
                    .attach_note(format!("original error: {}", e))
            })?;
            if neg {
                mag = -mag;
            }
            Ok(Value::from_bigint(mag))
        }
    }
}

pub fn bin(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BuiltinEnum::Bin, [1, 2], args)?;
    let (target, with_prefix) = match args.len() {
        1 => (&args[0], true),
        2 => match args[0] {
            Value::Bool(b) => (&args[1], b),
            _ => {
                return Err(type_mismatch(BuiltinEnum::Bin, 0, "bool", &args[0]));
            }
        },
        _ => unreachable!(),
    };
    target
        .to_bin_repr(with_prefix)
        .map_err(|e| e.into_wqerror().src(BuiltinEnum::Bin))
}

pub fn oct(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BuiltinEnum::Oct, [1, 2], args)?;
    let (target, with_prefix) = match args.len() {
        1 => (&args[0], true),
        2 => match args[0] {
            Value::Bool(b) => (&args[1], b),
            _ => {
                return Err(type_mismatch(BuiltinEnum::Oct, 0, "bool", &args[0]));
            }
        },
        _ => unreachable!(),
    };
    target
        .to_oct_repr(with_prefix)
        .map_err(|e| e.into_wqerror().src(BuiltinEnum::Oct))
}

pub fn hex(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BuiltinEnum::Hex, [1, 2], args)?;
    let (target, with_prefix) = match args.len() {
        1 => (&args[0], true),
        2 => match args[0] {
            Value::Bool(b) => (&args[1], b),
            _ => {
                return Err(type_mismatch(BuiltinEnum::Hex, 0, "bool", &args[0]));
            }
        },
        _ => unreachable!(),
    };
    target
        .to_hex_repr(with_prefix)
        .map_err(|e| e.into_wqerror().src(BuiltinEnum::Hex))
}

pub fn raise(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BuiltinEnum::Raise, [0, 1], args)?;
    match args.len() {
        0 => Err(WqError::new(WqErrorType::Raise).src(BuiltinEnum::Raise)),
        1 => {
            let msg = args[0]
                .try_to_string()
                .map_err(|e| e.src(BuiltinEnum::Raise))?;
            Err(WqError::new(WqErrorType::Raise)
                .src(BuiltinEnum::Raise)
                .msg(msg))
        }
        _ => unreachable!(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn exec(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    use crate::value::IntoWqValue as _;

    if args.is_empty() {
        return Err(WqError::new(WqErrorType::Arity)
            .src(BuiltinEnum::Exec)
            .msg("expected 1 or more args")
            .attach_note("require at least the program name"));
    }
    // Convert all args to strings
    let parts: Vec<String> = args
        .iter()
        .enumerate()
        .map(|(i, v)| v.try_to_string().map_err(|e| (i, e)))
        .collect::<Result<_, _>>()
        .map_err(|(i, e)| e.src(BuiltinEnum::Exec).at_arg(i))?;
    // parts[0] is the program; parts[1..] are its args
    let mut cmd = std::process::Command::new(&parts[0]);
    if parts.len() > 1 {
        cmd.args(&parts[1..]);
    }
    // Set stdin to null
    use std::process::Stdio;
    let output = cmd.stdin(Stdio::null()).output().map_err(|e| {
        WqError::new(WqErrorType::Exec)
            .src(BuiltinEnum::Exec)
            .msg(format!("cannot spawn '{}': {e}", parts[0]))
    })?;
    // On failure include exit info + stderr excerpt
    if !output.status.success() {
        let code = output
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "terminated by signal".into());
        let mut err = String::from_utf8_lossy(&output.stderr).into_owned();
        if err.len() > 8_192 {
            err.truncate(8_192);
            err.push_str("...");
        }
        return Err(WqError::new(WqErrorType::Exec)
            .src(BuiltinEnum::Exec)
            .msg("exec failed")
            .attach_note(format!("exit code: {code}"))
            .attach_note(format!("stderr excerpt: {err}")));
    }
    // Decode and return as List<List<Char>>
    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<Value> = text
        .lines()
        .map(|line| {
            // normalize CRLF: trim trailing '\r' if present
            let ln = if let Some(stripped) = line.strip_suffix('\r') {
                stripped
            } else {
                line
            };
            ln.into_wq_value()
        })
        .collect();
    Ok(Value::List(lines))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::Vm;
    use num_bigint::BigInt;

    #[test]
    fn chr_single_int() {
        let mut vm = Vm::new(vec![]);
        let result = chr(&mut vm, &[Value::Int(65)]).unwrap();
        assert_eq!(result, Value::Char('A'));
    }

    #[test]
    fn chr_int_list() {
        let mut vm = Vm::new(vec![]);
        let result = chr(&mut vm, &[Value::IntList(vec![65, 66])]).unwrap();
        assert_eq!(
            result,
            Value::List(vec![Value::Char('A'), Value::Char('B')])
        );
    }

    #[test]
    fn int_builtin_parses_bigint() {
        let mut vm = Vm::new(vec![]);
        let big = BigInt::from(i64::MAX) + BigInt::from(1);
        let value = into_wq_str(big.to_string());
        let result = int(&mut vm, &[value]).unwrap();
        match result {
            Value::BigInt(n) => assert_eq!(*n, big),
            other => panic!("expected bigint result, got {other:?}"),
        }
    }

    #[test]
    fn ord_single_char() {
        let mut vm = Vm::new(vec![]);
        let result = ord(&mut vm, &[Value::Char('A')]).unwrap();
        assert_eq!(result, Value::Int(65));
    }

    #[test]
    fn ord_char_list() {
        let mut vm = Vm::new(vec![]);
        let input = Value::List(vec![Value::Char('A'), Value::Char('B')]);
        let result = ord(&mut vm, &[input]).unwrap();
        assert_eq!(result, Value::IntList(vec![65, 66]));
    }
}
