use std::cmp::{max, min};

use indexmap::IndexMap;
use num_traits::ToPrimitive;

use crate::builtins::{
    BuiltinContext, BuiltinEnum as BE, BuiltinFnArgs, at_least_arity_error,
    check_registered_named_args,
};
use crate::cas::{infer_single_cas_var, substitute_cas};
use crate::style::{AnsiColor, ColorMode as StyleColorMode, TextStyle, paint};
use crate::value::seq::ValueSeq;
use crate::value::{Value, WqResult};
use crate::vm::builtin_frame::BuiltinFrameAction;
use crate::vm::pure::PureCallback;
use crate::wqerror::{Bound, Requirement, WqError, WqErrorType};

const EXPECTED_SERIES_ARGUMENT: &str = "expected each argument to be a numeric list, a list of numeric (x;y) points, a callable, a CAS expression, table-shaped data, or a series config dict";
const SERIES_ARGUMENT_EXAMPLES: &str =
    "e.g. (1;2;3), ((1;2);(2;4)), {x*x}, @s x^2, or (`x:(0;1);`y:(2;3))";

pub(crate) fn asciiplot(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_registered_named_args(&args, BE::Asciiplot)?;
    if args.is_empty() {
        return Err(at_least_arity_error(BE::Asciiplot, 1, 0));
    }
    let mut opts = PlotOptions::default();
    let explicit_size = opts.apply_from_named(&args)?;
    #[cfg(target_arch = "wasm32")]
    let _ = explicit_size;
    // Terminal auto-size: only when width/height/size not explicitly set
    #[cfg(not(target_arch = "wasm32"))]
    if !explicit_size && let Some((tw, th)) = vm.stdout_terminal_size() {
        opts.width = tw.saturating_sub(8).clamp(40, 200);
        opts.height = th.saturating_sub(6).clamp(10, 60);
    }
    // Collect series configs
    let mut configs: Vec<SeriesConfig> = Vec::new();
    for arg in args {
        configs.extend(parse_series_arg(&arg, &opts)?);
    }
    if configs.is_empty() {
        return Err(expected_series_arg_error());
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
    let rendered = render_ascii_plot(&all_series, &opts, vm.color_mode());
    vm.write_stdout_line(&rendered).map_err(|error| {
        WqError::new(WqErrorType::Io)
            .src(BE::Asciiplot)
            .attach_note(format!("host I/O error: {error}"))
    })?;
    Ok(Value::empty_list())
}

pub(crate) struct AsciiplotFrame {
    opts: PlotOptions,
    configs: Vec<SeriesConfig>,
    next_config: usize,
    all_series: Vec<PlotSeries>,
    active: Option<CallableSeriesFrame>,
    callback_result: Option<Option<Value>>,
    color_mode: StyleColorMode,
}

struct CallableSeriesFrame {
    config: SeriesConfig,
    func: Value,
    xmin: f64,
    step: f64,
    count: usize,
    next: usize,
    pending_x: Option<f64>,
    samples: Vec<(f64, Option<Value>)>,
}

impl AsciiplotFrame {
    pub(crate) fn new(
        args: &BuiltinFnArgs,
        terminal_size: Option<(usize, usize)>,
        color_mode: StyleColorMode,
    ) -> WqResult<Self> {
        check_registered_named_args(args, BE::Asciiplot)?;
        if args.is_empty() {
            return Err(at_least_arity_error(BE::Asciiplot, 1, 0));
        }
        let mut opts = PlotOptions::default();
        let explicit_size = opts.apply_from_named(args)?;
        #[cfg(not(target_arch = "wasm32"))]
        if !explicit_size && let Some((width, height)) = terminal_size {
            opts.width = width.saturating_sub(8).clamp(40, 200);
            opts.height = height.saturating_sub(6).clamp(10, 60);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (explicit_size, terminal_size);

        let mut configs = Vec::new();
        for arg in args.iter() {
            configs.extend(parse_series_arg(arg, &opts)?);
        }
        if configs.is_empty() {
            return Err(expected_series_arg_error());
        }
        Ok(Self {
            opts,
            all_series: Vec::with_capacity(configs.len()),
            configs,
            next_config: 0,
            active: None,
            callback_result: None,
            color_mode,
        })
    }

    pub(crate) fn accept_callback_result(&mut self, value: Value) {
        self.callback_result = Some(Some(value));
    }

    pub(crate) fn accept_callback_error(&mut self) {
        self.callback_result = Some(None);
    }

    pub(crate) fn step(&mut self) -> WqResult<BuiltinFrameAction> {
        if let Some(value) = self.callback_result.take() {
            let active = self
                .active
                .as_mut()
                .expect("asciiplot callback should have an active series");
            let x = active
                .pending_x
                .take()
                .expect("asciiplot callback should have a pending sample");
            active.samples.push((x, value));
            return Ok(BuiltinFrameAction::Continue);
        }

        if let Some(active) = &mut self.active {
            if active.next < active.count {
                let x = active.xmin + active.step * active.next as f64;
                active.next += 1;
                active.pending_x = Some(x);
                return Ok(BuiltinFrameAction::Call {
                    func: active.func.clone(),
                    args: Value::float(x).into(),
                });
            }
            let active = self.active.take().expect("active series was checked");
            let sampled = if self.opts.complex_mode == ComplexMode::Plane {
                transform_complex_plane(sampled_from_raw_samples(active.samples, &[]))
            } else {
                let samples = active
                    .samples
                    .into_iter()
                    .map(|(x, value)| {
                        (
                            x,
                            value.and_then(|value| {
                                extract_numeric_component(&value, self.opts.complex_mode)
                            }),
                        )
                    })
                    .collect::<Vec<_>>();
                let breaks = real_discontinuity_breaks(&samples);
                sampled_from_raw_samples(samples, &breaks)
            };
            self.all_series.push(plot_series(active.config, sampled));
            return Ok(BuiltinFrameAction::Continue);
        }

        if let Some(config) = self.configs.get(self.next_config).cloned() {
            self.next_config += 1;
            match config.data.clone() {
                SeriesData::Raw(sampled) => {
                    self.all_series.push(plot_series(config, sampled));
                }
                SeriesData::Cas(expr) => {
                    let sampled = sample_cas_series(&expr, &self.opts, config.xlim)?;
                    self.all_series.push(plot_series(config, sampled));
                }
                SeriesData::Callable(func) => {
                    let (xmin, xmax) = config.xlim.or(self.opts.xlim).unwrap_or((-10.0, 10.0));
                    let count = self.opts.samples.unwrap_or(self.opts.width).max(2);
                    let step = if count > 1 {
                        (xmax - xmin) / count.saturating_sub(1) as f64
                    } else {
                        0.0
                    };
                    self.active = Some(CallableSeriesFrame {
                        config,
                        func,
                        xmin,
                        step,
                        count,
                        next: 0,
                        pending_x: None,
                        samples: Vec::with_capacity(count),
                    });
                }
            }
            return Ok(BuiltinFrameAction::Continue);
        }

        Ok(BuiltinFrameAction::HostComplete {
            text: render_ascii_plot(&self.all_series, &self.opts, self.color_mode),
            stderr: false,
            status: None,
        })
    }
}

fn plot_series(config: SeriesConfig, sampled: SampledSeries<f64>) -> PlotSeries {
    PlotSeries {
        points: sampled.points,
        breaks_after: sampled.breaks_after,
        symbol: config.symbol,
        mode: config.mode,
        label: config.label,
    }
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

#[derive(Clone, Default)]
struct SeriesAttrs {
    xlim: Option<(f64, f64)>,
    symbol: Option<char>,
    mode: Option<PlotMode>,
    label: Option<String>,
}

impl SeriesAttrs {
    fn from_map(map: &IndexMap<std::sync::Arc<str>, Value>) -> Self {
        Self {
            xlim: map.get("xlim").and_then(pair_as_f64),
            symbol: map.get("symbol").and_then(parse_series_symbol),
            mode: map.get("mode").and_then(parse_plot_mode),
            label: map.get("label").and_then(Value::try_to_rust_string),
        }
    }

    fn has_config_option(&self) -> bool {
        self.xlim.is_some() || self.symbol.is_some() || self.mode.is_some() || self.label.is_some()
    }

    fn into_config(self, data: SeriesData) -> SeriesConfig {
        SeriesConfig {
            data,
            xlim: self.xlim,
            symbol: self.symbol,
            mode: self.mode,
            label: self.label,
        }
    }
}

fn parse_series_arg(arg: &Value, opts: &PlotOptions) -> WqResult<Vec<SeriesConfig>> {
    if let Some(series) = parse_raw_series_data(arg) {
        return Ok(vec![
            SeriesAttrs::default().into_config(SeriesData::Raw(series)),
        ]);
    }

    match arg {
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
            if let Some(configs) = parse_series_config_dict(map, opts)? {
                Ok(configs)
            } else if let Some(table) = parse_table_arg(arg) {
                table_series_configs(table, opts)
            } else if map.contains_key("fn") {
                Err(WqError::new(WqErrorType::Domain)
                    .src(BE::Asciiplot)
                    .msg("series config 'fn' must be a callable or CAS expression"))
            } else {
                Err(expected_series_arg_error())
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

fn parse_series_config_dict(
    map: &IndexMap<std::sync::Arc<str>, Value>,
    opts: &PlotOptions,
) -> WqResult<Option<Vec<SeriesConfig>>> {
    let attrs = SeriesAttrs::from_map(map);

    if (attrs.has_config_option() || map.len() == 1)
        && let Some(value) = map.get("data")
    {
        return Ok(Some(parse_series_config_data(
            value,
            "series config 'data' must be a numeric list, a list of numeric (x;y) points, a callable, a CAS expression, or table-shaped data",
            &attrs,
            opts,
        )?));
    }

    let data_value = map.get("points").or_else(|| map.get("values"));
    if let Some(value) = data_value {
        return Ok(Some(vec![
            attrs.into_config(parse_raw_series_config_data(value)?),
        ]));
    }

    if let Some(value) = map
        .get("fn")
        .or_else(|| map.get("cas"))
        .or_else(|| map.get("expr"))
    {
        return Ok(Some(vec![attrs.into_config(
            parse_callable_or_cas_series_config_data(
                value,
                "series config 'fn', 'cas', or 'expr' must be a callable or CAS expression",
            )?,
        )]));
    }

    if attrs.has_config_option()
        && let (Some(x_values), Some(y_values)) = (map.get("x"), map.get("y"))
    {
        return Ok(Some(vec![attrs.into_config(SeriesData::Raw(
            parse_xy_series_data(x_values, y_values)?,
        ))]));
    }

    if attrs.has_config_option()
        && let Some(y_values) = map.get("y")
    {
        let Some(series) = parse_raw_series_data(y_values) else {
            return Err(WqError::new(WqErrorType::Domain).src(BE::Asciiplot).msg(
                "series config 'y' must be a numeric list or a list of numeric (x;y) points",
            ));
        };
        return Ok(Some(vec![attrs.into_config(SeriesData::Raw(series))]));
    }

    Ok(None)
}

fn parse_series_config_data(
    value: &Value,
    error_msg: &str,
    attrs: &SeriesAttrs,
    opts: &PlotOptions,
) -> WqResult<Vec<SeriesConfig>> {
    if let Some(series) = parse_raw_series_data(value) {
        return Ok(vec![attrs.clone().into_config(SeriesData::Raw(series))]);
    }
    if let Some(table) = parse_table_arg(value) {
        let mut configs = table_series_configs(table, opts)?;
        apply_series_attrs(&mut configs, attrs);
        return Ok(configs);
    }
    Ok(vec![attrs.clone().into_config(
        parse_callable_or_cas_series_config_data(value, error_msg)?,
    )])
}

fn apply_series_attrs(configs: &mut [SeriesConfig], attrs: &SeriesAttrs) {
    let label = if configs.len() == 1 {
        attrs.label.clone()
    } else {
        None
    };
    for config in configs {
        if attrs.xlim.is_some() {
            config.xlim = attrs.xlim;
        }
        if attrs.symbol.is_some() {
            config.symbol = attrs.symbol;
        }
        if attrs.mode.is_some() {
            config.mode = attrs.mode;
        }
        if let Some(label) = &label {
            config.label = Some(label.clone());
        }
    }
}

fn parse_callable_or_cas_series_config_data(
    value: &Value,
    error_msg: &str,
) -> WqResult<SeriesData> {
    if value.is_cas_expr() {
        return Ok(SeriesData::Cas(value.clone()));
    }
    if value.is_callable() {
        return Ok(SeriesData::Callable(value.clone()));
    }
    Err(WqError::new(WqErrorType::Domain)
        .src(BE::Asciiplot)
        .msg(error_msg))
}

fn parse_raw_series_config_data(value: &Value) -> WqResult<SeriesData> {
    let Some(series) = parse_raw_series_data(value) else {
        return Err(WqError::new(WqErrorType::Domain).src(BE::Asciiplot).msg(
            "series config point data must be a numeric list or a list of numeric (x;y) points",
        ));
    };
    Ok(SeriesData::Raw(series))
}

fn parse_raw_series_data(value: &Value) -> Option<SampledSeries<f64>> {
    if let Value::List(items) = value
        && !items.is_empty()
        && let Some(points) = items.iter().map(numeric_pair).collect::<Option<Vec<_>>>()
    {
        return Some(SampledSeries::from_points(points));
    }
    let ys = numeric_sequence(value)?;
    Some(SampledSeries::from_points(
        ys.into_iter()
            .enumerate()
            .map(|(i, y)| (i as f64, y))
            .collect(),
    ))
}

fn parse_xy_series_data(x_value: &Value, y_value: &Value) -> WqResult<SampledSeries<f64>> {
    let Some(xs) = numeric_sequence(x_value) else {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BE::Asciiplot)
            .msg("series config 'x' must be a numeric list"));
    };
    let Some(ys) = numeric_sequence(y_value) else {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BE::Asciiplot)
            .msg("series config 'y' must be a numeric list"));
    };
    if xs.is_empty() || xs.len() != ys.len() {
        return Err(WqError::new(WqErrorType::Length)
            .src(BE::Asciiplot)
            .msg("series config 'x' and 'y' must have the same non-zero length"));
    }

    Ok(SampledSeries::from_points(xs.into_iter().zip(ys).collect()))
}

fn numeric_sequence(value: &Value) -> Option<Vec<f64>> {
    let items = ValueSeq::from_value(value)?;
    if items.len() == 0 {
        return None;
    }
    items.values().map(|item| item.as_f64()).collect()
}

fn numeric_pair(value: &Value) -> Option<(f64, f64)> {
    let items = ValueSeq::from_value(value)?;
    if items.len() != 2 {
        return None;
    }
    Some((items.get(0)?.as_f64()?, items.get(1)?.as_f64()?))
}

fn parse_series_symbol(value: &Value) -> Option<char> {
    match value {
        Value::Char(c) => Some(*c),
        _ => value.try_to_rust_string().and_then(|s| s.chars().next()),
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
    if map.is_empty() || !map.values().all(is_non_string_table_column) {
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

fn is_non_string_table_column(value: &Value) -> bool {
    match value {
        Value::String(_) => false,
        Value::List(items)
            if !items.is_empty() && items.iter().all(|item| matches!(item, Value::Char(_))) =>
        {
            false
        }
        _ => ValueSeq::from_value(value).is_some(),
    }
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
    ValueSeq::from_value(value).map_or(0, |items| items.len())
}

fn column_item(value: &Value, idx: usize) -> Option<Value> {
    ValueSeq::from_value(value)?.get(idx)
}

fn table_series_configs(table: TableData, opts: &PlotOptions) -> WqResult<Vec<SeriesConfig>> {
    let x_column = opts.table_x.as_deref();
    if let Some(name) = x_column
        && !table.columns.contains_key(name)
    {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BE::Asciiplot)
            .msg(format!("table x column '{name}' was not found")));
    }

    let y_columns = if let Some(columns) = &opts.table_y {
        for name in columns {
            if !table.columns.contains_key(name) {
                return Err(WqError::new(WqErrorType::Domain)
                    .src(BE::Asciiplot)
                    .msg(format!("table y column '{name}' was not found")));
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
                .msg(format!("table y column '{y_name}' has no numeric points")));
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
        .msg(EXPECTED_SERIES_ARGUMENT)
        .attach_note(SERIES_ARGUMENT_EXAMPLES)
}

fn sample_callable_series(
    vm: &mut dyn BuiltinContext,
    func: &Value,
    opts: &PlotOptions,
    xlim: Option<(f64, f64)>,
) -> WqResult<SampledSeries<f64>> {
    let (xmin, xmax) = xlim.or(opts.xlim).unwrap_or((-10.0, 10.0));
    let initial_samples = opts.samples.unwrap_or(opts.width).max(2);
    let pure = PureCallback::compile(func, 1);

    if opts.complex_mode == ComplexMode::Plane {
        let mut sampler =
            |x: f64| -> Option<Value> { sample_callable_value(vm, func, pure.as_ref(), x) };
        let raw = sample_with_segments(xmin, xmax, initial_samples, &mut sampler);
        Ok(transform_complex_plane(raw))
    } else {
        let mut sampler = |x: f64| -> Option<f64> {
            let y = sample_callable_value(vm, func, pure.as_ref(), x)?;
            extract_numeric_component(&y, opts.complex_mode)
        };
        Ok(sample_real_with_segments(
            xmin,
            xmax,
            initial_samples,
            &mut sampler,
        ))
    }
}

fn sample_callable_value(
    vm: &mut dyn BuiltinContext,
    func: &Value,
    pure: Option<&PureCallback>,
    x: f64,
) -> Option<Value> {
    let arg = Value::float(x);
    if let Some(pure) = pure
        && let Some(value) = pure.eval(&[&arg]).ok()?
    {
        return Some(value);
    }
    vm.call(func, arg.into()).ok()
}

fn sample_cas_series(
    expr: &Value,
    opts: &PlotOptions,
    xlim: Option<(f64, f64)>,
) -> WqResult<SampledSeries<f64>> {
    let (xmin, xmax) = xlim.or(opts.xlim).unwrap_or((-10.0, 10.0));
    let initial_samples = opts.samples.unwrap_or(opts.width).max(2);

    let var = Value::from_cas_var(infer_single_cas_var(expr).map_err(|e| e.src(BE::Asciiplot))?);

    if opts.complex_mode == ComplexMode::Plane {
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
            extract_numeric_component(&y, opts.complex_mode)
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

fn extract_numeric_component(value: &Value, mode: ComplexMode) -> Option<f64> {
    match mode {
        ComplexMode::Real | ComplexMode::Plane => expect_real_sample(value),
        ComplexMode::Imaginary => {
            if let Some(z) = value.as_complex64() {
                Some(z.im)
            } else {
                value.as_f64().filter(|y| y.is_finite()).map(|_| 0.0)
            }
        }
        ComplexMode::Abs => {
            if let Some(z) = value.as_complex64() {
                Some(z.norm())
            } else {
                value.as_f64().filter(|y| y.is_finite()).map(|y| y.abs())
            }
        }
        ComplexMode::Argument => {
            if let Some(z) = value.as_complex64() {
                Some(z.arg())
            } else {
                value
                    .as_f64()
                    .filter(|y| y.is_finite())
                    .map(|y| if y < 0.0 { std::f64::consts::PI } else { 0.0 })
            }
        }
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum ComplexMode {
    Real,
    Imaginary,
    Abs,
    Argument,
    Plane,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlotTheme {
    Minimal,
    Maximal,
}

fn option_keyword(value: &Value) -> Option<String> {
    match value {
        Value::Tag(sym) => Some(sym.to_string()),
        _ => value.try_to_rust_string(),
    }
    .map(|s| s.to_ascii_lowercase())
}

fn parse_plot_mode(value: &Value) -> Option<PlotMode> {
    match option_keyword(value)?.as_str() {
        "line" | "l" => Some(PlotMode::Line),
        "scatter" | "sc" => Some(PlotMode::Scatter),
        "step" | "st" => Some(PlotMode::Step),
        "bar" | "b" => Some(PlotMode::Bar),
        "area" | "a" => Some(PlotMode::Area),
        _ => None,
    }
}

fn parse_complex_mode(value: &Value) -> Option<ComplexMode> {
    match option_keyword(value)?.as_str() {
        "re" | "real" => Some(ComplexMode::Real),
        "im" | "imag" | "imaginary" => Some(ComplexMode::Imaginary),
        "abs" => Some(ComplexMode::Abs),
        "arg" => Some(ComplexMode::Argument),
        "plane" => Some(ComplexMode::Plane),
        _ => None,
    }
}

fn parse_axes_mode(value: &Value) -> Option<AxesMode> {
    match value {
        Value::Bool(false) => Some(AxesMode::Off),
        Value::Bool(true) => Some(AxesMode::Full),
        _ => match option_keyword(value)?.as_str() {
            "full" => Some(AxesMode::Full),
            "minimal" => Some(AxesMode::Minimal),
            "off" | "none" => Some(AxesMode::Off),
            _ => None,
        },
    }
}

fn parse_plot_theme(value: &Value) -> Option<PlotTheme> {
    match option_keyword(value)?.as_str() {
        "minimal" => Some(PlotTheme::Minimal),
        "maximal" => Some(PlotTheme::Maximal),
        _ => None,
    }
}

fn parse_column_name(value: &Value) -> Option<String> {
    let name = match value {
        Value::Tag(sym) => sym.to_string(),
        _ => value.try_to_rust_string()?,
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

fn plot_size_from_i64(n: i64, min_value: usize, option: &str, value: &Value) -> WqResult<usize> {
    let n = usize::try_from(n)
        .map_err(|_| plot_size_error(plot_int_size_requirement(), option, value))?;
    ensure_plot_size_fits(n, option, value)?;
    Ok(n.max(min_value))
}

fn plot_size_from_f64(n: f64, min_value: usize, option: &str, value: &Value) -> WqResult<usize> {
    if !n.is_finite() || n < 0.0 {
        return Err(plot_size_error(
            plot_float_size_requirement(),
            option,
            value,
        ));
    }
    let n = n
        .max(min_value as f64)
        .to_usize()
        .ok_or_else(|| plot_size_too_large(option, value))?;
    ensure_plot_size_fits(n, option, value)?;
    Ok(n.max(min_value))
}

fn ensure_plot_size_fits(n: usize, option: &str, value: &Value) -> WqResult<()> {
    let max = usize::try_from(isize::MAX).expect("isize::MAX fits in usize");
    if n > max {
        return Err(plot_size_too_large(option, value));
    }
    Ok(())
}

fn plot_size_too_large(option: &str, value: &Value) -> WqError {
    let requirement = if matches!(value, Value::Float(_)) {
        plot_float_size_requirement()
    } else {
        plot_int_size_requirement()
    };
    plot_size_error(requirement, option, value)
}

fn plot_int_size_requirement() -> Requirement {
    Requirement::int_range(Bound::Included(0), Bound::Included(isize::MAX as i128))
}

fn plot_float_size_requirement() -> Requirement {
    Requirement::phrase(
        format!("finite number from 0 through {}", isize::MAX),
        format!("finite numbers from 0 through {}", isize::MAX),
    )
}

fn plot_size_error(requirement: Requirement, option: &str, value: &Value) -> WqError {
    let (name, component) = option
        .split_once(' ')
        .map_or((option, None), |(name, component)| (name, Some(component)));
    let mut error = WqError::new(WqErrorType::Domain)
        .src(BE::Asciiplot)
        .expected(requirement)
        .at_named_arg(name);
    if let Some(component) = component {
        error = error.attach_note(format!("at '{component}' component"));
    }
    error.got1(value)
}

fn plot_size_to_isize(n: usize) -> isize {
    isize::try_from(n).expect("plot size checked to fit in isize")
}

fn clamped_plot_coord(value: isize, len: usize) -> usize {
    let hi = plot_size_to_isize(len.saturating_sub(1));
    let value = min(hi, max(0, value));
    usize::try_from(value).expect("clamped plot coordinate is non-negative")
}

fn grid_index(grid: &[Vec<Cell>], x: isize, y: isize) -> Option<(usize, usize)> {
    if x < 0 || y < 0 {
        return None;
    }
    let x = usize::try_from(x).ok()?;
    let y = usize::try_from(y).ok()?;
    let width = grid.first().map_or(0, Vec::len);
    (y < grid.len() && x < width).then_some((x, y))
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
    complex_mode: ComplexMode,
    unicode: bool,
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
    Custom(Vec<AnsiColor>),
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
            complex_mode: ComplexMode::Real,
            unicode: false,
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

        if let Some(theme) = args.named("theme").and_then(parse_plot_theme) {
            self.apply_theme(theme);
        }
        if let Some(size) = args.named("size")
            && let Some((a, b)) = pair_as_f64(size)
        {
            self.width = plot_size_from_f64(a, 10, "size width", &Value::float(a))?;
            self.height = plot_size_from_f64(b, 5, "size height", &Value::float(b))?;
            explicit_size = true;
        }
        if let Some(width) = args.named("width")
            && let Some(n) = width.as_i64()
        {
            self.width = plot_size_from_i64(n, 10, "width", width)?;
            explicit_size = true;
        }
        if let Some(height) = args.named("height")
            && let Some(n) = height.as_i64()
        {
            self.height = plot_size_from_i64(n, 5, "height", height)?;
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
                        if let Some(s) = it.try_to_rust_string()
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
            if let Some(s) = val.try_to_rust_string() {
                if let Some(c) = s.chars().next() {
                    self.symbols = vec![c];
                }
            } else if let Value::Char(c) = val {
                self.symbols = vec![*c];
            }
        }
        if let Some(axes) = args.named("axes").and_then(parse_axes_mode) {
            self.axes = axes;
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
                let n = plot_size_from_i64(n, 1, "grid", v)?;
                self.grid = GridMode::Density(n, n);
            } else if let Some((a, b)) = pair_as_f64(v) {
                self.grid = GridMode::Density(
                    plot_size_from_f64(a, 1, "grid width", &Value::float(a))?,
                    plot_size_from_f64(b, 1, "grid height", &Value::float(b))?,
                );
            }
        }
        if let Some(Value::List(items)) = args.named("labels") {
            let mut labs = Vec::new();
            for it in items.iter() {
                if let Some(s) = it.try_to_rust_string() {
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
        if let Some(samples) = args.named("samples")
            && let Some(v) = samples.as_i64()
        {
            self.samples = Some(plot_size_from_i64(v, 1, "samples", samples)?);
        }
        if let Some(mode) = args.named("complex").and_then(parse_complex_mode) {
            self.complex_mode = mode;
        }
        if let Some(Value::Bool(b)) = args.named("unicode") {
            self.unicode = *b;
        }
        if let Some(Value::Bool(b)) = args.named("ticklabels") {
            self.tick_labels = *b;
        }
        if let Some(v) = args.named("title")
            && let Some(s) = v.try_to_rust_string()
        {
            self.title = Some(s);
        }
        if let Some(v) = args.named("xlabel")
            && let Some(s) = v.try_to_rust_string()
        {
            self.xlabel = Some(s);
        }
        if let Some(v) = args.named("ylabel")
            && let Some(s) = v.try_to_rust_string()
        {
            self.ylabel = Some(s);
        }
        if let Some(Value::List(items)) = args.named("caption")
            && items.len() >= 3
        {
            if let Some(s) = items[0].try_to_rust_string() {
                self.title = Some(s);
            }
            if let Some(s) = items[1].try_to_rust_string() {
                self.xlabel = Some(s);
            }
            if let Some(s) = items[2].try_to_rust_string() {
                self.ylabel = Some(s);
            }
        }

        Ok(explicit_size)
    }

    fn apply_theme(&mut self, theme: PlotTheme) {
        match theme {
            PlotTheme::Minimal => {
                self.axes = AxesMode::Off;
                self.grid = GridMode::Off;
                self.color = ColorMode::On;
            }
            PlotTheme::Maximal => {
                self.axes = AxesMode::Full;
                self.grid = GridMode::On;
                self.color = ColorMode::On;
            }
        }
    }
}

fn pair_as_f64(value: &Value) -> Option<(f64, f64)> {
    numeric_pair(value)
}

fn render_ascii_plot(
    series_list: &[PlotSeries],
    opts: &PlotOptions,
    color_mode: StyleColorMode,
) -> String {
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
        clamped_plot_coord(row, height)
    } else {
        height - 1
    };
    let x0_col = if x0_in {
        let t = (0.0 - xmin) / xspan;
        let col = (t * (width as f64 - 1.0)).round() as isize;
        clamped_plot_coord(col, width)
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
        let grid_color = Some(AnsiColor::BrightBlack);
        let ch_h = if opts.unicode { '┈' } else { '.' };
        let ch_v = if opts.unicode { '┊' } else { ':' };
        for yv in yticks {
            let t = (yv - ymin) / yspan;
            let row = (height as f64 - 1.0 - t * (height as f64 - 1.0)).round() as isize;
            let r = clamped_plot_coord(row, height);
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
            let c = clamped_plot_coord(col, width);
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
            let axis_h = if opts.unicode { '─' } else { '-' };
            let axis_v = if opts.unicode { '│' } else { '|' };
            let cross = if opts.unicode { '┼' } else { '+' };
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
            let axis_h = if opts.unicode { '─' } else { '-' };
            let axis_v = if opts.unicode { '│' } else { '|' };
            let cross = if opts.unicode { '┼' } else { '+' };
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
                let c_col = usize::try_from(c).expect("clamped x tick is non-negative");
                if c_col != x0_col {
                    set_cell_layer(
                        &mut grid,
                        c,
                        y0_row as isize,
                        if opts.unicode { '┼' } else { '+' },
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
                let r_row = usize::try_from(r).expect("clamped y tick is non-negative");
                if r_row != y0_row {
                    set_cell_layer(
                        &mut grid,
                        x0_col as isize,
                        r,
                        if opts.unicode { '┼' } else { '+' },
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
        let color: Option<AnsiColor> = match &opts.color {
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
                            set_area_cell(
                                &mut grid,
                                x,
                                yy,
                                symbol,
                                color,
                                opts.color.is_on(),
                                si,
                                opts.unicode,
                            );
                        }
                    }
                }
                if pts.len() == 1 {
                    let (x, y) = pts[0];
                    let ystart = min(baseline_row, y);
                    let yend = max(baseline_row, y);
                    for yy in ystart..=yend {
                        set_area_cell(
                            &mut grid,
                            x,
                            yy,
                            symbol,
                            color,
                            opts.color.is_on(),
                            si,
                            opts.unicode,
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
            line.push_str(&paint_char_with_color_mode(cell.ch, cell.color, color_mode));
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
    // Legend (optional)
    // shown when global or per-series labels are provided
    let legend_len = opts.labels.as_ref().map_or(series_list.len(), |labels| {
        labels.len().max(series_list.len())
    });
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
        '*'
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
            let col = clamped_plot_coord(col, width);
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
        let row = clamped_plot_coord(row, height);
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
    color: Option<AnsiColor>,
    area_mask: u128,
    area_count: u16,
}
impl Cell {
    fn new(ch: char) -> Self {
        Self {
            ch,
            layer: Layer::Grid,
            color: None,
            area_mask: 0,
            area_count: 0,
        }
    }
}

fn set_cell_layer(
    grid: &mut [Vec<Cell>],
    x: isize,
    y: isize,
    ch: char,
    layer: Layer,
    color: Option<AnsiColor>,
    color_on: bool,
) {
    if let Some((x, y)) = grid_index(grid, x, y) {
        let cell = &mut grid[y][x];
        if layer >= cell.layer || cell.ch == ' ' {
            cell.ch = ch;
            cell.layer = layer;
            cell.color = if color_on { color } else { None };
            if layer == Layer::Data {
                cell.area_mask = 0;
                cell.area_count = 0;
            }
        }
    }
}

fn set_area_cell(
    grid: &mut [Vec<Cell>],
    x: isize,
    y: isize,
    ch: char,
    color: Option<AnsiColor>,
    color_on: bool,
    series_idx: usize,
    unicode: bool,
) {
    let Some((x, y)) = grid_index(grid, x, y) else {
        return;
    };

    let cell = &mut grid[y][x];
    let series_bit = area_series_bit(series_idx);
    if let Some(bit) = series_bit
        && cell.area_mask & bit != 0
    {
        return;
    }

    let overlaps_area = cell.layer == Layer::Data && cell.area_count > 0;
    if overlaps_area {
        cell.ch = area_overlap_char(unicode);
        cell.layer = Layer::Data;
        cell.color = if color_on {
            mix_area_colors(cell.color, color)
        } else {
            None
        };
    } else if Layer::Data >= cell.layer || cell.ch == ' ' {
        cell.ch = ch;
        cell.layer = Layer::Data;
        cell.color = if color_on { color } else { None };
    }

    if let Some(bit) = series_bit {
        cell.area_mask |= bit;
    }
    cell.area_count = cell.area_count.saturating_add(1);
}

fn area_series_bit(series_idx: usize) -> Option<u128> {
    if u32::try_from(series_idx).is_ok_and(|idx| idx < u128::BITS) {
        Some(1_u128 << series_idx)
    } else {
        None
    }
}

fn area_overlap_char(unicode: bool) -> char {
    if unicode { '▓' } else { '%' }
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
    color: Option<AnsiColor>,
) {
    for (x, y) in rasterize_line(x0, y0, x1, y1) {
        set_cell_layer(grid, x, y, ch, Layer::Data, color, color.is_some());
    }
}

fn series_color(idx: usize) -> AnsiColor {
    use AnsiColor::*;
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

fn mix_area_colors(existing: Option<AnsiColor>, incoming: Option<AnsiColor>) -> Option<AnsiColor> {
    match (existing, incoming) {
        (Some(a), Some(b)) => Some(mix_ansi_colors(a, b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn mix_ansi_colors(a: AnsiColor, b: AnsiColor) -> AnsiColor {
    let (a_mask, a_bright) = color_rgb_mask(a);
    let (b_mask, b_bright) = color_rgb_mask(b);
    ansi_from_rgb_mask(a_mask | b_mask, a_bright || b_bright)
}

fn color_rgb_mask(color: AnsiColor) -> (u8, bool) {
    match color {
        AnsiColor::Black => (0, false),
        AnsiColor::Red => (0b001, false),
        AnsiColor::Green => (0b010, false),
        AnsiColor::Yellow => (0b011, false),
        AnsiColor::Blue => (0b100, false),
        AnsiColor::Magenta | AnsiColor::Purple => (0b101, false),
        AnsiColor::Cyan => (0b110, false),
        AnsiColor::White => (0b111, false),
        AnsiColor::BrightBlack => (0, true),
        AnsiColor::BrightRed => (0b001, true),
        AnsiColor::BrightGreen => (0b010, true),
        AnsiColor::BrightYellow => (0b011, true),
        AnsiColor::BrightBlue => (0b100, true),
        AnsiColor::BrightMagenta => (0b101, true),
        AnsiColor::BrightCyan => (0b110, true),
        AnsiColor::BrightWhite => (0b111, true),
    }
}

fn ansi_from_rgb_mask(mask: u8, bright: bool) -> AnsiColor {
    match (mask, bright) {
        (0, false) => AnsiColor::Black,
        (0, true) => AnsiColor::BrightBlack,
        (0b001, false) => AnsiColor::Red,
        (0b001, true) => AnsiColor::BrightRed,
        (0b010, false) => AnsiColor::Green,
        (0b010, true) => AnsiColor::BrightGreen,
        (0b011, false) => AnsiColor::Yellow,
        (0b011, true) => AnsiColor::BrightYellow,
        (0b100, false) => AnsiColor::Blue,
        (0b100, true) => AnsiColor::BrightBlue,
        (0b101, false) => AnsiColor::Magenta,
        (0b101, true) => AnsiColor::BrightMagenta,
        (0b110, false) => AnsiColor::Cyan,
        (0b110, true) => AnsiColor::BrightCyan,
        (_, false) => AnsiColor::White,
        (_, true) => AnsiColor::BrightWhite,
    }
}

fn paint_char_with_color_mode(
    ch: char,
    color: Option<AnsiColor>,
    color_mode: StyleColorMode,
) -> String {
    let s = ch.to_string();
    if let Some(c) = color {
        paint(&s, TextStyle::new().fg(c), color_mode)
    } else {
        s
    }
}

fn parse_palette(val: &Value) -> Option<Vec<AnsiColor>> {
    use AnsiColor::*;
    fn parse_color(value: &Value) -> Option<AnsiColor> {
        match option_keyword(value)?.as_str() {
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
                if let Some(c) = parse_color(it) {
                    out.push(c);
                }
            }
            if out.is_empty() { None } else { Some(out) }
        }
        // Single value: "red"
        _ => parse_color(val).map(|c| vec![c]),
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
    use std::sync::{Arc, Mutex};

    use indexmap::IndexMap;
    use smallvec::smallvec;

    use super::*;
    use crate::builtins::Builtins;
    use crate::session::stdio::{WqIoError, WqOutput};
    use crate::value::cas::{CasFunction, CasOp};
    use crate::value::func::FunctionData;
    use crate::vm::Vm;
    use crate::vm::inst::{Instruction, Operand};

    struct SinkStdout;

    impl WqOutput for SinkStdout {
        fn write(&mut self, _text: &str) -> Result<(), WqIoError> {
            Ok(())
        }
    }

    struct CaptureStdout(Arc<Mutex<String>>);

    impl WqOutput for CaptureStdout {
        fn write(&mut self, text: &str) -> Result<(), WqIoError> {
            self.0
                .lock()
                .expect("plot capture lock should not be poisoned")
                .push_str(text);
            Ok(())
        }
    }

    fn render_with_session_color(color_mode: StyleColorMode) -> String {
        let output = Arc::new(Mutex::new(String::new()));
        let mut vm = Vm::new(Vec::new());
        vm.color_mode = color_mode;
        vm.runtime_io
            .set_stdout(Box::new(CaptureStdout(Arc::clone(&output))));
        asciiplot(
            &mut vm,
            BuiltinFnArgs::with_named(
                smallvec![Value::IntList(Arc::new(vec![1, 2, 3]))],
                vec![
                    (
                        Arc::from("size"),
                        Value::List(Arc::new(vec![Value::Int(12), Value::Int(5)])),
                    ),
                    (Arc::from("color"), Value::Bool(true)),
                ],
            ),
        )
        .expect("asciiplot should render");
        output
            .lock()
            .expect("plot capture lock should not be poisoned")
            .clone()
    }

    #[test]
    fn asciiplot_reports_the_actual_minimum_arity() {
        let mut vm = Vm::new(Vec::new());
        let error = asciiplot(&mut vm, BuiltinFnArgs::with_named(smallvec![], Vec::new()))
            .expect_err("asciiplot without series should fail");

        assert_eq!(
            error.msg.as_deref(),
            Some("expected at least 1 argument, got 0")
        );
        assert_eq!(error.notes.as_slice(), [BE::Asciiplot.usage()]);
    }

    #[test]
    fn asciiplot_uses_the_session_stdout_color_mode() {
        assert!(!render_with_session_color(StyleColorMode::Never).contains("\x1b["));
        assert!(render_with_session_color(StyleColorMode::Always).contains("\x1b["));
    }

    fn make_fn(params: Option<&[&str]>, locals: u16, instructions: Vec<Instruction>) -> Value {
        Value::CompiledFunction(Arc::new(FunctionData {
            params: params.map(|names| {
                Arc::<[String]>::from(names.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            }),
            named_params: None,
            locals,
            isolated_module: false,
            instructions: instructions.into(),
            dbg_chunk: None,
            dbg_stmt_spans: None,
            dbg_source_base_offset: 0,
            dbg_pc_spans: None,
            dbg_stmt_marks: None,
            dbg_local_names: None,
            dbg_provenance: None,
        }))
    }

    fn assert_raw_points(config: &SeriesConfig, expected: &[(f64, f64)]) {
        match &config.data {
            SeriesData::Raw(series) => assert_eq!(series.points.as_slice(), expected),
            SeriesData::Callable(_) | SeriesData::Cas(_) => {
                panic!("expected table-shaped data to produce raw points")
            }
        }
    }

    fn string_value(s: &str) -> Value {
        Value::String(Arc::new(s.to_owned()))
    }

    fn tag_value(s: &'static str) -> Value {
        Value::Tag(Arc::from(s))
    }

    fn mode_value(mode: &str) -> (Arc<str>, Value) {
        (Arc::from("mode"), string_value(mode))
    }

    #[test]
    fn invalid_series_arguments_share_one_requirement() {
        let expected = "expected each argument to be a numeric list, a list of numeric (x;y) points, a callable, a CAS expression, table-shaped data, or a series config dict";
        let opts = PlotOptions::default();
        let invalid_values = [
            Value::Bool(true),
            Value::Dict(Arc::new(IndexMap::from([(
                "unknown".into(),
                Value::Int(1),
            )]))),
        ];

        for value in invalid_values {
            let error = parse_series_arg(&value, &opts)
                .err()
                .expect("invalid series argument should fail");
            assert_eq!(error.msg.as_deref(), Some(expected));
        }
    }

    #[test]
    fn series_data_errors_use_numeric_list_terminology() {
        let error = parse_raw_series_config_data(&Value::Tag("bad".into()))
            .err()
            .expect("invalid point data should fail");

        assert_eq!(
            error.msg.as_deref(),
            Some(
                "series config point data must be a numeric list or a list of numeric (x;y) points"
            )
        );
    }

    #[test]
    fn raw_series_accepts_virtual_ranges_and_packed_point_pairs() {
        let range = Value::IntRange(Arc::new(crate::value::seq::IntRangeData::new(2, 2, 3)));
        let series = parse_raw_series_data(&range).expect("range is a numeric series");
        assert_eq!(series.points, vec![(0.0, 2.0), (1.0, 4.0), (2.0, 6.0)]);

        let points = Value::List(Arc::new(vec![
            Value::IntList(Arc::new(vec![10, 3])),
            Value::FloatList(Arc::new(vec![
                ordered_float::OrderedFloat(20.0),
                ordered_float::OrderedFloat(4.5),
            ])),
        ]));
        let series = parse_raw_series_data(&points).expect("packed pairs are explicit points");
        assert_eq!(series.points, vec![(10.0, 3.0), (20.0, 4.5)]);

        let empty_range = Value::IntRange(Arc::new(crate::value::seq::IntRangeData::new(0, 1, 0)));
        assert!(is_non_string_table_column(&empty_range));
        assert!(!is_non_string_table_column(&Value::String(Arc::new(
            String::new()
        ))));
    }

    #[test]
    fn plot_options_reject_negative_width() {
        let mut opts = PlotOptions::default();
        let err = opts
            .apply_from_named(&BuiltinFnArgs::with_named(
                smallvec![],
                vec![(Arc::from("width"), Value::Int(-1))],
            ))
            .expect_err("negative width should fail");

        assert_eq!(
            err.msg.as_deref(),
            Some(format!("expected int from 0 through {}", isize::MAX).as_str())
        );
        assert_eq!(
            err.notes.as_slice(),
            ["at named argument 'width'", "got -1 (int)"]
        );
    }

    #[test]
    fn plot_options_reject_negative_samples() {
        let mut opts = PlotOptions::default();
        let err = opts
            .apply_from_named(&BuiltinFnArgs::with_named(
                smallvec![],
                vec![(Arc::from("samples"), Value::Int(-1))],
            ))
            .expect_err("negative samples should fail");

        assert_eq!(
            err.msg.as_deref(),
            Some(format!("expected int from 0 through {}", isize::MAX).as_str())
        );
        assert_eq!(
            err.notes.as_slice(),
            ["at named argument 'samples'", "got -1 (int)"]
        );
    }

    #[test]
    fn plot_options_reject_negative_float_pair_size() {
        let mut opts = PlotOptions::default();
        let size = Value::List(Arc::new(vec![Value::float(-1.0), Value::float(5.0)]));
        let err = opts
            .apply_from_named(&BuiltinFnArgs::with_named(
                smallvec![],
                vec![(Arc::from("size"), size)],
            ))
            .expect_err("negative size width should fail");

        assert_eq!(
            err.msg.as_deref(),
            Some(format!("expected finite number from 0 through {}", isize::MAX).as_str())
        );
        assert_eq!(
            err.notes.as_slice(),
            [
                "at named argument 'size'",
                "at 'width' component",
                "got -1.0 (float)",
            ]
        );
    }

    #[test]
    fn plot_options_reject_negative_float_pair_grid() {
        let mut opts = PlotOptions::default();
        let grid = Value::List(Arc::new(vec![Value::float(-1.0), Value::float(2.0)]));
        let err = opts
            .apply_from_named(&BuiltinFnArgs::with_named(
                smallvec![],
                vec![(Arc::from("grid"), grid)],
            ))
            .expect_err("negative grid width should fail");

        assert_eq!(
            err.msg.as_deref(),
            Some(format!("expected finite number from 0 through {}", isize::MAX).as_str())
        );
        assert_eq!(
            err.notes.as_slice(),
            [
                "at named argument 'grid'",
                "at 'width' component",
                "got -1.0 (float)",
            ]
        );
    }

    #[test]
    fn maximal_theme_sets_full_axes_grid_and_color() {
        let mut opts = PlotOptions::default();

        opts.apply_from_named(&BuiltinFnArgs::with_named(
            smallvec![],
            vec![(Arc::from("theme"), string_value("maximal"))],
        ))
        .expect("theme option should parse");

        assert!(matches!(opts.axes, AxesMode::Full));
        assert!(matches!(opts.grid, GridMode::On));
        assert!(matches!(opts.color, ColorMode::On));
    }

    #[test]
    fn named_options_override_theme() {
        let mut opts = PlotOptions::default();

        opts.apply_from_named(&BuiltinFnArgs::with_named(
            smallvec![],
            vec![
                (Arc::from("theme"), string_value("minimal")),
                (Arc::from("axes"), Value::Bool(true)),
                (Arc::from("grid"), Value::Bool(true)),
                (Arc::from("color"), Value::Bool(false)),
            ],
        ))
        .expect("theme option should parse");

        assert!(matches!(opts.axes, AxesMode::Full));
        assert!(matches!(opts.grid, GridMode::On));
        assert!(matches!(opts.color, ColorMode::Off));
    }

    #[test]
    fn keyword_options_accept_tags() {
        let mut opts = PlotOptions::default();

        opts.apply_from_named(&BuiltinFnArgs::with_named(
            smallvec![],
            vec![
                (Arc::from("theme"), tag_value("minimal")),
                (Arc::from("axes"), tag_value("full")),
                (Arc::from("mode"), tag_value("scatter")),
                (Arc::from("complex"), tag_value("abs")),
            ],
        ))
        .expect("tag keyword options should parse");

        assert!(matches!(opts.axes, AxesMode::Full));
        assert!(matches!(opts.grid, GridMode::Off));
        assert!(matches!(opts.mode, PlotMode::Scatter));
        assert!(matches!(opts.complex_mode, ComplexMode::Abs));
    }

    #[test]
    fn complex_mode_extracts_components() {
        let z = Value::from_complex64(num_complex::Complex64::new(3.0, 4.0));

        assert_eq!(
            extract_numeric_component(&Value::float(3.0), ComplexMode::Real),
            Some(3.0)
        );
        assert_eq!(extract_numeric_component(&z, ComplexMode::Real), None);
        assert_eq!(
            extract_numeric_component(&z, ComplexMode::Imaginary),
            Some(4.0)
        );
        assert_eq!(extract_numeric_component(&z, ComplexMode::Abs), Some(5.0));
        assert_eq!(
            extract_numeric_component(&z, ComplexMode::Argument),
            Some((4.0_f64).atan2(3.0))
        );
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
    fn sample_callable_series_uses_pure_user_callback() {
        let opts = PlotOptions {
            width: 3,
            xlim: Some((0.0, 2.0)),
            ..PlotOptions::default()
        };
        let mut vm = Vm::new(vec![]);
        vm.max_call_depth = 0;
        let f = make_fn(
            Some(&["x"]),
            1,
            vec![
                Instruction::LoadLocal(0),
                Instruction::load_const(Value::Int(1)),
                Instruction::binary_op(
                    crate::ast::BinaryOperator::Add,
                    Operand::Stack,
                    Operand::Stack,
                ),
                Instruction::Return,
            ],
        );

        let series =
            sample_callable_series(&mut vm, &f, &opts, None).expect("pure callable should sample");

        assert_eq!(series.points, vec![(0.0, 1.0), (1.0, 2.0), (2.0, 3.0)]);
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
    fn series_config_accepts_raw_data_with_per_series_mode() {
        let value = Value::Dict(Arc::new(IndexMap::from([
            ("data".into(), Value::IntList(Arc::new(vec![1, 3, 2]))),
            ("mode".into(), string_value("bar")),
            ("label".into(), string_value("counts")),
        ])));

        let configs = parse_series_arg(&value, &PlotOptions::default())
            .expect("raw series config should parse");

        assert_eq!(configs.len(), 1);
        assert!(matches!(configs[0].mode, Some(PlotMode::Bar)));
        assert_eq!(configs[0].label.as_deref(), Some("counts"));
        assert_raw_points(&configs[0], &[(0.0, 1.0), (1.0, 3.0), (2.0, 2.0)]);
    }

    #[test]
    fn series_config_accepts_xy_lists_with_per_series_mode() {
        let value = Value::Dict(Arc::new(IndexMap::from([
            ("x".into(), Value::IntList(Arc::new(vec![10, 20, 30]))),
            ("y".into(), Value::IntList(Arc::new(vec![2, 4, 3]))),
            ("mode".into(), string_value("scatter")),
        ])));

        let configs = parse_series_arg(&value, &PlotOptions::default())
            .expect("xy series config should parse");

        assert_eq!(configs.len(), 1);
        assert!(matches!(configs[0].mode, Some(PlotMode::Scatter)));
        assert_raw_points(&configs[0], &[(10.0, 2.0), (20.0, 4.0), (30.0, 3.0)]);
    }

    #[test]
    fn series_config_accepts_callable_data_with_per_series_mode() {
        let value = Value::Dict(Arc::new(IndexMap::from([
            ("data".into(), Value::builtin_function("abs", Builtins::ABS)),
            ("mode".into(), string_value("line")),
            ("label".into(), string_value("abs")),
        ])));

        let configs = parse_series_arg(&value, &PlotOptions::default())
            .expect("callable data series config should parse");

        assert_eq!(configs.len(), 1);
        assert!(matches!(configs[0].data, SeriesData::Callable(_)));
        assert!(matches!(configs[0].mode, Some(PlotMode::Line)));
        assert_eq!(configs[0].label.as_deref(), Some("abs"));
    }

    #[test]
    fn series_config_accepts_cas_data_with_per_series_mode() {
        let value = Value::Dict(Arc::new(IndexMap::from([
            (
                "data".into(),
                Value::from_cas_op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
            ),
            ("mode".into(), string_value("area")),
        ])));

        let configs = parse_series_arg(&value, &PlotOptions::default())
            .expect("CAS data series config should parse");

        assert_eq!(configs.len(), 1);
        assert!(matches!(configs[0].data, SeriesData::Cas(_)));
        assert!(matches!(configs[0].mode, Some(PlotMode::Area)));
    }

    #[test]
    fn table_column_named_data_still_parses_as_table_data_without_config() {
        let value = Value::Dict(Arc::new(IndexMap::from([
            ("x".into(), Value::IntList(Arc::new(vec![0, 1, 2]))),
            ("data".into(), Value::IntList(Arc::new(vec![2, 4, 3]))),
        ])));
        let opts = PlotOptions {
            table_x: Some("x".to_string()),
            ..PlotOptions::default()
        };

        let configs = parse_series_arg(&value, &opts).expect("data column table should parse");

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].label.as_deref(), Some("data"));
        assert_raw_points(&configs[0], &[(0.0, 2.0), (1.0, 4.0), (2.0, 3.0)]);
    }

    #[test]
    fn series_config_accepts_table_data_with_per_series_mode() {
        let table = Value::Dict(Arc::new(IndexMap::from([
            ("x".into(), Value::IntList(Arc::new(vec![0, 1, 2]))),
            ("sin".into(), Value::IntList(Arc::new(vec![0, 1, 0]))),
            ("cos".into(), Value::IntList(Arc::new(vec![1, 0, -1]))),
        ])));
        let value = Value::Dict(Arc::new(IndexMap::from([
            ("data".into(), table),
            ("mode".into(), string_value("scatter")),
            ("symbol".into(), string_value("x")),
        ])));
        let opts = PlotOptions {
            table_x: Some("x".to_string()),
            table_y: Some(vec!["sin".to_string(), "cos".to_string()]),
            ..PlotOptions::default()
        };

        let configs = parse_series_arg(&value, &opts).expect("table data config should parse");

        assert_eq!(configs.len(), 2);
        assert!(
            configs
                .iter()
                .all(|config| matches!(config.mode, Some(PlotMode::Scatter)))
        );
        assert!(configs.iter().all(|config| config.symbol == Some('x')));
        assert_eq!(configs[0].label.as_deref(), Some("sin"));
        assert_eq!(configs[1].label.as_deref(), Some("cos"));
        assert_raw_points(&configs[0], &[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)]);
        assert_raw_points(&configs[1], &[(0.0, 1.0), (1.0, 0.0), (2.0, -1.0)]);
    }

    #[test]
    fn plain_xy_dict_still_parses_as_table_data() {
        let value = Value::Dict(Arc::new(IndexMap::from([
            ("x".into(), Value::IntList(Arc::new(vec![0, 1, 2]))),
            ("y".into(), Value::IntList(Arc::new(vec![2, 4, 3]))),
        ])));
        let opts = PlotOptions {
            table_x: Some("x".to_string()),
            ..PlotOptions::default()
        };

        let configs = parse_series_arg(&value, &opts).expect("xy table should still parse");

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].label.as_deref(), Some("y"));
        assert_raw_points(&configs[0], &[(0.0, 2.0), (1.0, 4.0), (2.0, 3.0)]);
    }

    #[test]
    fn asciiplot_accepts_single_cas_arg() {
        let expr = Value::from_cas_function(CasFunction::Sin, vec![Value::from_cas_var("x")]);
        let mut vm = Vm::new(vec![]);
        vm.runtime_io.set_stdout(Box::new(SinkStdout));
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
        assert_eq!(result.unwrap(), Value::empty_list());
    }

    #[test]
    fn asciiplot_accepts_callable_and_cas_for_each_mode() {
        let callable = Value::builtin_function("abs", Builtins::ABS);
        let cas = Value::from_cas_op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]);

        for mode in ["line", "scatter", "step", "bar", "area"] {
            let mut vm = Vm::new(vec![]);
            vm.runtime_io.set_stdout(Box::new(SinkStdout));
            let callable_result = asciiplot(
                &mut vm,
                BuiltinFnArgs::with_named(
                    smallvec![callable.clone()],
                    vec![
                        mode_value(mode),
                        (
                            Arc::from("size"),
                            Value::List(Arc::new(vec![Value::Int(12), Value::Int(5)])),
                        ),
                        (
                            Arc::from("xlim"),
                            Value::List(Arc::new(vec![Value::Int(-2), Value::Int(2)])),
                        ),
                        (Arc::from("samples"), Value::Int(5)),
                        (Arc::from("color"), Value::Bool(false)),
                    ],
                ),
            );
            assert_eq!(callable_result.unwrap(), Value::empty_list());

            let mut vm = Vm::new(vec![]);
            vm.runtime_io.set_stdout(Box::new(SinkStdout));
            let cas_result = asciiplot(
                &mut vm,
                BuiltinFnArgs::with_named(
                    smallvec![cas.clone()],
                    vec![
                        mode_value(mode),
                        (
                            Arc::from("size"),
                            Value::List(Arc::new(vec![Value::Int(12), Value::Int(5)])),
                        ),
                        (
                            Arc::from("xlim"),
                            Value::List(Arc::new(vec![Value::Int(-2), Value::Int(2)])),
                        ),
                        (Arc::from("samples"), Value::Int(5)),
                        (Arc::from("color"), Value::Bool(false)),
                    ],
                ),
            );
            assert_eq!(cas_result.unwrap(), Value::empty_list());
        }
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

        let rendered = render_ascii_plot(&series, &opts, StyleColorMode::Never);

        assert!(rendered.contains("sine(s)"));
        assert!(rendered.contains("cosine(c)"));
    }

    #[test]
    fn plot_chars_use_explicit_style_renderer() {
        assert_eq!(
            paint_char_with_color_mode('*', Some(AnsiColor::BrightCyan), StyleColorMode::Always),
            "\x1b[96m*\x1b[0m"
        );
        assert_eq!(
            paint_char_with_color_mode('*', Some(AnsiColor::BrightCyan), StyleColorMode::Never),
            "*"
        );
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

        let rendered = render_ascii_plot(&series, &opts, StyleColorMode::Never);
        let plotted_points = rendered.chars().filter(|&ch| ch == '*').count();

        assert_eq!(plotted_points, 3);
    }

    #[test]
    fn render_area_overlap_marks_shared_fill_when_color_is_off() {
        let opts = PlotOptions {
            width: 5,
            height: 5,
            xlim: Some((0.0, 1.0)),
            ylim: Some((0.0, 1.0)),
            mode: PlotMode::Area,
            axes: AxesMode::Off,
            color: ColorMode::Off,
            ..PlotOptions::default()
        };
        let series = vec![
            PlotSeries {
                points: vec![(0.0, 1.0), (1.0, 1.0)],
                breaks_after: Vec::new(),
                symbol: Some('a'),
                mode: None,
                label: None,
            },
            PlotSeries {
                points: vec![(0.0, 1.0), (1.0, 1.0)],
                breaks_after: Vec::new(),
                symbol: Some('b'),
                mode: None,
                label: None,
            },
        ];

        let rendered = render_ascii_plot(&series, &opts, StyleColorMode::Never);

        assert!(rendered.contains(area_overlap_char(false)));
    }

    #[test]
    fn render_defaults_to_ascii_glyphs() {
        let opts = PlotOptions {
            width: 5,
            height: 5,
            xlim: Some((-1.0, 1.0)),
            ylim: Some((-1.0, 1.0)),
            axes: AxesMode::Minimal,
            color: ColorMode::Off,
            ..PlotOptions::default()
        };
        let series = vec![PlotSeries {
            points: vec![(1.0, 1.0)],
            breaks_after: Vec::new(),
            symbol: None,
            mode: Some(PlotMode::Scatter),
            label: None,
        }];

        let rendered = render_ascii_plot(&series, &opts, StyleColorMode::Never);

        assert!(rendered.contains('+'));
        assert!(rendered.contains('-'));
        assert!(rendered.contains('|'));
        assert!(rendered.contains('·'));
        assert!(!rendered.contains('┼'));
        assert!(!rendered.contains('─'));
        assert!(!rendered.contains('│'));
    }

    #[test]
    fn render_can_opt_into_unicode_glyphs() {
        let opts = PlotOptions {
            width: 5,
            height: 5,
            xlim: Some((-1.0, 1.0)),
            ylim: Some((-1.0, 1.0)),
            axes: AxesMode::Minimal,
            unicode: true,
            color: ColorMode::Off,
            ..PlotOptions::default()
        };
        let series = vec![PlotSeries {
            points: vec![(1.0, 1.0)],
            breaks_after: Vec::new(),
            symbol: None,
            mode: Some(PlotMode::Scatter),
            label: None,
        }];

        let rendered = render_ascii_plot(&series, &opts, StyleColorMode::Never);

        assert!(rendered.contains('┼'));
        assert!(rendered.contains('·'));
    }

    #[test]
    fn area_overlap_mixes_primary_colors() {
        assert_eq!(
            mix_area_colors(Some(AnsiColor::Red), Some(AnsiColor::Blue)),
            Some(AnsiColor::Magenta)
        );
        assert_eq!(
            mix_area_colors(Some(AnsiColor::Magenta), Some(AnsiColor::Green)),
            Some(AnsiColor::White)
        );
        assert_eq!(
            mix_area_colors(Some(AnsiColor::BrightRed), Some(AnsiColor::Blue)),
            Some(AnsiColor::BrightMagenta)
        );
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
            vec![
                (-1.0, 0.0),
                (-0.5, 0.0),
                (0.0, 10.0),
                (0.5, 10.0),
                (1.0, 10.0)
            ]
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

        let rendered = render_ascii_plot(&series, &opts, StyleColorMode::Never);
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
            unicode: false,
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

        let rendered = render_ascii_plot(&series, &opts, StyleColorMode::Never);
        let lines: Vec<&str> = rendered.lines().collect();
        let x_labels = lines[opts.height];

        assert!(x_labels.contains('1'));
        assert!(x_labels.contains('2'));
        assert!(x_labels.contains('3'));
        assert!(lines.iter().any(|line| line.trim_start().starts_with("2 ")));
    }
}
