use super::arity_error;
use crate::{
    repl::stdio::stdout_println,
    value::{Value, WqResult},
    wqerror::WqError,
};
use rgb::RGB8;
use textplots::{Chart, ColorPlot, Plot, Shape};

pub mod show_table {

    use indexmap::IndexMap;

    use crate::wqerror::WqError;

    use super::*;

    pub fn show_table(args: &[Value]) -> WqResult<Value> {
        if args.len() != 1 {
            return Err(arity_error("showt", "1", args.len()));
        }

        let val = &args[0];

        if let Value::Dict(map) = val {
            let mut wrapped: IndexMap<String, Value> = IndexMap::new();
            for (k, v) in map {
                if matches!(v, Value::List(_) | Value::IntList(_)) {
                    wrapped.insert(k.clone(), v.clone());
                } else {
                    wrapped.insert(k.clone(), Value::List(vec![v.clone()]));
                }
            }
            let wrapped_val = Value::Dict(wrapped);

            if let Some((headers, rows)) = parse_dict_of_lists(&wrapped_val) {
                print_table(&headers, &rows);
                return Ok(Value::unit());
            }
        }

        if let Some((headers, rows)) = parse_list_of_dicts(val) {
            print_table(&headers, &rows);
            return Ok(Value::unit());
        }

        if let Some((headers, rows)) = parse_dict_of_lists(val) {
            print_table(&headers, &rows);
            return Ok(Value::unit());
        }

        if let Some((headers, rows)) = parse_dict_of_dicts(val) {
            print_table(&headers, &rows);
            return Ok(Value::unit());
        }

        Err(WqError::DomainError(
            "`showt`: invalid table, expected (a dict), (a list of dicts), (a dict of lists), or (a dict of dicts)"
                .to_string(),
        ))
    }

    fn parse_list_of_dicts(val: &Value) -> Option<(Vec<String>, Vec<Vec<String>>)> {
        if let Value::List(rows) = val
            && rows.iter().all(|r| matches!(r, Value::Dict(_)))
        {
            let mut headers: Vec<String> = Vec::new();
            for row in rows {
                if let Value::Dict(map) = row {
                    for k in map.keys() {
                        if !headers.contains(k) {
                            headers.push(k.clone());
                        }
                    }
                }
            }
            headers.sort();
            let mut data = Vec::new();
            for row in rows {
                if let Value::Dict(map) = row {
                    let mut r = Vec::new();
                    for h in &headers {
                        if let Some(v) = map.get(h) {
                            r.push(v.to_string());
                        } else {
                            r.push(String::new());
                        }
                    }
                    data.push(r);
                }
            }
            return Some((headers, data));
        }
        None
    }

    fn parse_dict_of_lists(val: &Value) -> Option<(Vec<String>, Vec<Vec<String>>)> {
        if let Value::Dict(map) = val
            && map
                .values()
                .all(|v| matches!(v, Value::List(_) | Value::IntList(_)))
        {
            let mut headers: Vec<String> = map.keys().cloned().collect();
            headers.sort();
            let nrows = map
                .values()
                .filter_map(|v| match v {
                    Value::List(items) => Some(items.len()),
                    Value::IntList(items) => Some(items.len()),
                    _ => None,
                })
                .max()
                .unwrap_or(0);

            let mut data = Vec::new();
            for i in 0..nrows {
                let mut row = Vec::new();
                for h in &headers {
                    if let Some(value) = map.get(h) {
                        match value {
                            Value::List(items) => {
                                if let Some(v) = items.get(i) {
                                    row.push(v.to_string());
                                } else {
                                    row.push(String::new());
                                }
                            }
                            Value::IntList(items) => {
                                if let Some(v) = items.get(i) {
                                    row.push(v.to_string());
                                } else {
                                    row.push(String::new());
                                }
                            }
                            _ => row.push(String::new()),
                        }
                    } else {
                        row.push(String::new());
                    }
                }
                data.push(row);
            }
            return Some((headers, data));
        }
        None
    }

    fn parse_dict_of_dicts(val: &Value) -> Option<(Vec<String>, Vec<Vec<String>>)> {
        if let Value::Dict(map) = val
            && map.values().all(|v| matches!(v, Value::Dict(_)))
        {
            let mut row_names: Vec<String> = map.keys().cloned().collect();
            row_names.sort();

            let mut columns: Vec<String> = Vec::new();
            for v in map.values() {
                if let Value::Dict(inner) = v {
                    for k in inner.keys() {
                        if !columns.contains(k) {
                            columns.push(k.clone());
                        }
                    }
                }
            }
            columns.sort();

            let mut headers = Vec::with_capacity(columns.len() + 1);
            headers.push(String::new());
            headers.extend(columns.clone());

            let mut data = Vec::new();
            for row_name in &row_names {
                let mut row = Vec::new();
                row.push(row_name.clone());
                if let Some(Value::Dict(inner)) = map.get(row_name) {
                    for col in &columns {
                        if let Some(v) = inner.get(col) {
                            row.push(v.to_string());
                        } else {
                            row.push(String::new());
                        }
                    }
                } else {
                    for _ in &columns {
                        row.push(String::new());
                    }
                }
                data.push(row);
            }
            return Some((headers, data));
        }
        None
    }

    fn print_table(headers: &[String], rows: &[Vec<String>]) {
        let mut table = Vec::new();
        if !headers.is_empty() {
            table.push(headers.to_vec());
        }
        table.extend_from_slice(rows);

        let ncols = table.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut widths = vec![0usize; ncols];
        for row in &table {
            for (j, cell) in row.iter().enumerate() {
                if widths[j] < cell.len() {
                    widths[j] = cell.len();
                }
            }
        }

        for row in table {
            let mut parts = Vec::new();
            for (j, cell) in row.iter().enumerate() {
                parts.push(format!("{:<width$}", cell, width = widths[j]));
            }
            println!("{}", parts.join(" ").trim_end());
        }
    }
}

pub fn asciiplot(args: &[Value]) -> WqResult<Value> {
    if args.is_empty() {
        return Err(arity_error("asciiplot", "at least 1", args.len()));
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
                _ => return Err(WqError::DomainError(
                    "`asciiplot`: expected each arg to be (a list of numbers) or (a list of 2‑element numeric lists)"
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

    // chart_ref.display();
    chart_ref.axis();
    chart_ref.figures();
    stdout_println(chart_ref.to_string().as_str());

    Ok(Value::unit())
}
