use std::process::Command;

use crate::{
    builtins::values_to_strings,
    value::valuei::{Value, WqError, WqResult},
};
use std::io::{Write, stdout};

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
                    println!("{s}");
                } else {
                    print!("{s}");
                    let _ = stdout().flush();
                }
            }
            Value::Char(c) => {
                if newline {
                    println!("{c}");
                } else {
                    print!("{c}");
                    let _ = stdout().flush();
                }
            }
            other => {
                if newline {
                    println!("{other}");
                } else {
                    print!("{other}");
                    let _ = stdout().flush();
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

pub mod show_table {

    use std::collections::HashMap;

    use super::*;

    pub fn show_table(args: &[Value]) -> WqResult<Value> {
        if args.len() != 1 {
            return Err(WqError::FnArgCountMismatchError(
                "showt expects 1 argument".to_string(),
            ));
        }

        let val = &args[0];

        if let Value::Dict(map) = val {
            let mut wrapped: HashMap<String, Value> = HashMap::new();
            for (k, v) in map {
                if let Value::List(_) = v {
                    wrapped.insert(k.clone(), v.clone());
                } else {
                    wrapped.insert(k.clone(), Value::List(vec![v.clone()]));
                }
            }
            let wrapped_val = Value::Dict(wrapped);

            if let Some((headers, rows)) = parse_dict_of_lists(&wrapped_val) {
                print_table(&headers, &rows);
                return Ok(Value::Null);
            }
        }

        if let Some((headers, rows)) = parse_list_of_dicts(val) {
            print_table(&headers, &rows);
            return Ok(Value::Null);
        }

        if let Some((headers, rows)) = parse_dict_of_lists(val) {
            print_table(&headers, &rows);
            return Ok(Value::Null);
        }

        if let Some((headers, rows)) = parse_dict_of_dicts(val) {
            print_table(&headers, &rows);
            return Ok(Value::Null);
        }

        Err(WqError::RuntimeError(
            "failed parsing table, expected a list of dicts, a dict of lists, or a dict of dicts"
                .to_string(),
        ))
    }

    fn parse_list_of_dicts(val: &Value) -> Option<(Vec<String>, Vec<Vec<String>>)> {
        if let Value::List(rows) = val {
            if rows.iter().all(|r| matches!(r, Value::Dict(_))) {
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
        }
        None
    }

    fn parse_dict_of_lists(val: &Value) -> Option<(Vec<String>, Vec<Vec<String>>)> {
        if let Value::Dict(map) = val {
            if map.values().all(|v| matches!(v, Value::List(_))) {
                let mut headers: Vec<String> = map.keys().cloned().collect();
                headers.sort();

                let nrows = map
                    .values()
                    .filter_map(|v| match v {
                        Value::List(items) => Some(items.len()),
                        _ => None,
                    })
                    .max()
                    .unwrap_or(0);

                let mut data = Vec::new();
                for i in 0..nrows {
                    let mut row = Vec::new();
                    for h in &headers {
                        if let Some(Value::List(items)) = map.get(h) {
                            if let Some(v) = items.get(i) {
                                row.push(v.to_string());
                            } else {
                                row.push(String::new());
                            }
                        } else {
                            row.push(String::new());
                        }
                    }
                    data.push(row);
                }

                return Some((headers, data));
            }
        }
        None
    }

    fn parse_dict_of_dicts(val: &Value) -> Option<(Vec<String>, Vec<Vec<String>>)> {
        if let Value::Dict(map) = val {
            if map.values().all(|v| matches!(v, Value::Dict(_))) {
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

pub fn exec(args: &[Value]) -> WqResult<Value> {
    if args.is_empty() {
        return Err(WqError::DomainError(
            "exec expects at least 1 argument".into(),
        ));
    }
    let parts = values_to_strings(args)?;

    let output = if parts.len() == 1 && parts[0].contains(char::is_whitespace) {
        if cfg!(windows) {
            Command::new("cmd")
                .arg("/C")
                .arg(&parts[0])
                .output()
                .map_err(|e| WqError::RuntimeError(e.to_string()))?
        } else {
            Command::new("sh")
                .arg("-c")
                .arg(&parts[0])
                .output()
                .map_err(|e| WqError::RuntimeError(e.to_string()))?
        }
    } else {
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&parts[0]);
            c
        } else {
            Command::new(&parts[0])
        };
        if parts.len() > 1 {
            cmd.args(&parts[1..]);
        }
        cmd.output()
            .map_err(|e| WqError::RuntimeError(e.to_string()))?
    };

    if !output.status.success() {
        let code = output
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "unknown".into());
        return Err(WqError::RuntimeError(format!("exec failed (exit {code})")));
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
