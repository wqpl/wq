use std::cmp::{max, min};

use colored::{Color, Colorize};
use indexmap::IndexMap;

use crate::builtins::{BuiltinContext, BuiltinEnum as BE, BuiltinFnArgs, check_named_args};
use crate::cas::{infer_single_cas_var, substitute_cas};
use crate::session::stdio::wqstdout_println;
use crate::value::{Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

pub(crate) fn asciiplot(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_named_args(&args, BE::Asciiplot, super::super::ASCIIPLOT_NAMED_ARGS)?;
    if args.is_empty() {
        return Err(WqError::new(WqErrorType::Arity)
            .src(BE::Asciiplot)
            .msg("expected 1 or more args")
            .attach_note(BE::Asciiplot.usage()));
    }
    let mut opts = PlotOptions::default();
    let explicit_size = opts.apply_from_named(&args)?;
    #[cfg(target_arch = "wasm32")]
    let _ = explicit_size;
    // Apply theme preset before terminal sizing
    let theme = opts.theme.clone();
    if let Some(ref t) = theme {
        opts.apply_theme(t);
    }
    // Terminal auto-size: only when width/height/size not explicitly set
    #[cfg(not(target_arch = "wasm32"))]
    if !explicit_size && let Some((term_w, term_h)) = terminal_size::terminal_size() {
        let tw = term_w.0 as usize;
        let th = term_h.0 as usize;
        opts.width = tw.saturating_sub(8).clamp(40, 200);
        opts.height = th.saturating_sub(6).clamp(10, 60);
    }
    // Collect series configs
    let mut configs: Vec<SeriesConfig> = Vec::new();
    for arg in args {
        configs.extend(parse_series_arg(&arg, &opts)?);
    }
    if configs.is_empty() {
        return Err(WqError::new(WqErrorType::Domain).src(BE::Asciiplot).msg("expected each arg to be (a list of numbers) or (a list of 2‑element numeric lists)").attach_note(
                "e.g. (1;2;3), ((1;2);(2;4))"));
    }
    let mut all_series: Vec<PlotSeries> = Vec::with_capacity(configs.len());
    for config in &configs {
        let sampled = match &config.data {
            SeriesData::Raw(xy) => xy.clone(),
            SeriesData::Callable(func) => sample_callable_series(vm, func, &opts, config.xlim)?,
            SeriesData::Cas(expr) => sample_cas_series(expr, &opts, config.xlim)?,
        };
        all_series.push(PlotSeries {
            points: sampled.points,
            breaks_after: sampled.breaks_after,
            symbol: config.symbol,
            mode: config.mode,
            label: config.label.clone(),
        });
    }
    let rendered = render_ascii_plot(&all_series, &opts);
    wqstdout_println(rendered);
    Ok(Value::unit())
}

#[derive(Clone)]
struct SeriesConfig {
    data: SeriesData,
    xlim: Option<(f64, f64)>,
    symbol: Option<char>,
    mode: Option<PlotMode>,
    label: Option<String>,
}

#[derive(Clone)]
enum SeriesData {
    Raw(SampledSeries<f64>),
    Callable(Value),
    Cas(Value),
}

struct PlotSeries {
    points: Vec<(f64, f64)>,
    breaks_after: Vec<usize>,
    symbol: Option<char>,
    mode: Option<PlotMode>,
    label: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct SampledSeries<T> {
    points: Vec<(f64, T)>,
    breaks_after: Vec<usize>,
}

impl<T> SampledSeries<T> {
    fn new() -> Self {
        Self {
            points: Vec::new(),
            breaks_after: Vec::new(),
        }
    }

    fn from_points(points: Vec<(f64, T)>) -> Self {
        Self {
            points,
            breaks_after: Vec::new(),
        }
    }

    fn push(&mut self, point: (f64, T), break_before: bool) {
        if break_before {
            self.break_before_next();
        }
        self.points.push(point);
    }

    fn break_before_next(&mut self) {
        let Some(idx) = self.points.len().checked_sub(1) else {
            return;
        };
        if self.breaks_after.last().copied() != Some(idx) {
            self.breaks_after.push(idx);
        }
    }
}

fn parse_series_arg(arg: &Value, opts: &PlotOptions) -> WqResult<Vec<SeriesConfig>> {
    match arg {
        Value::IntList(arr) if !arr.is_empty() => Ok(vec![SeriesConfig {
            data: SeriesData::Raw(SampledSeries::from_points(
                arr.iter()
                    .enumerate()
                    .map(|(i, &y)| (i as f64, y as f64))
                    .collect(),
            )),
            xlim: None,
            symbol: None,
            mode: None,
            label: None,
        }]),
        Value::List(items)
            if items.iter().all(|it| {
                if let Value::List(ref pair) = *it {
                    pair.len() == 2 && pair[0].as_f64().is_some() && pair[1].as_f64().is_some()
                } else {
                    false
                }
            }) && !items.is_empty() =>
        {
            Ok(vec![SeriesConfig {
                data: SeriesData::Raw(SampledSeries::from_points(
                    items
                        .iter()
                        .map(|it| {
                            let Value::List(ref pair) = *it else {
                                unreachable!();
                            };
                            (
                                pair[0].as_f64().expect("guard checked x is numeric"),
                                pair[1].as_f64().expect("guard checked y is numeric"),
                            )
                        })
                        .collect(),
                )),
                xlim: None,
                symbol: None,
                mode: None,
                label: None,
            }])
        }
        Value::List(items) if items.iter().all(|v| v.as_f64().is_some()) && !items.is_empty() => {
            Ok(vec![SeriesConfig {
                data: SeriesData::Raw(SampledSeries::from_points(
                    items
                        .iter()
                        .enumerate()
                        .map(|(i, y)| {
                            (
                                i as f64,
                                y.as_f64().expect("guard checked list item is numeric"),
                            )
                        })
                        .collect(),
                )),
                xlim: None,
                symbol: None,
                mode: None,
                label: None,
            }])
        }
        Value::List(_) => {
            if let Some(table) = parse_table_arg(arg) {
                table_series_configs(table, opts)
            } else {
                Err(expected_series_arg_error())
            }
        }
        _ if arg.is_cas_expr() => Ok(vec![SeriesConfig {
            data: SeriesData::Cas(arg.clone()),
            xlim: None,
            symbol: None,
            mode: None,
            label: None,
        }]),
        Value::Dict(map) => {
            if let Some(fn_val) = map.get("fn")
                && (fn_val.is_callable() || fn_val.is_cas_expr())
            {
                let xlim = map.get("xlim").and_then(pair_as_f64);
                let symbol = map.get("symbol").and_then(|v| match v {
                    Value::Char(c) => Some(*c),
                    _ => v
                        .to_rust_string_with_note()
                        .ok()
                        .and_then(|s| s.chars().next()),
                });
                let mode = map.get("mode").and_then(parse_plot_mode);
                let label = map
                    .get("label")
                    .and_then(|v| v.to_rust_string_with_note().ok());

                let data = if fn_val.is_callable() {
                    SeriesData::Callable(fn_val.clone())
                } else {
                    SeriesData::Cas(fn_val.clone())
                };

                Ok(vec![SeriesConfig {
                    data,
                    xlim,
                    symbol,
                    mode,
                    label,
                }])
            } else if let Some(table) = parse_table_arg(arg) {
                table_series_configs(table, opts)
            } else if map.contains_key("fn") {
                Err(WqError::new(WqErrorType::Domain)
                    .src(BE::Asciiplot)
                    .msg("series config `fn` must be a callable function or CAS expression"))
            } else {
                Err(WqError::new(WqErrorType::Domain)
                    .src(BE::Asciiplot)
                    .msg(
                        "expected each arg to be point data, a function, a CAS expression, \
                         table-shaped data, or a series config dict with `fn`",
                    )
                    .attach_note(
                        "e.g. (1;2;3), ((1;2);(2;4)), {x*x}, @s x^2, or (`x:(0;1);`y:(2;3))",
                    ))
            }
        }
        v if v.is_callable() => Ok(vec![SeriesConfig {
            data: SeriesData::Callable(arg.clone()),
            xlim: None,
            symbol: None,
            mode: None,
            label: None,
        }]),
        _ => Err(expected_series_arg_error()),
    }
}

#[derive(Clone)]
struct TableData {
    headers: Vec<String>,
    columns: IndexMap<String, Vec<Option<Value>>>,
    nrows: usize,
}

fn parse_table_arg(arg: &Value) -> Option<TableData> {
    match arg {
        Value::Dict(map) => parse_dict_of_lists_table(map),
        Value::List(rows) => parse_list_of_dicts_table(rows),
        _ => None,
    }
}

fn parse_dict_of_lists_table(map: &IndexMap<std::sync::Arc<str>, Value>) -> Option<TableData> {
    if map.is_empty()
        || !map
            .values()
            .all(|v| matches!(v, Value::List(_) | Value::IntList(_)))
    {
        return None;
    }

    let headers: Vec<String> = map.keys().map(|k| k.to_string()).collect();
    let nrows = map.values().map(column_len).max().unwrap_or(0);
    let mut columns = IndexMap::new();
    for (key, value) in map.iter() {
        let mut column = Vec::with_capacity(nrows);
        for idx in 0..nrows {
            column.push(column_item(value, idx));
        }
        columns.insert(key.to_string(), column);
    }
    Some(TableData {
        headers,
        columns,
        nrows,
    })
}

fn parse_list_of_dicts_table(rows: &[Value]) -> Option<TableData> {
    if rows.is_empty() || !rows.iter().all(|row| matches!(row, Value::Dict(_))) {
        return None;
    }

    let mut headers: Vec<String> = Vec::new();
    for row in rows {
        let Value::Dict(map) = row else {
            unreachable!();
        };
        for key in map.keys() {
            if !headers.iter().any(|header| header == key.as_ref()) {
                headers.push(key.to_string());
            }
        }
    }

    let nrows = rows.len();
    let mut columns = IndexMap::new();
    for header in &headers {
        let mut column = Vec::with_capacity(nrows);
        for row in rows {
            let Value::Dict(map) = row else {
                unreachable!();
            };
            column.push(map.get(header.as_str()).cloned());
        }
        columns.insert(header.clone(), column);
    }
    Some(TableData {
        headers,
        columns,
        nrows,
    })
}

fn column_len(value: &Value) -> usize {
    match value {
        Value::List(items) => items.len(),
        Value::IntList(items) => items.len(),
        _ => 0,
    }
}

fn column_item(value: &Value, idx: usize) -> Option<Value> {
    match value {
        Value::List(items) => items.get(idx).cloned(),
        Value::IntList(items) => items.get(idx).copied().map(Value::Int),
        _ => None,
    }
}

fn table_series_configs(table: TableData, opts: &PlotOptions) -> WqResult<Vec<SeriesConfig>> {
    let x_column = opts.table_x.as_deref();
    if let Some(name) = x_column
        && !table.columns.contains_key(name)
    {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BE::Asciiplot)
            .msg(format!("table x column `{name}` was not found")));
    }

    let y_columns = if let Some(columns) = &opts.table_y {
        for name in columns {
            if !table.columns.contains_key(name) {
                return Err(WqError::new(WqErrorType::Domain)
                    .src(BE::Asciiplot)
                    .msg(format!("table y column `{name}` was not found")));
            }
        }
        columns.clone()
    } else {
        table
            .headers
            .iter()
            .filter(|name| Some(name.as_str()) != x_column)
            .filter(|name| table_column_has_number(&table, name))
            .cloned()
            .collect()
    };

    if y_columns.is_empty() {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BE::Asciiplot)
            .msg("table-shaped data has no numeric y columns to plot"));
    }

    let mut configs = Vec::with_capacity(y_columns.len());
    for y_name in y_columns {
        let mut points = Vec::new();
        for idx in 0..table.nrows {
            let x = if let Some(name) = x_column {
                table_cell_as_f64(&table, name, idx)
            } else {
                Some(idx as f64)
            };
            let y = table_cell_as_f64(&table, &y_name, idx);
            if let (Some(x), Some(y)) = (x, y) {
                points.push((x, y));
            }
        }

        if points.is_empty() {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BE::Asciiplot)
                .msg(format!("table y column `{y_name}` has no numeric points")));
        }

        configs.push(SeriesConfig {
            data: SeriesData::Raw(SampledSeries::from_points(points)),
            xlim: None,
            symbol: None,
            mode: None,
            label: Some(y_name),
        });
    }

    Ok(configs)
}

fn table_column_has_number(table: &TableData, name: &str) -> bool {
    (0..table.nrows).any(|idx| table_cell_as_f64(table, name, idx).is_some())
}

fn table_cell_as_f64(table: &TableData, name: &str, idx: usize) -> Option<f64> {
    let value = table.columns.get(name)?.get(idx)?.as_ref()?;
    expect_real_sample(value)
}

fn expected_series_arg_error() -> WqError {
    WqError::new(WqErrorType::Domain)
        .src(BE::Asciiplot)
        .msg(
            "expected each arg to be point data, a function, a symbolic CAS expression, or table-shaped data",
        )
        .attach_note("e.g. (1;2;3), ((1;2);(2;4)), {x*x}, @s x^2, or (`x:(0;1);`y:(2;3))")
}

fn sample_callable_series(
    vm: &mut dyn BuiltinContext,
    func: &Value,
    opts: &PlotOptions,
    xlim: Option<(f64, f64)>,
) -> WqResult<SampledSeries<f64>> {
    let (xmin, xmax) = xlim.or(opts.xlim).unwrap_or((-10.0, 10.0));
    let initial_samples = opts.samples.unwrap_or(opts.width).max(2);

    if opts.complex_mode == "plane" {
        let mut sampler = |x: f64| -> Option<Value> { vm.call(func, Value::float(x).into()).ok() };
        let raw = sample_with_segments(xmin, xmax, initial_samples, &mut sampler);
        Ok(transform_complex_plane(raw))
    } else {
        let mut sampler = |x: f64| -> Option<f64> {
            let y = vm.call(func, Value::float(x).into()).ok()?;
            extract_numeric_component(&y, &opts.complex_mode)
        };
        Ok(sample_real_with_segments(
            xmin,
            xmax,
            initial_samples,
            &mut sampler,
        ))
    }
}

fn sample_cas_series(
    expr: &Value,
    opts: &PlotOptions,
    xlim: Option<(f64, f64)>,
) -> WqResult<SampledSeries<f64>> {
    let (xmin, xmax) = xlim.or(opts.xlim).unwrap_or((-10.0, 10.0));
    let initial_samples = opts.samples.unwrap_or(opts.width).max(2);

    let var = Value::from_cas_var(infer_single_cas_var(expr).map_err(|e| e.src(BE::Asciiplot))?);

    if opts.complex_mode == "plane" {
        let mut sampler = |x: f64| -> Option<Value> {
            substitute_cas(expr, &var, &Value::float(x))
                .map_err(|e| e.src(BE::Asciiplot))
                .ok()
        };
        let raw = sample_with_segments(xmin, xmax, initial_samples, &mut sampler);
        Ok(transform_complex_plane(raw))
    } else {
        let mut sampler = |x: f64| -> Option<f64> {
            let y = substitute_cas(expr, &var, &Value::float(x))
                .map_err(|e| e.src(BE::Asciiplot))
                .ok()?;
            extract_numeric_component(&y, &opts.complex_mode)
        };
        Ok(sample_real_with_segments(
            xmin,
            xmax,
            initial_samples,
            &mut sampler,
        ))
    }
}

fn sample_with_segments<T, F>(
    xmin: f64,
    xmax: f64,
    initial_samples: usize,
    sampler: &mut F,
) -> SampledSeries<T>
where
    F: FnMut(f64) -> Option<T>,
{
    sampled_from_raw_samples(collect_samples(xmin, xmax, initial_samples, sampler), &[])
}

fn sample_real_with_segments<F>(
    xmin: f64,
    xmax: f64,
    initial_samples: usize,
    sampler: &mut F,
) -> SampledSeries<f64>
where
    F: FnMut(f64) -> Option<f64>,
{
    let samples = collect_samples(xmin, xmax, initial_samples, sampler);
    let breaks = real_discontinuity_breaks(&samples);
    sampled_from_raw_samples(samples, &breaks)
}

fn collect_samples<T, F>(
    xmin: f64,
    xmax: f64,
    initial_samples: usize,
    sampler: &mut F,
) -> Vec<(f64, Option<T>)>
where
    F: FnMut(f64) -> Option<T>,
{
    let count = initial_samples.max(1);
    let step = if count > 1 {
        (xmax - xmin) / (count.saturating_sub(1)) as f64
    } else {
        0.0
    };

    let mut samples = Vec::with_capacity(count);
    for i in 0..count {
        let x = xmin + step * i as f64;
        samples.push((x, sampler(x)));
    }
    samples
}

fn sampled_from_raw_samples<T>(
    samples: Vec<(f64, Option<T>)>,
    break_after_raw: &[bool],
) -> SampledSeries<T> {
    let mut out = SampledSeries::new();
    let mut break_before_next_finite = false;
    for (idx, (x, y)) in samples.into_iter().enumerate() {
        if let Some(y) = y {
            out.push((x, y), break_before_next_finite);
            break_before_next_finite = false;
        } else {
            break_before_next_finite = true;
        }
        if break_after_raw.get(idx).copied().unwrap_or(false) {
            break_before_next_finite = true;
        }
    }
    out
}

fn real_discontinuity_breaks(samples: &[(f64, Option<f64>)]) -> Vec<bool> {
    let mut deltas = vec![None; samples.len().saturating_sub(1)];
    let mut finite_deltas = Vec::new();
    let mut max_abs_y = 0.0_f64;

    for &(_, y) in samples {
        if let Some(y) = y
            && y.is_finite()
        {
            max_abs_y = max_abs_y.max(y.abs());
        }
    }

    for idx in 0..samples.len().saturating_sub(1) {
        if let (Some(y0), Some(y1)) = (samples[idx].1, samples[idx + 1].1) {
            let delta = (y1 - y0).abs();
            if delta.is_finite() {
                deltas[idx] = Some(delta);
                finite_deltas.push(delta);
            }
        }
    }

    let typical = median(&mut finite_deltas).unwrap_or(0.0);
    let epsilon = typical.max(max_abs_y * 1e-12).max(1e-12);
    let mut breaks = vec![false; samples.len().saturating_sub(1)];

    for idx in 0..deltas.len() {
        let Some(delta) = deltas[idx] else {
            continue;
        };
        if delta <= epsilon {
            continue;
        }

        let left = idx
            .checked_sub(1)
            .and_then(|left_idx| deltas[left_idx])
            .unwrap_or(0.0);
        let right = deltas.get(idx + 1).and_then(|d| *d).unwrap_or(0.0);
        let local = left.max(right).max(epsilon);
        let (Some(y0), Some(y1)) = (samples[idx].1, samples[idx + 1].1) else {
            continue;
        };
        let sign_flip = (y0 < 0.0 && y1 > 0.0) || (y0 > 0.0 && y1 < 0.0);
        let much_larger_than_typical = delta > epsilon * 20.0;
        let isolated_jump = delta > local * 8.0;
        let sign_flip_jump = sign_flip && delta > epsilon * 2.5 && delta > local * 1.2;

        if (much_larger_than_typical && isolated_jump) || sign_flip_jump {
            breaks[idx] = true;
        }
    }

    breaks
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    Some(values[values.len() / 2])
}

fn expect_real_sample(value: &Value) -> Option<f64> {
    let y = value.as_f64()?;
    if y.is_finite() { Some(y) } else { None }
}

fn extract_numeric_component(value: &Value, mode: &str) -> Option<f64> {
    match mode {
        "re" => expect_real_sample(value),
        "im" => {
            if let Some(z) = value.as_complex64() {
                Some(z.im)
            } else {
                value.as_f64().filter(|y| y.is_finite()).map(|_| 0.0)
            }
        }
        "abs" => {
            if let Some(z) = value.as_complex64() {
                Some(z.norm())
            } else {
                value.as_f64().filter(|y| y.is_finite()).map(|y| y.abs())
            }
        }
        "arg" => {
            if let Some(z) = value.as_complex64() {
                Some(z.arg())
            } else {
                value
                    .as_f64()
                    .filter(|y| y.is_finite())
                    .map(|y| if y < 0.0 { std::f64::consts::PI } else { 0.0 })
            }
        }
        _ => expect_real_sample(value),
    }
}

fn transform_complex_plane(raw: SampledSeries<Value>) -> SampledSeries<f64> {
    filter_map_sampled(raw, |_x, v| {
        let z = v.as_complex64()?;
        Some((z.re, z.im))
    })
}

fn filter_map_sampled<T, U, F>(raw: SampledSeries<T>, mut f: F) -> SampledSeries<U>
where
    F: FnMut(f64, T) -> Option<(f64, U)>,
{
    let SampledSeries {
        points,
        breaks_after,
    } = raw;
    let mut out = SampledSeries::new();
    let mut break_idx = 0;
    let mut break_before_next = false;

    for (idx, (x, value)) in points.into_iter().enumerate() {
        if let Some(mapped) = f(x, value) {
            out.push(mapped, break_before_next);
            break_before_next = false;
        } else {
            break_before_next = true;
        }
        if breaks_after.get(break_idx).copied() == Some(idx) {
            break_before_next = true;
            break_idx += 1;
        }
    }

    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlotMode {
    Line,
    Scatter,
    Step,
    Bar,
    Area,
}

fn parse_plot_mode(value: &Value) -> Option<PlotMode> {
    let s: Option<&str> = match value {
        Value::Tag(sym) => Some(sym),
        _ => value
            .to_rust_string_with_note()
            .ok()
            .map(|ss| Box::leak(ss.into_boxed_str()) as &str),
    };
    s.and_then(|m| match m {
        "line" | "l" => Some(PlotMode::Line),
        "scatter" | "sc" => Some(PlotMode::Scatter),
        "step" | "st" => Some(PlotMode::Step),
        "bar" | "b" => Some(PlotMode::Bar),
        "area" | "a" => Some(PlotMode::Area),
        _ => None,
    })
}

fn parse_column_name(value: &Value) -> Option<String> {
    let name = match value {
        Value::Tag(sym) => sym.to_string(),
        _ => value.to_rust_string_with_note().ok()?,
    };
    if name.is_empty() { None } else { Some(name) }
}

fn parse_column_names(value: &Value) -> Option<Vec<String>> {
    let names = match value {
        Value::List(items) if !items.iter().all(|item| matches!(item, Value::Char(_))) => {
            items.iter().filter_map(parse_column_name).collect()
        }
        _ => parse_column_name(value).map(|name| vec![name])?,
    };
    if names.is_empty() { None } else { Some(names) }
}

#[derive(Clone)]
struct PlotOptions {
    width: usize,
    height: usize,
    xlim: Option<(f64, f64)>,
    ylim: Option<(f64, f64)>,
    symbols: Vec<char>,
    labels: Option<Vec<String>>,
    table_x: Option<String>,
    table_y: Option<Vec<String>>,
    mode: PlotMode,
    axes: AxesMode,
    color: ColorMode,
    grid: GridMode,
    samples: Option<usize>,
    theme: Option<String>,
    complex_mode: String,
    ascii: bool,
    tick_labels: bool,
    title: Option<String>,
    xlabel: Option<String>,
    ylabel: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AxesMode {
    Full,
    Minimal,
    Off,
}

#[derive(Clone)]
enum ColorMode {
    On,
    Off,
    Custom(Vec<Color>),
}

impl ColorMode {
    fn is_on(&self) -> bool {
        !matches!(self, ColorMode::Off)
    }
}

#[derive(Clone)]
enum GridMode {
    Off,
    On,
    Density(usize, usize),
}

impl Default for PlotOptions {
    fn default() -> Self {
        Self {
            width: 80,
            height: 24,
            xlim: None,
            ylim: None,
            symbols: vec!['·'],
            labels: None,
            table_x: None,
            table_y: None,
            mode: PlotMode::Line,
            axes: AxesMode::Full,
            color: ColorMode::On,
            grid: GridMode::Off,
            samples: None,
            theme: None,
            complex_mode: "re".to_string(),
            ascii: false,
            tick_labels: false,
            title: None,
            xlabel: None,
            ylabel: None,
        }
    }
}

impl PlotOptions {
    fn apply_from_named(&mut self, args: &BuiltinFnArgs) -> WqResult<bool> {
        let mut explicit_size = false;

        if let Some((a, b)) = args.named("size").and_then(pair_as_f64) {
            self.width = max(10, a as usize);
            self.height = max(5, b as usize);
            explicit_size = true;
        }
        if let Some(n) = args.named("width").and_then(|v| v.as_i64()) {
            self.width = max(10, n as usize);
            explicit_size = true;
        }
        if let Some(n) = args.named("height").and_then(|v| v.as_i64()) {
            self.height = max(5, n as usize);
            explicit_size = true;
        }
        if let Some((a, b)) = args.named("xlim").and_then(pair_as_f64) {
            self.xlim = Some((a, b));
        }
        if let Some((a, b)) = args.named("ylim").and_then(pair_as_f64) {
            self.ylim = Some((a, b));
        }
        if let Some(Value::List(items)) = args.named("symbols") {
            let mut syms = Vec::new();
            for it in items.iter() {
                match *it {
                    Value::Char(c) => syms.push(c),
                    _ => {
                        if let Ok(s) = it.to_rust_string_with_note()
                            && let Some(c) = s.chars().next()
                        {
                            syms.push(c);
                        }
                    }
                }
            }
            if !syms.is_empty() {
                self.symbols = syms;
            }
        } else if let Some(val) = args.named("symbols") {
            if let Ok(s) = val.to_rust_string_with_note() {
                if let Some(c) = s.chars().next() {
                    self.symbols = vec![c];
                }
            } else if let Value::Char(c) = val {
                self.symbols = vec![*c];
            }
        }
        if let Some(v) = args.named("axes") {
            if let Value::Bool(false) = v {
                self.axes = AxesMode::Off;
            } else if let Value::Bool(true) = v {
                self.axes = AxesMode::Full;
            } else if let Ok(s) = v.to_rust_string_with_note() {
                self.axes = match s.as_str() {
                    "full" => AxesMode::Full,
                    "minimal" => AxesMode::Minimal,
                    _ => self.axes,
                };
            }
        }
        if let Some(v) = args.named("mode")
            && let Some(mode) = parse_plot_mode(v)
        {
            self.mode = mode;
        }
        if let Some(v) = args.named("color") {
            if let Value::Bool(false) = v {
                self.color = ColorMode::Off;
            } else if let Value::Bool(true) = v {
                self.color = ColorMode::On;
            } else if let Some(p) = parse_palette(v) {
                self.color = ColorMode::Custom(p);
            }
        }
        if let Some(v) = args.named("grid") {
            if let Value::Bool(false) = v {
                self.grid = GridMode::Off;
            } else if let Value::Bool(true) = v {
                self.grid = GridMode::On;
            } else if let Some(n) = v.as_i64() {
                let n = n.max(1) as usize;
                self.grid = GridMode::Density(n, n);
            } else if let Some((a, b)) = pair_as_f64(v) {
                self.grid = GridMode::Density(a.max(1.0) as usize, b.max(1.0) as usize);
            }
        }
        if let Some(Value::List(items)) = args.named("labels") {
            let mut labs = Vec::new();
            for it in items.iter() {
                if let Ok(s) = it.to_rust_string_with_note() {
                    labs.push(s);
                }
            }
            if !labs.is_empty() {
                self.labels = Some(labs);
            }
        }
        if let Some(v) = args.named("x")
            && let Some(name) = parse_column_name(v)
        {
            self.table_x = Some(name);
        }
        if let Some(v) = args.named("y")
            && let Some(names) = parse_column_names(v)
        {
            self.table_y = Some(names);
        }
        if let Some(v) = args.named("samples").and_then(|v| v.as_i64()) {
            self.samples = Some(max(1, v as usize));
        }
        if let Some(v) = args.named("theme")
            && let Ok(s) = v.to_rust_string_with_note()
        {
            self.theme = Some(s);
        }
        if let Some(v) = args.named("complex")
            && let Ok(s) = v.to_rust_string_with_note()
        {
            self.complex_mode = s;
        }
        if let Some(Value::Bool(b)) = args.named("ascii") {
            self.ascii = *b;
        }
        if let Some(Value::Bool(b)) = args.named("ticklabels") {
            self.tick_labels = *b;
        }
        if let Some(v) = args.named("title")
            && let Ok(s) = v.to_rust_string_with_note()
        {
            self.title = Some(s);
        }
        if let Some(v) = args.named("xlabel")
            && let Ok(s) = v.to_rust_string_with_note()
        {
            self.xlabel = Some(s);
        }
        if let Some(v) = args.named("ylabel")
            && let Ok(s) = v.to_rust_string_with_note()
        {
            self.ylabel = Some(s);
        }
        if let Some(Value::List(items)) = args.named("caption")
            && items.len() >= 3
        {
            if let Ok(s) = items[0].to_rust_string_with_note() {
                self.title = Some(s);
            }
            if let Ok(s) = items[1].to_rust_string_with_note() {
                self.xlabel = Some(s);
            }
            if let Ok(s) = items[2].to_rust_string_with_note() {
                self.ylabel = Some(s);
            }
        }

        Ok(explicit_size)
    }

    fn apply_theme(&mut self, theme: &str) {
        match theme {
            "minimal" => {
                self.axes = AxesMode::Off;
                self.grid = GridMode::Off;
                self.color = ColorMode::On;
            }
            "scientific" => {
                self.axes = AxesMode::Full;
                self.grid = GridMode::On;
                self.color = ColorMode::On;
            }
            "dark" => {
                self.axes = AxesMode::Full;
                self.grid = GridMode::Off;
                self.color = ColorMode::On;
            }
            _ => {}
        }
    }
}

fn pair_as_f64(value: &Value) -> Option<(f64, f64)> {
    let (a, b) = match value {
        Value::List(items) if items.len() == 2 => (items[0].as_f64()?, items[1].as_f64()?),
        Value::IntList(items) if items.len() == 2 => (items[0] as f64, items[1] as f64),
        _ => return None,
    };
    Some((a, b))
}

fn render_ascii_plot(series_list: &[PlotSeries], opts: &PlotOptions) -> String {
    let width = opts.width;
    let height = opts.height;
    let width = max(10, width);
    let height = max(5, height);
    // Determine bounds
    let (mut xmin, mut xmax) = opts.xlim.unwrap_or_else(|| {
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        for s in series_list {
            for &(x, _) in &s.points {
                if x < min_x {
                    min_x = x;
                }
                if x > max_x {
                    max_x = x;
                }
            }
        }
        (min_x, max_x)
    });
    let (mut ymin, mut ymax) = opts.ylim.unwrap_or_else(|| {
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for s in series_list {
            for &(_, y) in &s.points {
                if y < min_y {
                    min_y = y;
                }
                if y > max_y {
                    max_y = y;
                }
            }
        }
        (min_y, max_y)
    });
    if !xmin.is_finite() || !xmax.is_finite() || xmin == xmax {
        xmin -= 0.5;
        xmax += 0.5;
    }
    if !ymin.is_finite() || !ymax.is_finite() || ymin == ymax {
        ymin -= 0.5;
        ymax += 0.5;
    }
    let xspan = xmax - xmin;
    let yspan = ymax - ymin;
    // grid[y][x]
    let mut grid: Vec<Vec<Cell>> = vec![vec![Cell::new(' '); width]; height];
    // axes
    let y0_in = ymin <= 0.0 && 0.0 <= ymax;
    let x0_in = xmin <= 0.0 && 0.0 <= xmax;
    let y0_row = if y0_in {
        let t = (0.0 - ymin) / yspan; // 0..1 from bottom
        let row = (height as f64 - 1.0 - t * (height as f64 - 1.0)).round() as isize;
        min(height as isize - 1, max(0, row)) as usize
    } else {
        height - 1
    };
    let x0_col = if x0_in {
        let t = (0.0 - xmin) / xspan;
        let col = (t * (width as f64 - 1.0)).round() as isize;
        min(width as isize - 1, max(0, col)) as usize
    } else {
        0usize
    };
    // optional gridlines
    let (gx, gy) = match &opts.grid {
        GridMode::Off => (0, 0),
        GridMode::On => (4, 4),
        GridMode::Density(x, y) => (*x, *y),
    };
    if gx > 0 && gy > 0 {
        let xticks = ticks_in_range(xmin, xmax, gx);
        let yticks = ticks_in_range(ymin, ymax, gy);
        let grid_color = Some(Color::BrightBlack);
        let ch_h = if opts.ascii { '.' } else { '┈' };
        let ch_v = if opts.ascii { ':' } else { '┊' };
        for yv in yticks {
            let t = (yv - ymin) / yspan;
            let row = (height as f64 - 1.0 - t * (height as f64 - 1.0)).round() as isize;
            let r = std::cmp::min(height as isize - 1, std::cmp::max(0, row)) as usize;
            for x in 0..width {
                if x % 2 == 0 {
                    set_cell_layer(
                        &mut grid,
                        x as isize,
                        r as isize,
                        ch_h,
                        Layer::Grid,
                        grid_color,
                        opts.color.is_on(),
                    );
                }
            }
        }
        for xv in xticks {
            let t = (xv - xmin) / xspan;
            let col = (t * (width as f64 - 1.0)).round() as isize;
            let c = std::cmp::min(width as isize - 1, std::cmp::max(0, col)) as usize;
            for y in 0..height {
                if y % 2 == 0 {
                    set_cell_layer(
                        &mut grid,
                        c as isize,
                        y as isize,
                        ch_v,
                        Layer::Grid,
                        grid_color,
                        opts.color.is_on(),
                    );
                }
            }
        }
    }
    // axes
    match opts.axes {
        AxesMode::Off => {}
        AxesMode::Minimal => {
            let axis_h = if opts.ascii { '-' } else { '─' };
            let axis_v = if opts.ascii { '|' } else { '│' };
            let cross = if opts.ascii { '+' } else { '┼' };
            for x in 0..width {
                set_cell_layer(
                    &mut grid,
                    x as isize,
                    y0_row as isize,
                    axis_h,
                    Layer::Axis,
                    None,
                    opts.color.is_on(),
                );
            }
            if x0_in {
                for y in 0..height {
                    set_cell_layer(
                        &mut grid,
                        x0_col as isize,
                        y as isize,
                        axis_v,
                        Layer::Axis,
                        None,
                        opts.color.is_on(),
                    );
                }
            }
            if x0_in && y0_in {
                set_cell_layer(
                    &mut grid,
                    x0_col as isize,
                    y0_row as isize,
                    cross,
                    Layer::Axis,
                    None,
                    opts.color.is_on(),
                );
            }
        }
        AxesMode::Full => {
            let axis_h = if opts.ascii { '-' } else { '─' };
            let axis_v = if opts.ascii { '|' } else { '│' };
            let cross = if opts.ascii { '+' } else { '┼' };
            for x in 0..width {
                set_cell_layer(
                    &mut grid,
                    x as isize,
                    y0_row as isize,
                    axis_h,
                    Layer::Axis,
                    None,
                    opts.color.is_on(),
                );
            }
            if x0_in {
                for y in 0..height {
                    set_cell_layer(
                        &mut grid,
                        x0_col as isize,
                        y as isize,
                        axis_v,
                        Layer::Axis,
                        None,
                        opts.color.is_on(),
                    );
                }
            }
            if x0_in && y0_in {
                set_cell_layer(
                    &mut grid,
                    x0_col as isize,
                    y0_row as isize,
                    cross,
                    Layer::Axis,
                    None,
                    opts.color.is_on(),
                );
            }
            // Tick marks
            let tick_count = 5usize;
            let xticks = ticks_in_range(xmin, xmax, tick_count);
            let yticks = ticks_in_range(ymin, ymax, tick_count);
            for xv in &xticks {
                let t = (*xv - xmin) / xspan;
                let col = (t * (width as f64 - 1.0)).round() as isize;
                let c = min(width as isize - 1, max(0, col));
                if c as usize != x0_col {
                    set_cell_layer(
                        &mut grid,
                        c,
                        y0_row as isize,
                        if opts.ascii { '+' } else { '┼' },
                        Layer::Axis,
                        None,
                        opts.color.is_on(),
                    );
                }
            }
            for yv in &yticks {
                let t = (*yv - ymin) / yspan;
                let row = ((height as f64 - 1.0) - t * (height as f64 - 1.0)).round() as isize;
                let r = min(height as isize - 1, max(0, row));
                if r as usize != y0_row {
                    set_cell_layer(
                        &mut grid,
                        x0_col as isize,
                        r,
                        if opts.ascii { '+' } else { '┼' },
                        Layer::Axis,
                        None,
                        opts.color.is_on(),
                    );
                }
            }
        }
    }
    // draw series
    for (si, plot_series) in series_list.iter().enumerate() {
        let series = &plot_series.points;
        if series.is_empty() {
            continue;
        }
        let symbol = plot_series_symbol(Some(plot_series), si, opts);
        let mode = plot_series.mode.unwrap_or(opts.mode);
        let color: Option<Color> = match &opts.color {
            ColorMode::On => Some(series_color(si)),
            ColorMode::Off => None,
            ColorMode::Custom(p) => p
                .get(si % p.len())
                .cloned()
                .or_else(|| Some(series_color(si))),
        };
        // eprintln!("asciiplot: using symbol '{}' for mode {:?}", symbol, opts.mode as
        // u8); map all points first
        let mut pts: Vec<(isize, isize)> = Vec::with_capacity(series.len());
        for &(x, y) in series {
            let tx = (x - xmin) / xspan;
            let ty = (y - ymin) / yspan;
            let col = (tx * (width as f64 - 1.0)).round() as isize;
            let row = ((height as f64 - 1.0) - ty * (height as f64 - 1.0)).round() as isize;
            pts.push((col, row));
        }
        match mode {
            PlotMode::Line => {
                for (idx, w) in pts.windows(2).enumerate() {
                    if plot_series.breaks_after.contains(&idx) {
                        continue;
                    }
                    let (x0, y0) = w[0];
                    let (x1, y1) = w[1];
                    plot_line(&mut grid, x0, y0, x1, y1, symbol, color);
                }
                for &(x, y) in &pts {
                    set_cell_layer(
                        &mut grid,
                        x,
                        y,
                        symbol,
                        Layer::Data,
                        color,
                        opts.color.is_on(),
                    );
                }
            }
            PlotMode::Scatter => {
                for (x, y) in pts {
                    set_cell_layer(
                        &mut grid,
                        x,
                        y,
                        symbol,
                        Layer::Data,
                        color,
                        opts.color.is_on(),
                    );
                }
            }
            PlotMode::Step => {
                for (idx, w) in pts.windows(2).enumerate() {
                    if plot_series.breaks_after.contains(&idx) {
                        continue;
                    }
                    let (x0, y0) = w[0];
                    let (x1, y1) = w[1];
                    // horizontal, then vertical
                    plot_line(&mut grid, x0, y0, x1, y0, symbol, color);
                    plot_line(&mut grid, x1, y0, x1, y1, symbol, color);
                }
                if pts.len() == 1 {
                    let (x, y) = pts[0];
                    set_cell_layer(
                        &mut grid,
                        x,
                        y,
                        symbol,
                        Layer::Data,
                        color,
                        opts.color.is_on(),
                    );
                }
            }
            PlotMode::Bar => {
                let baseline_row = if y0_in {
                    y0_row as isize
                } else {
                    height as isize - 1
                };
                for (x, y) in pts {
                    plot_line(&mut grid, x, baseline_row, x, y, symbol, color);
                }
            }
            PlotMode::Area => {
                let baseline_row = if y0_in {
                    y0_row as isize
                } else {
                    height as isize - 1
                };
                for (idx, w) in pts.windows(2).enumerate() {
                    if plot_series.breaks_after.contains(&idx) {
                        continue;
                    }
                    let (x0, y0) = w[0];
                    let (x1, y1) = w[1];
                    let line = rasterize_line(x0, y0, x1, y1);
                    for (x, yl) in line {
                        let ystart = min(baseline_row, yl);
                        let yend = max(baseline_row, yl);
                        for yy in ystart..=yend {
                            set_cell_layer(
                                &mut grid,
                                x,
                                yy,
                                symbol,
                                Layer::Data,
                                color,
                                opts.color.is_on(),
                            );
                        }
                    }
                }
                if pts.len() == 1 {
                    let (x, y) = pts[0];
                    let ystart = min(baseline_row, y);
                    let yend = max(baseline_row, y);
                    for yy in ystart..=yend {
                        set_cell_layer(
                            &mut grid,
                            x,
                            yy,
                            symbol,
                            Layer::Data,
                            color,
                            opts.color.is_on(),
                        );
                    }
                }
            }
        }
    }
    // Convert grid to string, with optional interior tick labels.
    let mut out = String::new();
    let ylabel_width = opts.ylabel.as_ref().map_or(0, |yl| yl.len() + 1);
    let label_tick_x = if gx > 0 { gx } else { 5 };
    let label_tick_y = if gy > 0 { gy } else { 5 };
    let y_tick_rows = if opts.tick_labels {
        y_tick_label_rows(height, ymin, ymax, yspan, label_tick_y)
    } else {
        vec![None; height]
    };
    let y_tick_width = y_tick_rows
        .iter()
        .filter_map(|label| label.as_ref().map(String::len))
        .max()
        .unwrap_or(0);
    let y_tick_prefix_width = if y_tick_width > 0 {
        y_tick_width + 1
    } else {
        0
    };
    let left_label_width = ylabel_width + y_tick_prefix_width;
    // Title
    if let Some(ref title) = opts.title {
        let tw = width.max(title.len());
        let pad = tw.saturating_sub(title.len()) / 2 + left_label_width;
        out.push_str(&" ".repeat(pad));
        out.push_str(title);
        out.push('\n');
    }
    // ylabel prefix on every row so plot area stays aligned
    for (i, row) in grid.iter().enumerate() {
        let mut line = String::new();
        if i == height / 2
            && let Some(ref yl) = opts.ylabel
        {
            line.push_str(yl);
            line.push(' ');
        } else {
            line.push_str(&" ".repeat(ylabel_width));
        }
        if y_tick_width > 0 {
            if let Some(label) = &y_tick_rows[i] {
                line.push_str(&format!("{label:>y_tick_width$} "));
            } else {
                line.push_str(&" ".repeat(y_tick_prefix_width));
            }
        }
        for cell in row {
            line.push_str(&paint_char(cell.ch, cell.color));
        }
        if !opts.tick_labels && i == 0 {
            line.push(' ');
            line.push_str(&format_number(ymax));
        }
        if !opts.tick_labels && i + 1 == height {
            line.push(' ');
            line.push_str(&format_number(ymin));
        }
        out.push_str(&line);
        out.push('\n');
    }
    // x labels on bottom line aligned to plot width
    // left label at col 0, right label ending at last col
    let left = format_number(xmin);
    let right = format_number(xmax);
    let mut xline = String::new();
    xline.push_str(&" ".repeat(left_label_width));
    if opts.tick_labels {
        xline.push_str(&x_tick_label_line(width, xmin, xmax, xspan, label_tick_x));
    } else {
        xline.push_str(&left);
        if width > left.len() + right.len() {
            let spaces = width - left.len() - right.len();
            for _ in 0..spaces {
                xline.push(' ');
            }
        } else {
            xline.push(' ');
        }
        xline.push_str(&right);
    }
    out.push_str(&xline);
    out.push('\n');
    // xlabel
    if let Some(ref xlabel) = opts.xlabel {
        let tw = width.max(xlabel.len());
        let pad = tw.saturating_sub(xlabel.len()) / 2 + left_label_width;
        out.push_str(&" ".repeat(pad));
        out.push_str(xlabel);
        out.push('\n');
    }
    // Legend (optional) — shown when global or per-series labels are provided
    let legend_len = opts
        .labels
        .as_ref()
        .map_or(series_list.len(), |labels| labels.len().max(series_list.len()));
    let mut leg_parts = Vec::new();
    for i in 0..legend_len {
        let label = series_list
            .get(i)
            .and_then(|series| series.label.as_deref())
            .or_else(|| {
                opts.labels
                    .as_ref()
                    .and_then(|labels| labels.get(i).map(String::as_str))
            });
        if let Some(label) = label {
            let sym = plot_series_symbol(series_list.get(i), i, opts);
            leg_parts.push(format!("{}({})", label, sym));
        }
    }
    if !leg_parts.is_empty() {
        out.push_str(&format!("{}\n", leg_parts.join(", ")));
    }
    out
}

fn plot_series_symbol(series: Option<&PlotSeries>, idx: usize, opts: &PlotOptions) -> char {
    let fallback = if opts.symbols.is_empty() {
        '·'
    } else {
        opts.symbols[idx % opts.symbols.len()]
    };
    series.and_then(|series| series.symbol).unwrap_or(fallback)
}

fn format_number(v: f64) -> String {
    // Use a compact formatting with up to 4 significant digits after decimal when
    // needed
    if v.abs() >= 10000.0 || (v != 0.0 && v.abs() < 0.001) {
        format!("{:.3e}", v)
    } else if (v - v.round()).abs() < 1e-9 {
        format!("{:.0}", v)
    } else {
        format!("{:.3}", v)
    }
}

fn ticks_in_range(minv: f64, maxv: f64, target: usize) -> Vec<f64> {
    let lo = minv.min(maxv);
    let hi = minv.max(maxv);
    let eps = ((hi - lo).abs() * 1e-9).max(1e-12);
    nice_ticks(minv, maxv, target)
        .into_iter()
        .filter(|tick| *tick >= lo - eps && *tick <= hi + eps)
        .collect()
}

fn tick_label_values(minv: f64, maxv: f64, target: usize) -> Vec<f64> {
    let lo = minv.min(maxv);
    let hi = minv.max(maxv);
    let eps = ((hi - lo).abs() * 1e-9).max(1e-12);
    let mut values = Vec::new();
    values.push(minv);
    for tick in ticks_in_range(minv, maxv, target) {
        if (tick - minv).abs() > eps && (tick - maxv).abs() > eps {
            values.push(tick);
        }
    }
    values.push(maxv);
    values.sort_by(f64::total_cmp);
    values.dedup_by(|a, b| (*a - *b).abs() <= eps);
    if minv > maxv {
        values.reverse();
    }
    values.retain(|value| *value >= lo - eps && *value <= hi + eps);
    values
}

fn x_tick_label_line(width: usize, xmin: f64, xmax: f64, xspan: f64, target: usize) -> String {
    let labels: Vec<(usize, String)> = tick_label_values(xmin, xmax, target)
        .into_iter()
        .map(|value| {
            let t = (value - xmin) / xspan;
            let col = (t * (width as f64 - 1.0)).round() as isize;
            let col = min(width as isize - 1, max(0, col)) as usize;
            (col, format_number(value))
        })
        .collect();
    render_positioned_labels(width, &labels)
}

fn y_tick_label_rows(
    height: usize,
    ymin: f64,
    ymax: f64,
    yspan: f64,
    target: usize,
) -> Vec<Option<String>> {
    let mut rows = vec![None; height];
    for value in tick_label_values(ymin, ymax, target) {
        let t = (value - ymin) / yspan;
        let row = ((height as f64 - 1.0) - t * (height as f64 - 1.0)).round() as isize;
        let row = min(height as isize - 1, max(0, row)) as usize;
        let label = format_number(value);
        if rows[row]
            .as_ref()
            .map(|current: &String| label.len() > current.len())
            .unwrap_or(true)
        {
            rows[row] = Some(label);
        }
    }
    rows
}

fn render_positioned_labels(width: usize, labels: &[(usize, String)]) -> String {
    let mut chars = vec![' '; width];
    let mut last_end = None;

    for (idx, (col, label)) in labels.iter().enumerate() {
        let len = label.chars().count();
        if len == 0 || len > width {
            continue;
        }
        let mut start = if idx == 0 {
            0
        } else if idx + 1 == labels.len() {
            width - len
        } else {
            col.saturating_sub(len / 2).min(width - len)
        };

        if let Some(end) = last_end
            && start <= end
        {
            start = end + 1;
        }
        if start + len > width {
            continue;
        }

        for (offset, ch) in label.chars().enumerate() {
            chars[start + offset] = ch;
        }
        last_end = Some(start + len - 1);
    }

    chars.into_iter().collect()
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Layer {
    Grid = 0,
    Axis = 1,
    Data = 2,
}

#[derive(Clone, Copy)]
struct Cell {
    ch: char,
    layer: Layer,
    color: Option<Color>,
}
impl Cell {
    fn new(ch: char) -> Self {
        Self {
            ch,
            layer: Layer::Grid,
            color: None,
        }
    }
}

fn set_cell_layer(
    grid: &mut [Vec<Cell>],
    x: isize,
    y: isize,
    ch: char,
    layer: Layer,
    color: Option<Color>,
    color_on: bool,
) {
    if y >= 0 && (y as usize) < grid.len() && x >= 0 && (x as usize) < grid[0].len() {
        let cell = &mut grid[y as usize][x as usize];
        if layer >= cell.layer || cell.ch == ' ' {
            cell.ch = ch;
            cell.layer = layer;
            cell.color = if color_on { color } else { None };
        }
    }
}

fn rasterize_line(mut x0: isize, mut y0: isize, x1: isize, y1: isize) -> Vec<(isize, isize)> {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy; // error value e_xy
    let mut pts = Vec::new();
    loop {
        pts.push((x0, y0));
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
    pts
}

fn plot_line(
    grid: &mut [Vec<Cell>],
    x0: isize,
    y0: isize,
    x1: isize,
    y1: isize,
    ch: char,
    color: Option<Color>,
) {
    for (x, y) in rasterize_line(x0, y0, x1, y1) {
        set_cell_layer(grid, x, y, ch, Layer::Data, color, color.is_some());
    }
}

fn series_color(idx: usize) -> Color {
    use Color::*;
    let palette = [
        Red,
        Green,
        Blue,
        Yellow,
        Magenta,
        Cyan,
        BrightRed,
        BrightGreen,
        BrightBlue,
        BrightYellow,
        BrightMagenta,
        BrightCyan,
    ];
    palette[idx % palette.len()]
}

fn paint_char(ch: char, color: Option<Color>) -> String {
    let s = ch.to_string();
    if let Some(c) = color {
        s.color(c).to_string()
    } else {
        s
    }
}

fn parse_palette(val: &Value) -> Option<Vec<Color>> {
    use Color::*;
    fn name_to_color<S: AsRef<str>>(s: S) -> Option<Color> {
        match s.as_ref().to_ascii_lowercase().as_str() {
            "black" | "bl" => Some(Black),
            "red" | "r" => Some(Red),
            "green" | "g" => Some(Green),
            "yellow" | "y" => Some(Yellow),
            "blue" | "b" => Some(Blue),
            "magenta" | "m" => Some(Magenta),
            "cyan" | "c" => Some(Cyan),
            "white" | "w" => Some(White),

            "bright_black" | "gray" | "grey" | "bbl" => Some(BrightBlack),
            "bright_red" | "br" => Some(BrightRed),
            "bright_green" | "bg" => Some(BrightGreen),
            "bright_yellow" | "by" => Some(BrightYellow),
            "bright_blue" | "bb" => Some(BrightBlue),
            "bright_magenta" | "bm" => Some(BrightMagenta),
            "bright_cyan" | "bc" => Some(BrightCyan),
            "bright_white" | "bw" => Some(BrightWhite),
            _ => None,
        }
    }
    match val {
        Value::List(items) => {
            let mut out = Vec::new();
            for it in items.iter() {
                if let Ok(s) = it.to_rust_string_with_note()
                    && let Some(c) = name_to_color(&s)
                {
                    out.push(c);
                }
            }
            if out.is_empty() { None } else { Some(out) }
        }
        // Single value: "red"
        _ => {
            if let Ok(s) = val.to_rust_string_with_note() {
                name_to_color(&s).map(|c| vec![c])
            } else {
                None
            }
        }
    }
}

fn nice_step(range: f64, target: usize) -> f64 {
    let target = target.max(1) as f64;
    let raw = (range / target).abs().max(1e-12);
    let exp = raw.log10().floor();
    let f = raw / 10f64.powf(exp);
    let nf = if f < 1.5 {
        1.0
    } else if f < 3.0 {
        2.0
    } else if f < 7.0 {
        5.0
    } else {
        10.0
    };
    nf * 10f64.powf(exp)
}

fn nice_ticks(minv: f64, maxv: f64, target: usize) -> Vec<f64> {
    let mut lo = minv.min(maxv);
    let mut hi = minv.max(maxv);
    if !lo.is_finite() || !hi.is_finite() || lo == hi {
        lo -= 0.5;
        hi += 0.5;
    }
    let step = nice_step(hi - lo, target);
    let start = (lo / step).floor() * step;
    let end = (hi / step).ceil() * step;
    let mut t = start;
    let mut out = Vec::new();
    for _ in 0..200 {
        if t > end + step * 0.5 {
            break;
        }
        out.push(t);
        t += step;
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use indexmap::IndexMap;
    use smallvec::smallvec;

    use super::*;
    use crate::builtins::Builtins;
    use crate::session::stdio::{WqStdout, set_wqstdout};
    use crate::value::cas::{CasFunction, CasOp};
    use crate::vm::Vm;

    struct SinkStdout;

    impl WqStdout for SinkStdout {
        fn print(&mut self, _s: &str) {}

        fn println(&mut self, _s: &str) {}
    }

    fn assert_raw_points(config: &SeriesConfig, expected: &[(f64, f64)]) {
        match &config.data {
            SeriesData::Raw(series) => assert_eq!(series.points.as_slice(), expected),
            SeriesData::Callable(_) | SeriesData::Cas(_) => {
                panic!("expected table-shaped data to produce raw points")
            }
        }
    }

    #[test]
    fn sample_cas_series_uses_symbolic_expression() {
        let expr = Value::from_cas_op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]);
        let opts = PlotOptions {
            width: 3,
            xlim: Some((0.0, 2.0)),
            ..PlotOptions::default()
        };
        let series = sample_cas_series(&expr, &opts, None).unwrap();
        assert_eq!(series.points, vec![(0.0, 0.0), (1.0, 1.0), (2.0, 4.0)]);
        assert!(series.breaks_after.is_empty());
    }

    #[test]
    fn sample_callable_series_uses_callback() {
        let opts = PlotOptions {
            width: 3,
            xlim: Some((-1.0, 1.0)),
            ..PlotOptions::default()
        };
        let mut vm = Vm::new(vec![]);
        let series = sample_callable_series(
            &mut vm,
            &Value::builtin_function("abs", Builtins::ABS),
            &opts,
            None,
        )
        .unwrap();
        assert_eq!(series.points, vec![(-1.0, 1.0), (0.0, 0.0), (1.0, 1.0)]);
        assert!(series.breaks_after.is_empty());
    }

    #[test]
    fn table_dict_of_lists_expands_to_selected_y_columns() {
        let value = Value::Dict(Arc::new(IndexMap::from([
            ("x".into(), Value::IntList(Arc::new(vec![0, 1, 2]))),
            (
                "sin".into(),
                Value::List(Arc::new(vec![
                    Value::float(0.0),
                    Value::float(0.84),
                    Value::float(0.91),
                ])),
            ),
            (
                "cos".into(),
                Value::List(Arc::new(vec![
                    Value::float(1.0),
                    Value::float(0.54),
                    Value::float(-0.42),
                ])),
            ),
        ])));
        let opts = PlotOptions {
            table_x: Some("x".to_string()),
            table_y: Some(vec!["sin".to_string(), "cos".to_string()]),
            ..PlotOptions::default()
        };

        let configs = parse_series_arg(&value, &opts).expect("table should parse");

        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].label.as_deref(), Some("sin"));
        assert_eq!(configs[1].label.as_deref(), Some("cos"));
        assert_raw_points(&configs[0], &[(0.0, 0.0), (1.0, 0.84), (2.0, 0.91)]);
        assert_raw_points(&configs[1], &[(0.0, 1.0), (1.0, 0.54), (2.0, -0.42)]);
    }

    #[test]
    fn table_list_of_dicts_uses_numeric_columns_when_y_is_unset() {
        let value = Value::List(Arc::new(vec![
            Value::Dict(Arc::new(IndexMap::from([
                ("x".into(), Value::Int(0)),
                ("sin".into(), Value::float(0.0)),
                ("name".into(), Value::Tag("a".into())),
            ]))),
            Value::Dict(Arc::new(IndexMap::from([
                ("x".into(), Value::Int(1)),
                ("sin".into(), Value::float(0.84)),
                ("name".into(), Value::Tag("b".into())),
            ]))),
        ]));
        let opts = PlotOptions {
            table_x: Some("x".to_string()),
            ..PlotOptions::default()
        };

        let configs = parse_series_arg(&value, &opts).expect("table should parse");

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].label.as_deref(), Some("sin"));
        assert_raw_points(&configs[0], &[(0.0, 0.0), (1.0, 0.84)]);
    }

    #[test]
    fn asciiplot_accepts_single_cas_arg() {
        let expr = Value::from_cas_function(CasFunction::Sin, vec![Value::from_cas_var("x")]);
        let mut vm = Vm::new(vec![]);
        set_wqstdout(Some(Box::new(SinkStdout)));
        let result = asciiplot(
            &mut vm,
            BuiltinFnArgs::with_named(
                smallvec![expr],
                vec![
                    (Arc::from("width"), Value::Int(4)),
                    (
                        Arc::from("xlim"),
                        Value::List(Arc::new(vec![Value::Int(0), Value::Int(1)])),
                    ),
                    (Arc::from("color"), Value::Bool(false)),
                ],
            ),
        );
        set_wqstdout(None);
        assert_eq!(result.unwrap(), Value::unit());
    }

    #[test]
    fn render_uses_per_series_symbols_in_legend() {
        let opts = PlotOptions {
            width: 10,
            height: 5,
            axes: AxesMode::Off,
            color: ColorMode::Off,
            ..PlotOptions::default()
        };
        let series = vec![
            PlotSeries {
                points: vec![(0.0, 0.0)],
                breaks_after: Vec::new(),
                symbol: Some('s'),
                mode: None,
                label: Some("sine".to_string()),
            },
            PlotSeries {
                points: vec![(1.0, 1.0)],
                breaks_after: Vec::new(),
                symbol: Some('c'),
                mode: None,
                label: Some("cosine".to_string()),
            },
        ];

        let rendered = render_ascii_plot(&series, &opts);

        assert!(rendered.contains("sine(s)"));
        assert!(rendered.contains("cosine(c)"));
    }

    #[test]
    fn render_uses_per_series_mode() {
        let opts = PlotOptions {
            width: 9,
            height: 5,
            xlim: Some((0.0, 2.0)),
            ylim: Some((0.0, 2.0)),
            symbols: vec!['*'],
            mode: PlotMode::Line,
            axes: AxesMode::Off,
            color: ColorMode::Off,
            ..PlotOptions::default()
        };
        let series = vec![PlotSeries {
            points: vec![(0.0, 0.0), (1.0, 2.0), (2.0, 0.0)],
            breaks_after: Vec::new(),
            symbol: Some('*'),
            mode: Some(PlotMode::Scatter),
            label: None,
        }];

        let rendered = render_ascii_plot(&series, &opts);
        let plotted_points = rendered.chars().filter(|&ch| ch == '*').count();

        assert_eq!(plotted_points, 3);
    }

    #[test]
    fn sample_callable_series_skips_invalid_points() {
        let opts = PlotOptions {
            width: 5,
            xlim: Some((-2.0, 2.0)),
            ..PlotOptions::default()
        };
        let mut vm = Vm::new(vec![]);
        // sqrt returns complex for negative inputs in wq; those should be skipped
        let series = sample_callable_series(
            &mut vm,
            &Value::builtin_function("sqrt", Builtins::SQRT),
            &opts,
            None,
        )
        .unwrap();
        // Only non-negative x values (0, 1, 2) should produce valid points
        assert!(!series.points.is_empty());
        for (x, _y) in &series.points {
            assert!(*x >= 0.0);
        }
    }

    #[test]
    fn sample_cas_series_skips_invalid_points() {
        let expr = Value::from_cas_function(CasFunction::Sqrt, vec![Value::from_cas_var("x")]);
        let opts = PlotOptions {
            width: 5,
            xlim: Some((-2.0, 2.0)),
            ..PlotOptions::default()
        };
        let series = sample_cas_series(&expr, &opts, None).unwrap();
        assert!(!series.points.is_empty());
        for (x, _y) in &series.points {
            assert!(*x >= 0.0);
        }
    }

    #[test]
    fn sample_callable_series_keeps_smooth_series_continuous() {
        let opts = PlotOptions {
            width: 5,
            xlim: Some((-1.0, 1.0)),
            ..PlotOptions::default()
        };
        let mut vm = Vm::new(vec![]);
        let series = sample_callable_series(
            &mut vm,
            &Value::builtin_function("abs", Builtins::ABS),
            &opts,
            None,
        )
        .unwrap();
        assert!(!series.points.is_empty());
        assert!(series.breaks_after.is_empty());
    }

    #[test]
    fn sampler_breaks_across_invalid_gap() {
        let mut sampler = |x: f64| if x == 0.0 { None } else { Some(1.0 / x) };

        let series = sample_real_with_segments(-1.0, 1.0, 3, &mut sampler);

        assert_eq!(series.points, vec![(-1.0, -1.0), (1.0, 1.0)]);
        assert_eq!(series.breaks_after, vec![0]);
    }

    #[test]
    fn sampler_breaks_across_finite_asymptote_jump() {
        let mut sampler = |x: f64| Some(1.0 / x);

        let series = sample_real_with_segments(-1.0, 1.0, 4, &mut sampler);

        assert_eq!(series.breaks_after, vec![1]);
    }

    #[test]
    fn sampler_breaks_across_isolated_jump() {
        let mut sampler = |x: f64| Some(if x < 0.0 { 0.0 } else { 10.0 });

        let series = sample_real_with_segments(-1.0, 1.0, 5, &mut sampler);

        assert_eq!(
            series.points,
            vec![(-1.0, 0.0), (-0.5, 0.0), (0.0, 10.0), (0.5, 10.0), (1.0, 10.0)]
        );
        assert_eq!(series.breaks_after, vec![1]);
    }

    #[test]
    fn render_line_honors_segment_breaks() {
        let opts = PlotOptions {
            width: 5,
            height: 5,
            xlim: Some((0.0, 1.0)),
            ylim: Some((0.0, 4.0)),
            symbols: vec!['*'],
            axes: AxesMode::Off,
            color: ColorMode::Off,
            ..PlotOptions::default()
        };
        let series = vec![PlotSeries {
            points: vec![(0.0, 0.0), (1.0, 4.0)],
            breaks_after: vec![0],
            symbol: Some('*'),
            mode: None,
            label: None,
        }];

        let rendered = render_ascii_plot(&series, &opts);
        let plotted_points = rendered.chars().filter(|&ch| ch == '*').count();

        assert_eq!(plotted_points, 2);
    }

    #[test]
    fn render_ticklabels_adds_interior_x_and_y_labels() {
        let opts = PlotOptions {
            width: 21,
            height: 7,
            xlim: Some((0.0, 4.0)),
            ylim: Some((0.0, 4.0)),
            grid: GridMode::Density(4, 4),
            tick_labels: true,
            axes: AxesMode::Full,
            ascii: true,
            color: ColorMode::Off,
            ..PlotOptions::default()
        };
        let series = vec![PlotSeries {
            points: vec![(2.0, 2.0)],
            breaks_after: Vec::new(),
            symbol: Some('*'),
            mode: Some(PlotMode::Scatter),
            label: None,
        }];

        let rendered = render_ascii_plot(&series, &opts);
        let lines: Vec<&str> = rendered.lines().collect();
        let x_labels = lines[opts.height];

        assert!(x_labels.contains('1'));
        assert!(x_labels.contains('2'));
        assert!(x_labels.contains('3'));
        assert!(
            lines
                .iter()
                .any(|line| line.trim_start().starts_with("2 "))
        );
    }
}
