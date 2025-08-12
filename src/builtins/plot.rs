use super::arity_error;
use crate::value::{Value, WqError, WqResult};
use rgb::RGB8;
use textplots::{Chart, ColorPlot, Plot, Shape};

pub fn asciiplot(args: &[Value]) -> WqResult<Value> {
    if args.is_empty() {
        return Err(arity_error(
            "asciiplot",
            "at least one argument",
            args.len(),
        ));
    }

    let mut all_series = Vec::with_capacity(args.len());
    for arg in args {
        let series: Vec<(f32, f32)> =
            match arg {
                Value::IntList(arr) if !arr.is_empty() => arr
                    .iter()
                    .enumerate()
                    .map(|(i, &y)| (i as f32, y as f32))
                    .collect(),
                Value::List(items)
                    if items.iter().all(|it| {
                        if let Value::List(pair) = it {
                            pair.len() == 2
                                && pair[0].to_float().is_some()
                                && pair[1].to_float().is_some()
                        } else {
                            false
                        }
                    }) =>
                {
                    items
                        .iter()
                        .map(|it| {
                            let pair = if let Value::List(pair) = it {
                                pair
                            } else {
                                unreachable!()
                            };
                            let x = pair[0].to_float().unwrap() as f32;
                            let y = pair[1].to_float().unwrap() as f32;
                            (x, y)
                        })
                        .collect()
                }
                Value::List(items)
                    if items.iter().all(|v| v.to_float().is_some()) && !items.is_empty() =>
                {
                    items
                        .iter()
                        .enumerate()
                        .map(|(i, v)| (i as f32, v.to_float().unwrap() as f32))
                        .collect()
                }
                _ => return Err(WqError::TypeError(
                    "each argument must be a list of numbers or a list of 2‑element numeric lists"
                        .into(),
                )),
            };
        all_series.push(series);
    }

    let xmin = all_series
        .iter()
        .flat_map(|s| s.iter().map(|&(x, _)| x))
        .fold(f32::INFINITY, f32::min);
    let xmax = all_series
        .iter()
        .flat_map(|s| s.iter().map(|&(x, _)| x))
        .fold(f32::NEG_INFINITY, f32::max);

    let shapes: Vec<Shape<'_>> = all_series
        .iter()
        .map(|series| Shape::Lines(series.as_slice()))
        .collect();

    let palette = [
        RGB8 { r: 255, g: 0, b: 0 }, // red
        RGB8 { r: 0, g: 255, b: 0 }, // green
        RGB8 { r: 0, g: 0, b: 255 }, // blue
        RGB8 {
            r: 255,
            g: 255,
            b: 0,
        }, // yellow
        RGB8 {
            r: 255,
            g: 0,
            b: 255,
        }, // magenta
    ];

    let mut chart = Chart::new(100, 100, xmin, xmax);
    let mut chart_ref: &mut Chart = &mut chart;
    for (i, shape) in shapes.iter().enumerate() {
        if i == 0 {
            chart_ref = chart_ref.lineplot(shape);
        } else {
            let color = palette[(i - 1) % palette.len()];
            chart_ref = chart_ref.linecolorplot(shape, color);
        }
    }

    chart_ref.display();

    Ok(Value::Null)
}
