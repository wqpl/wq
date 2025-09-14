use crate::{
    builtins::{BuiltinEnum as BE, wqerr_ext::check_arity},
    value::{Excerpt as _, Value, WqResult},
    vm::Vm,
    wqerr::{WqErr, WqErrType},
};

pub fn alloc(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Alloc, [1], args)?;
    let dims = parse_shape(&args[0]).map_err(|e| e.src(BE::Alloc).at_arg(0))?;
    let mut generator = || Value::Int(0);
    Ok(build_from_parsed_shape(&dims, &mut generator))
}

pub fn till(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Till, [1], args)?;
    if let Value::Int(n) = args[0] {
        if n < 0 {
            return Err(WqErr::new(WqErrType::Domain)
                .src(BE::Till)
                .msg("invalid shape")
                .attach_note("shape is positive int or list<positive int>"));
        }
        // let mut cache = INTS_CACHE.lock().unwrap();
        // if let Some(v) = cache.get(&n) {
        //     return Ok(v.clone());
        // }
        let val = Value::IntList((0..n).collect());
        // cache.insert(n, val.clone());
        return Ok(val);
    }
    let dims = parse_shape(&args[0]).map_err(|e| e.src(BE::Till).at_arg(0))?;
    if dims.is_empty() {
        return Ok(Value::Int(0));
    }
    let mut next = 0i64;
    let mut generator = || {
        let v = Value::Int(next);
        next += 1;
        v
    };
    Ok(build_from_parsed_shape(&dims, &mut generator))
}

pub fn iota(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    fn build_coords(dims: &[usize], prefix: &mut Vec<i64>) -> WqResult<Value> {
        if dims.is_empty() {
            return Ok(Value::IntList(prefix.clone()));
        }
        let n = dims[0];
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            prefix.push(i.try_into().map_err(|e| {
                WqErr::new(WqErrType::NumericOverflow)
                    .src(BE::Iota)
                    .attach_note(e)
            })?);
            out.push(build_coords(&dims[1..], prefix)?);
            prefix.pop();
        }
        Ok(Value::List(out))
    }

    check_arity(BE::Iota, [1], args)?;
    match &args[0] {
        // 1D case: simple range 0..n-1
        Value::Int(n) => {
            if *n < 0 {
                Err(WqErr::new(WqErrType::Domain)
                    .src(BE::Iota)
                    .msg("invalid shape")
                    .attach_note("shape is positive int or list<positive int>"))
            } else {
                Ok(Value::IntList((0..*n).collect()))
            }
        }
        // Multidimensional: nested grid of coordinate vectors, shaped by dims
        _ => {
            let dims = parse_shape(&args[0]).map_err(|e| e.src(BE::Iota).at_arg(0))?;
            if dims.is_empty() {
                // Preserve existing behavior for empty shape
                return Ok(Value::IntList(vec![]));
            }
            let mut prefix = Vec::<i64>::with_capacity(dims.len());
            Ok(build_coords(&dims, &mut prefix)?)
        }
    }
}

pub fn reshape(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Reshape, [2], args)?;
    let dims = parse_shape(&args[0]).map_err(|e| e.src(BE::Reshape).at_arg(0))?;
    let flattened = args[1].flatten();
    if flattened.is_empty() {
        let mut generator = || Value::Int(0);
        return Ok(build_from_parsed_shape(&dims, &mut generator));
    }
    let n = flattened.len();
    let mut i = 0usize;
    let mut generator = || {
        let v = flattened[i].clone();
        i += 1;
        if i == n {
            i = 0;
        }
        v
    };
    Ok(build_from_parsed_shape(&dims, &mut generator))
}

pub fn wq_where(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    const EXP: &str = "list whose leaves are int or bool";

    fn fmt_path(path: &[i64]) -> String {
        if path.is_empty() {
            "[]".to_string()
        } else {
            path.iter().map(|i| format!("[{}]", i)).collect()
        }
    }

    fn fail_type<T>(got: &Value, path: &[i64]) -> WqResult<T> {
        Err(WqErr::new(WqErrType::Domain)
            .src(BE::Where)
            .msg(format!("expected {EXP}"))
            .at_arg(0)
            .attach_note(format!(
                "unexpected {} at path {}",
                got.type_name(),
                fmt_path(path)
            )))
    }

    // Helper used by the nested collector: emit Int for length-1 paths, IntList otherwise.
    fn push_coord(out: &mut Vec<Value>, coord: Vec<i64>) {
        if coord.len() == 1 {
            out.push(Value::Int(coord[0]));
        } else {
            out.push(Value::IntList(coord));
        }
    }

    // Helper: flat 1D case (list of ints/bools).
    fn where_1d_list(items: &[Value]) -> WqResult<Value> {
        let mut indices = Vec::new();
        for (i, item) in items.iter().enumerate() {
            match item {
                Value::Int(n) if *n != 0 => indices.push(i.try_into().map_err(|e| {
                    WqErr::new(WqErrType::NumericOverflow)
                        .src(BE::Where)
                        .attach_note(e)
                })?),
                Value::Bool(b) if *b => indices.push(i.try_into().map_err(|e| {
                    WqErr::new(WqErrType::NumericOverflow)
                        .src(BE::Where)
                        .attach_note(e)
                })?),
                Value::Int(_) | Value::Bool(_) => {}
                _ => {
                    // precise path for flat case: [i]
                    return fail_type::<Value>(
                        item,
                        &[i.try_into().map_err(|e| {
                            WqErr::new(WqErrType::NumericOverflow)
                                .src(BE::Where)
                                .attach_note(e)
                        })?],
                    );
                }
            }
        }
        Ok(Value::IntList(indices))
    }

    // Recursively collect coordinates. Single-index paths become Int atoms.
    fn collect_coords(v: &Value, prefix: &mut Vec<i64>, out: &mut Vec<Value>) -> WqResult<()> {
        match v {
            Value::List(items) => {
                for (i, item) in items.iter().enumerate() {
                    prefix.push(i.try_into().map_err(|e| {
                        WqErr::new(WqErrType::NumericOverflow)
                            .src(BE::Where)
                            .attach_note(e)
                    })?);
                    collect_coords(item, prefix, out)?;
                    prefix.pop();
                }
                Ok(())
            }
            Value::IntList(items) => {
                for (i, &n) in items.iter().enumerate() {
                    if n != 0 {
                        let mut coord = prefix.clone();
                        coord.push(i.try_into().map_err(|e| {
                            WqErr::new(WqErrType::NumericOverflow)
                                .src(BE::Where)
                                .attach_note(e)
                        })?);
                        push_coord(out, coord);
                    }
                }
                Ok(())
            }
            Value::Int(n) => {
                if *n != 0 {
                    push_coord(out, prefix.clone());
                }
                Ok(())
            }
            Value::Bool(b) => {
                if *b {
                    push_coord(out, prefix.clone());
                }
                Ok(())
            }
            // Anything else: report precise nested path
            other => fail_type::<()>(other, prefix),
        }
    }

    check_arity(BE::Where, [1], args)?;
    match &args[0] {
        // Flat vector of ints -> indices as list of ints
        Value::IntList(items) => {
            let mut indices = Vec::new();
            for (i, n) in items.iter().enumerate() {
                if *n != 0 {
                    indices.push(i.try_into().map_err(|e| {
                        WqErr::new(WqErrType::NumericOverflow)
                            .src(BE::Where)
                            .attach_note(e)
                    })?);
                }
            }
            Ok(Value::IntList(indices))
        }
        // Generic list: nested -> coordinate vectors (with atom for length-1), else flat ints/bools.
        Value::List(items) => {
            let has_nested = items
                .iter()
                .any(|x| matches!(x, Value::List(_) | Value::IntList(_)));
            if has_nested {
                let mut out = Vec::new();
                let mut pref = Vec::new();
                collect_coords(&args[0], &mut pref, &mut out)?;
                Ok(Value::List(out))
            } else {
                where_1d_list(items)
            }
        }
        other => Err(WqErr::new(WqErrType::Domain)
            .msg(format!("expected {EXP}"))
            .at_arg(0)
            .attach_note(format!(
                "got {} of type {}",
                other.excerpt(),
                other.type_name()
            ))),
    }
}

fn parse_shape(v: &Value) -> WqResult<Vec<usize>> {
    const EXP: &str = "positive int or list<positive int>";
    match v {
        Value::Int(n) => {
            if *n < 0 {
                Err(WqErr::new(WqErrType::Domain).msg(EXP))
            } else {
                Ok(vec![*n as usize])
            }
        }
        Value::IntList(dims) => dims
            .iter()
            .enumerate()
            .map(|(i, &d)| {
                if d < 0 {
                    Err(WqErr::new(WqErrType::Domain)
                        .msg(EXP)
                        .attach_note(format!("at index {i}"))
                        .attach_note(format!("value excerpt is {}", d.excerpt())))
                } else {
                    Ok(d as usize)
                }
            })
            .collect(),
        Value::List(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| match &v {
                Value::Int(n) if *n > 0 => Ok(*n as usize),
                _ => Err(WqErr::new(WqErrType::Domain)
                    .msg(EXP)
                    .attach_note(format!("at index {i}"))
                    .attach_note(format!("value excerpt is {}", v.excerpt()))),
            })
            .collect(),
        _ => Err(WqErr::new(WqErrType::Domain).msg(EXP)),
    }
}

fn build_from_parsed_shape<F>(shape: &[usize], next: &mut F) -> Value
where
    F: FnMut() -> Value,
{
    match shape.len() {
        0 => next(),
        1 => {
            let n = shape[0];
            if n == 0 {
                return Value::IntList(vec![]);
            }
            let mut tmp: Vec<Value> = Vec::with_capacity(n);
            for _ in 0..n {
                let v = next();
                tmp.push(v);
            }
            Value::from_items(tmp)
        }
        _ => {
            let m = shape[0];
            let mut out = Vec::with_capacity(m);
            for _ in 0..m {
                out.push(build_from_parsed_shape(&shape[1..], next));
            }
            Value::List(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::Vm;

    #[test]
    fn ints_empty_shape() {
        let mut vm = Vm::new(vec![]);
        assert_eq!(
            till(&mut vm, &[Value::List(vec![])]).unwrap(),
            Value::Int(0)
        );
    }

    #[test]
    fn iota_zero() {
        let mut vm = Vm::new(vec![]);
        assert_eq!(
            iota(&mut vm, &[Value::Int(0)]).unwrap(),
            Value::IntList(vec![])
        );
    }
}
