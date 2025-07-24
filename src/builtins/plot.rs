use crate::value::valuei::{Value, WqError, WqResult};
use textplots::{Chart, Plot, Shape};

pub fn asciiplot(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "asciiplot expects exactly one sequence argument".into(),
        ));
    }

    let points: Vec<(f32, f32)> = match &args[0] {
        Value::IntArray(arr) if !arr.is_empty() => {
            arr.iter()
               .enumerate()
               .map(|(i, &x)| (i as f32, x as f32))
               .collect()
        }

        Value::List(items) if items.iter().all(|item| {
            if let Value::List(pair) = item {
                pair.len() == 2
                    && pair[0].to_float().is_some()
                    && pair[1].to_float().is_some()
            } else {
                false
            }
        }) => {
            items.iter().map(|item| {
                let pair = match item { Value::List(p) => p, _ => unreachable!() };
                let x = pair[0].to_float().unwrap() as f32;
                let y = pair[1].to_float().unwrap() as f32;
                (x, y)
            }).collect()
        }

        Value::List(items) if items.iter().all(|v| v.to_float().is_some()) && !items.is_empty() => {
            items.iter()
                 .enumerate()
                 .map(|(i, v)| (i as f32, v.to_float().unwrap() as f32))
                 .collect()
        }

        _ => {
            return Err(WqError::TypeError(
                "asciiplot expects a non-empty IntArray, or a List of numbers, or a List of 2‑tuples".into()
            ))
        }
    };

    let xmin = points.iter().map(|&(x, _)| x).fold(f32::INFINITY, f32::min);
    let xmax = points.iter().map(|&(x, _)| x).fold(f32::NEG_INFINITY, f32::max);

    Chart::new(100, 100, xmin, xmax)
        .lineplot(&Shape::Lines(&points))
        .display();

    Ok(Value::Null)
}
