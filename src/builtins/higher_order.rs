use crate::{
    builtins::{
        BuiltinEnum,
        wqerror_helper::{check_arity, type_mismatch},
    },
    value::{Value, WqResult},
    vm::Vm,
};

// map[f;xs], map[f;d;xs]
pub fn map(vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    #[inline]
    fn eff_layers(raw_d: &Value, total_depth: i64) -> Option<i64> {
        match raw_d {
            // non-negative: go min(d, D) layers
            Value::Int(n) if *n >= 0 => Some((*n).min(total_depth)),
            // negative: “cut |d| from xs.depth()” -> L = max(0, D + d)
            Value::Int(n) => Some((total_depth + *n).max(0)),
            // +inf: go fully (atoms only) -> L = D
            Value::Float(n) if n.is_infinite() && n.is_sign_positive() => Some(total_depth),
            // -inf: apply at root -> L = 0
            Value::Float(n) if n.is_infinite() && n.is_sign_negative() => Some(0),
            _ => None,
        }
    }

    fn _map(vm: &mut Vm, d: &Value, f: &Value, xs: &Value) -> WqResult<Value> {
        let el = match eff_layers(d, xs.depth()) {
            Some(l) => l,
            None => return Err(type_mismatch(BuiltinEnum::Map, 0, "int, inf or -inf", d)),
        };
        // atoms are always leaves; stop after traversing L layers from the root
        let is_leaf = |depth_from_root: usize, v: &Value| -> bool {
            v.is_atom() || (depth_from_root as i64) >= el
        };
        let op1 = |v: &Value| vm.call_value(f, std::slice::from_ref(v));
        xs.bc1_until(is_leaf, op1)
            .map_err(|e| e.into_wqerror().src(BuiltinEnum::Map))
    }

    check_arity(BuiltinEnum::Map, [2, 3], args)?;
    match args.len() {
        2 => {
            let (f, xs) = (&args[0], &args[1]);
            _map(vm, &Value::Int(1), f, xs)
        }
        3 => {
            let (d, f, xs) = (&args[0], &args[1], &args[2]);
            _map(vm, d, f, xs)
        }
        _ => unreachable!(),
    }
}

pub fn zipw(vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    #[inline]
    fn eff_layers_2(raw_d: &Value, dx: i64, dy: i64) -> Option<i64> {
        let dmax = dx.max(dy);
        match raw_d {
            // non-negative: go min(d, Dmax) layers
            Value::Int(n) if *n >= 0 => Some((*n).min(dmax)),
            // negative: cut |d| from max depth, L = max(0, Dmax + d)
            Value::Int(n) => Some((dmax + *n).max(0)),
            // +inf: go fully
            Value::Float(n) if n.is_infinite() && n.is_sign_positive() => Some(dmax),
            // -inf: treat as atom (apply at root)
            Value::Float(n) if n.is_infinite() && n.is_sign_negative() => Some(0),
            _ => None,
        }
    }

    fn _zipw(vm: &mut Vm, d: &Value, f: &Value, xs: &Value, ys: &Value) -> WqResult<Value> {
        let el = match eff_layers_2(d, xs.depth(), ys.depth()) {
            Some(l) => l,
            None => return Err(type_mismatch(BuiltinEnum::ZipW, 0, "int, inf or -inf", d)),
        };
        // atoms are always leaves; stop after traversing L layers from the root
        let is_leaf = |depth_from_root: usize, a: &Value, b: &Value| {
            (a.is_atom() && b.is_atom()) || (depth_from_root as i64) >= el
        };
        let op2 = |a: &Value, b: &Value| {
            let args = [a.clone(), b.clone()];
            vm.call_value(f, &args)
        };
        xs.bc2_until(ys, is_leaf, op2)
            .map_err(|e| e.into_wqerror().src(BuiltinEnum::ZipW))
    }

    check_arity(BuiltinEnum::ZipW, [3, 4], args)?;
    match args.len() {
        3 => {
            let (f, xs, ys) = (&args[0], &args[1], &args[2]);
            _zipw(vm, &Value::Int(1), f, xs, ys)
        }
        4 => {
            let (d, f, xs, ys) = (&args[0], &args[1], &args[2], &args[3]);
            _zipw(vm, d, f, xs, ys)
        }
        _ => unreachable!(),
    }
}

pub fn fold(vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BuiltinEnum::Fold, [2, 3], args)?;
    match args.len() {
        2 => {
            let func = &args[0];
            match &args[1] {
                Value::IntList(items) => {
                    if items.is_empty() {
                        return Ok(Value::unit());
                    }
                    let mut acc = Value::Int(items[0]);
                    for &x in &items[1..] {
                        acc = vm.call_value(func, &[acc, Value::Int(x)])?;
                    }
                    Ok(acc)
                }
                Value::List(items) => {
                    if items.is_empty() {
                        return Ok(Value::unit());
                    }
                    let mut iter = items.iter();
                    let mut acc = iter.next().unwrap().clone();
                    for it in iter {
                        acc = vm.call_value(func, &[acc, it.clone()])?;
                    }
                    Ok(acc)
                }
                Value::Dict(map) => {
                    if map.is_empty() {
                        return Ok(Value::unit());
                    }
                    let mut iter = map.values();
                    let mut acc = iter.next().unwrap().clone();
                    for it in iter {
                        acc = vm.call_value(func, &[acc, it.clone()])?;
                    }
                    Ok(acc)
                }
                other => Ok(other.clone()),
            }
        }
        3 => {
            let func = &args[0];
            let mut acc = args[1].clone();
            match &args[2] {
                Value::IntList(items) => {
                    for &x in items {
                        acc = vm.call_value(func, &[acc, Value::Int(x)])?;
                    }
                    Ok(acc)
                }
                Value::List(items) => {
                    for it in items {
                        acc = vm.call_value(func, &[acc, it.clone()])?;
                    }
                    Ok(acc)
                }
                Value::Dict(map) => {
                    for it in map.values() {
                        acc = vm.call_value(func, &[acc, it.clone()])?;
                    }
                    Ok(acc)
                }
                other => Ok(other.clone()),
            }
        }
        _ => unreachable!(),
    }
}

pub fn scan(vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BuiltinEnum::Scan, [2, 3], args)?;
    match args.len() {
        2 => {
            let f = &args[0];
            match &args[1] {
                Value::IntList(xs) => {
                    if xs.is_empty() {
                        return Ok(Value::unit());
                    }
                    let mut results: Vec<Value> = Vec::with_capacity(xs.len());
                    let mut acc = Value::Int(xs[0]);
                    results.push(acc.clone());
                    for &x in &xs[1..] {
                        acc = vm.call_value(f, &[acc, Value::Int(x)])?;
                        results.push(acc.clone());
                    }
                    Ok(Value::from_items(results))
                }
                Value::List(xs) => {
                    if xs.is_empty() {
                        return Ok(Value::unit());
                    }
                    let mut results: Vec<Value> = Vec::with_capacity(xs.len());
                    let mut acc = xs[0].clone();
                    results.push(acc.clone());
                    for x in &xs[1..] {
                        acc = vm.call_value(f, &[acc, x.clone()])?;
                        results.push(acc.clone());
                    }
                    Ok(Value::from_items(results))
                }
                Value::Dict(map) => {
                    // return a list
                    if map.is_empty() {
                        return Ok(Value::unit());
                    }
                    let mut results: Vec<Value> = Vec::with_capacity(map.len());
                    let mut iter = map.values();
                    let mut acc = iter.next().unwrap().clone();
                    results.push(acc.clone());
                    for v in iter {
                        acc = vm.call_value(f, &[acc, v.clone()])?;
                        results.push(acc.clone());
                    }
                    Ok(Value::from_items(results))
                }
                other => Ok(other.clone()),
            }
        }
        3 => {
            let f = &args[0];
            let mut acc = args[1].clone();
            match &args[2] {
                Value::IntList(xs) => {
                    let mut results: Vec<Value> = Vec::with_capacity(xs.len());
                    for &x in xs {
                        acc = vm.call_value(f, &[acc, Value::Int(x)])?;
                        results.push(acc.clone());
                    }
                    Ok(Value::from_items(results))
                }
                Value::List(xs) => {
                    let mut results: Vec<Value> = Vec::with_capacity(xs.len());
                    for x in xs {
                        acc = vm.call_value(f, &[acc, x.clone()])?;
                        results.push(acc.clone());
                    }
                    Ok(Value::List(results))
                }
                Value::Dict(map) => {
                    // return a list
                    let mut results: Vec<Value> = Vec::with_capacity(map.len());
                    for v in map.values() {
                        acc = vm.call_value(f, &[acc, v.clone()])?;
                        results.push(acc.clone());
                    }
                    Ok(Value::from_items(results))
                }
                other => Ok(other.clone()),
            }
        }
        _ => unreachable!(),
    }
}
