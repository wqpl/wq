use crate::{
    builtins::{BuiltinEnum as BE, wqerror_helper::check_arity},
    stdio::stdout_println,
    value::{Value, WqResult},
    vm::Vm,
    wqerror::{WqError, WqErrorType},
};

use indexmap::IndexMap;

pub fn show_table(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Showtable, [1], args)?;
    let val = &args[0];
    if let Value::Dict(map) = val {
        let mut wrapped: IndexMap<String, Value> = IndexMap::new();
        for (k, v) in map.iter() {
            if matches!(v, Value::List(_) | Value::IntList(_)) {
                wrapped.insert(k.clone(), v.clone());
            } else {
                wrapped.insert(k.clone(), Value::List(vec![v.clone()]));
            }
        }
        let wrapped_val = Value::Dict(Box::new(wrapped));
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
    Err(WqError::new(WqErrorType::Domain).src(BE::Showtable).msg("invalid table, expected (a dict), (a list of dicts), (a dict of lists), or (a dict of dicts)"))
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
        let headers: Vec<String> = map.keys().cloned().collect();
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
        let row_names: Vec<String> = map.keys().cloned().collect();
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
        stdout_println(parts.join(" ").trim_end());
    }
}
