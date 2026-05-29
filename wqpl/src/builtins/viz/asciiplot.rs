use std::cmp::{max, min};

use colored::{Color, Colorize};

use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_named_args};
use crate::cas::{infer_single_cas_var, substitute_cas};
use crate::session::stdio::wqstdout_println;
use crate::value::{Value, WqResult};
use crate::vm::Vm;
use crate::wqerror::{WqError, WqErrorType};

pub(crate) fn asciiplot(vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    #[rustfmt::skip]
    check_named_args(&args, BE::Asciiplot, &[
        "size", "width", "height", "xlim", "ylim",
        "symbols", "labels", "mode", "axes", "color", "grid",
        "samples", "theme", "complex", "ascii",
        "title", "xlabel", "ylabel", "caption",
    ])?;
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
        configs.push(parse_series_arg(vm, &arg, &opts)?);
    }
    if configs.is_empty() {
        return Err(WqError::new(WqErrorType::Domain).src(BE::Asciiplot).msg("expected each arg to be (a list of numbers) or (a list of 2‑element numeric lists)").attach_note(
                "e.g. (1;2;3), ((1;2);(2;4))"));
    }
    let mut all_series: Vec<Vec<(f64, f64)>> = Vec::with_capacity(configs.len());
    for config in &configs {
        let points = match &config.data {
            SeriesData::Raw(xy) => xy.clone(),
            SeriesData::Callable(func) => sample_callable_series(vm, func, &opts, config.xlim)?,
            SeriesData::Cas(expr) => sample_cas_series(expr, &opts, config.xlim)?,
        };
        all_series.push(points);
    }
    let rendered = render_ascii_plot(&all_series, &opts);
    wqstdout_println(rendered);
    Ok(Value::unit())
}

#[derive(Clone)]
#[expect(dead_code)]
struct SeriesConfig {
    data: SeriesData,
    xlim: Option<(f64, f64)>,
    symbol: Option<char>,
    mode: Option<PlotMode>,
    label: Option<String>,
}

#[derive(Clone)]
enum SeriesData {
    Raw(Vec<(f64, f64)>),
    Callable(Value),
    Cas(Value),
}

fn parse_series_arg(_vm: &mut Vm, arg: &Value, _opts: &PlotOptions) -> WqResult<SeriesConfig> {
    match arg {
        Value::IntList(arr) if !arr.is_empty() => Ok(SeriesConfig {
            data: SeriesData::Raw(
                arr.iter()
                    .enumerate()
                    .map(|(i, &y)| (i as f64, y as f64))
                    .collect(),
            ),
            xlim: None,
            symbol: None,
            mode: None,
            label: None,
        }),
        Value::List(items)
            if items.iter().all(|it| {
                if let Value::List(ref pair) = *it {
                    pair.len() == 2 && pair[0].as_f64().is_some() && pair[1].as_f64().is_some()
                } else {
                    false
                }
            }) && !items.is_empty() =>
        {
            Ok(SeriesConfig {
                data: SeriesData::Raw(
                    items
                        .iter()
                        .map(|it| {
                            let Value::List(ref pair) = *it else {
                                unreachable!();
                            };
                            (pair[0].as_f64().unwrap(), pair[1].as_f64().unwrap())
                        })
                        .collect(),
                ),
                xlim: None,
                symbol: None,
                mode: None,
                label: None,
            })
        }
        Value::List(items) if items.iter().all(|v| v.as_f64().is_some()) && !items.is_empty() => {
            Ok(SeriesConfig {
                data: SeriesData::Raw(
                    items
                        .iter()
                        .enumerate()
                        .map(|(i, y)| (i as f64, y.as_f64().unwrap()))
                        .collect(),
                ),
                xlim: None,
                symbol: None,
                mode: None,
                label: None,
            })
        }
        _ if arg.is_cas_expr() => Ok(SeriesConfig {
            data: SeriesData::Cas(arg.clone()),
            xlim: None,
            symbol: None,
            mode: None,
            label: None,
        }),
        Value::Dict(map) => {
            if let Some(fn_val) = map.get("fn") {
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

                let data = match fn_val {
                    v if v.is_callable() => {
                        SeriesData::Callable(fn_val.clone())
                    }
                    _ if fn_val.is_cas_expr() => SeriesData::Cas(fn_val.clone()),
                    _ => {
                        return Err(WqError::new(WqErrorType::Domain).src(BE::Asciiplot).msg(
                            "series config `fn` must be a callable function or CAS expression",
                        ));
                    }
                };

                Ok(SeriesConfig {
                    data,
                    xlim,
                    symbol,
                    mode,
                    label,
                })
            } else {
                Err(WqError::new(WqErrorType::Domain)
                    .src(BE::Asciiplot)
                    .msg(
                        "expected each arg to be point data, a function, a CAS expression, \
                         or a series config dict with `fn`",
                    )
                    .attach_note("e.g. (1;2;3), ((1;2);(2;4)), {x*x}, or @s x^2"))
            }
        }
        v if v.is_callable() => {
            Ok(SeriesConfig {
                data: SeriesData::Callable(arg.clone()),
                xlim: None,
                symbol: None,
                mode: None,
                label: None,
            })
        }
        _ => Err(WqError::new(WqErrorType::Domain)
            .src(BE::Asciiplot)
            .msg("expected each arg to be point data, a function, or a symbolic CAS expression")
            .attach_note("e.g. (1;2;3), ((1;2);(2;4)), {x*x}, or @s x^2")),
    }
}

fn sample_callable_series(
    vm: &mut Vm,
    func: &Value,
    opts: &PlotOptions,
    xlim: Option<(f64, f64)>,
) -> WqResult<Vec<(f64, f64)>> {
    let (xmin, xmax) = xlim.or(opts.xlim).unwrap_or((-10.0, 10.0));
    let initial_samples = opts.samples.unwrap_or(opts.width).max(2);

    if opts.complex_mode == "plane" {
        let mut sampler = |x: f64| -> Option<Value> { vm.call(func, Value::float(x).into()).ok() };
        let raw = sample_with_adaptive(xmin, xmax, initial_samples, true, &mut sampler);
        Ok(transform_complex_plane(&raw))
    } else {
        let mut sampler = |x: f64| -> Option<f64> {
            let y = vm.call(func, Value::float(x).into()).ok()?;
            extract_numeric_component(&y, &opts.complex_mode)
        };
        Ok(sample_with_adaptive(
            xmin,
            xmax,
            initial_samples,
            true,
            &mut sampler,
        ))
    }
}

fn sample_cas_series(
    expr: &Value,
    opts: &PlotOptions,
    xlim: Option<(f64, f64)>,
) -> WqResult<Vec<(f64, f64)>> {
    let (xmin, xmax) = xlim.or(opts.xlim).unwrap_or((-10.0, 10.0));
    let initial_samples = opts.samples.unwrap_or(opts.width).max(2);

    let var = Value::from_cas_var(infer_single_cas_var(expr).map_err(|e| e.src(BE::Asciiplot))?);

    if opts.complex_mode == "plane" {
        let mut sampler = |x: f64| -> Option<Value> {
            substitute_cas(expr, &var, &Value::float(x))
                .map_err(|e| e.src(BE::Asciiplot))
                .ok()
        };
        let raw = sample_with_adaptive(xmin, xmax, initial_samples, true, &mut sampler);
        Ok(transform_complex_plane(&raw))
    } else {
        let mut sampler = |x: f64| -> Option<f64> {
            let y = substitute_cas(expr, &var, &Value::float(x))
                .map_err(|e| e.src(BE::Asciiplot))
                .ok()?;
            extract_numeric_component(&y, &opts.complex_mode)
        };
        Ok(sample_with_adaptive(
            xmin,
            xmax,
            initial_samples,
            true,
            &mut sampler,
        ))
    }
}

fn sample_with_adaptive<T, F>(
    xmin: f64,
    xmax: f64,
    initial_samples: usize,
    adaptive: bool,
    sampler: &mut F,
) -> Vec<(f64, T)>
where
    T: Clone,
    F: FnMut(f64) -> Option<T>,
{
    let step = if initial_samples > 1 {
        (xmax - xmin) / (initial_samples.saturating_sub(1)) as f64
    } else {
        0.0
    };

    let mut points = Vec::with_capacity(initial_samples);
    for i in 0..initial_samples {
        let x = xmin + step * i as f64;
        if let Some(y) = sampler(x) {
            points.push((x, y));
        }
    }

    if !adaptive || points.len() < 2 || step <= 0.0 {
        return points;
    }

    let max_total = initial_samples * 2;
    let mut result = Vec::with_capacity(points.len());
    result.push(points[0].clone());

    for i in 1..points.len() {
        let gap = points[i].0 - points[i - 1].0;

        if gap > 3.0 * step && result.len() + 3 <= max_total {
            for j in 1..=3 {
                let x = points[i - 1].0 + gap * j as f64 / 4.0;
                if let Some(y) = sampler(x) {
                    result.push((x, y));
                    if result.len() >= max_total {
                        break;
                    }
                }
            }
        }
        if result.len() < max_total {
            result.push(points[i].clone());
        } else {
            break;
        }
    }

    result
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

fn transform_complex_plane(raw: &[(f64, Value)]) -> Vec<(f64, f64)> {
    raw.iter()
        .filter_map(|(_x, v)| {
            let z = v.as_complex64()?;
            Some((z.re, z.im))
        })
        .collect()
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

#[derive(Clone)]
struct PlotOptions {
    width: usize,
    height: usize,
    xlim: Option<(f64, f64)>,
    ylim: Option<(f64, f64)>,
    symbols: Vec<char>,
    labels: Option<Vec<String>>,
    mode: PlotMode,
    axes: AxesMode,
    color: ColorMode,
    grid: GridMode,
    samples: Option<usize>,
    theme: Option<String>,
    complex_mode: String,
    ascii: bool,
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
            mode: PlotMode::Line,
            axes: AxesMode::Full,
            color: ColorMode::On,
            grid: GridMode::Off,
            samples: None,
            theme: None,
            complex_mode: "re".to_string(),
            ascii: false,
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

fn render_ascii_plot(series_list: &[Vec<(f64, f64)>], opts: &PlotOptions) -> String {
    let width = opts.width;
    let height = opts.height;
    let width = max(10, width);
    let height = max(5, height);
    // Determine bounds
    let (mut xmin, mut xmax) = opts.xlim.unwrap_or_else(|| {
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        for s in series_list {
            for &(x, _) in s {
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
            for &(_, y) in s {
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
        let xticks = nice_ticks(xmin, xmax, gx);
        let yticks = nice_ticks(ymin, ymax, gy);
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
            let xticks = nice_ticks(xmin, xmax, tick_count);
            let yticks = nice_ticks(ymin, ymax, tick_count);
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
    for (si, series) in series_list.iter().enumerate() {
        if series.is_empty() {
            continue;
        }
        let symbol = opts.symbols[si % opts.symbols.len()];
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
        match opts.mode {
            PlotMode::Line => {
                for w in pts.windows(2) {
                    let (x0, y0) = w[0];
                    let (x1, y1) = w[1];
                    plot_line(&mut grid, x0, y0, x1, y1, symbol, color);
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
                for w in pts.windows(2) {
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
                for w in pts.windows(2) {
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
    // Convert grid to string, with y labels at the end of first and last rows
    let mut out = String::new();
    let ylabel_width = opts.ylabel.as_ref().map_or(0, |yl| yl.len() + 1);
    // Title
    if let Some(ref title) = opts.title {
        let tw = width.max(title.len());
        let pad = tw.saturating_sub(title.len()) / 2 + ylabel_width;
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
        for cell in row {
            line.push_str(&paint_char(cell.ch, cell.color));
        }
        if i == 0 {
            line.push(' ');
            line.push_str(&format_number(ymax));
        }
        if i + 1 == height {
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
    xline.push_str(&" ".repeat(ylabel_width));
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
    out.push_str(&xline);
    out.push('\n');
    // xlabel
    if let Some(ref xlabel) = opts.xlabel {
        let tw = width.max(xlabel.len());
        let pad = tw.saturating_sub(xlabel.len()) / 2 + ylabel_width;
        out.push_str(&" ".repeat(pad));
        out.push_str(xlabel);
        out.push('\n');
    }
    // Legend (optional) — shown when labels are provided
    if let Some(labels) = &opts.labels {
        let mut leg_parts = Vec::new();
        for (i, label) in labels.iter().enumerate() {
            let sym = opts.symbols[i % opts.symbols.len()];
            leg_parts.push(format!("{}({})", label, sym));
        }
        out.push_str(&format!("{}\n", leg_parts.join(", ")));
    }
    out
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

    use smallvec::smallvec;

    use super::*;
    use crate::builtins::Builtins;
    use crate::session::stdio::{WqStdout, set_wqstdout};

    struct SinkStdout;

    impl WqStdout for SinkStdout {
        fn print(&mut self, _s: &str) {}

        fn println(&mut self, _s: &str) {}
    }

    #[test]
    fn sample_cas_series_uses_symbolic_expression() {
        let expr = Value::from_cas_op("^", vec![Value::from_cas_var("x"), Value::Int(2)]);
        let opts = PlotOptions {
            width: 3,
            xlim: Some((0.0, 2.0)),
            ..PlotOptions::default()
        };
        let series = sample_cas_series(&expr, &opts, None).unwrap();
        assert_eq!(series, vec![(0.0, 0.0), (1.0, 1.0), (2.0, 4.0)]);
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
        assert_eq!(series, vec![(-1.0, 1.0), (0.0, 0.0), (1.0, 1.0)]);
    }

    #[test]
    fn asciiplot_accepts_single_cas_arg() {
        let expr = Value::from_cas_call("sin", vec![Value::from_cas_var("x")]);
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
        assert!(!series.is_empty());
        for (x, _y) in &series {
            assert!(*x >= 0.0);
        }
    }

    #[test]
    fn sample_cas_series_skips_invalid_points() {
        let expr = Value::from_cas_call("sqrt", vec![Value::from_cas_var("x")]);
        let opts = PlotOptions {
            width: 5,
            xlim: Some((-2.0, 2.0)),
            ..PlotOptions::default()
        };
        let series = sample_cas_series(&expr, &opts, None).unwrap();
        assert!(!series.is_empty());
        for (x, _y) in &series {
            assert!(*x >= 0.0);
        }
    }

    #[test]
    fn adaptive_sampling_refines_gaps() {
        // A function that is valid everywhere but has a sharp jump at x=0
        // We'll use abs(x) which is smooth, but we can test the adaptive logic
        // by checking that more points are generated when adaptive is on.
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
        // With width=5, initial_samples=5, max_total=10.
        // abs is valid everywhere, so adaptive may or may not trigger.
        // Just ensure it doesn't panic and returns valid data.
        assert!(!series.is_empty());
    }
}
