use std::fmt::Display;

use crate::{
    builtins::BuiltinEnum,
    value::{Value, WqResult},
    wqerror::{WqError, WqErrorType},
};

pub fn type_mismatch(builtin: BuiltinEnum, pos: usize, expected: &str, got: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .src(builtin)
        .msg(format!("expected {expected}"))
        .at_arg(pos)
        .got1(got)
        .attach_note(format!("usage: {}", builtin.usage()))
}

pub fn check_arity(
    builtin: BuiltinEnum,
    arity: impl AsRef<[usize]>,
    args: &[Value],
) -> WqResult<()> {
    let arity = arity.as_ref();
    let n = args.len();
    if !arity.contains(&n) {
        let expected = match arity {
            [] => return Ok(()),
            [one] => format!("{one}"),
            [a, b] => format!("{a} or {b}"),
            many => {
                let mut parts: Vec<String> = many.iter().map(|x| x.to_string()).collect();
                let last = parts.pop().unwrap();
                if parts.is_empty() {
                    last
                } else {
                    format!("{}, or {}", parts.join(", "), last)
                }
            }
        };
        let plural = if arity.len() == 1 && arity[0] == 1 {
            "arg"
        } else {
            "args"
        };
        return Err(WqError::new(WqErrorType::Arity)
            .src(builtin)
            .msg(format!("expected {expected} {plural}, got {n}"))
            .attach_note(builtin.usage()));
    }
    Ok(())
}

impl Display for BuiltinEnum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bfn '{}'", self.name())
    }
}

impl WqError {
    pub fn at_arg(mut self, pos: usize) -> Self {
        self = self.attach_note(format!("at arg #{}", pos));
        self
    }
}
