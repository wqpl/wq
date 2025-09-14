use std::cmp::{max, min};

use crate::{
    builtins::BuiltinEnum as BE,
    colored::{Color, Colorize},
    repl::stdio::stdout_println,
    value::{Value, WqResult},
    vm::Vm,
    wqerr::{WqErr, WqErrType},
};

pub fn asciiplot(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    if args.is_empty() {
        return Err(WqErr::new(WqErrType::Arity)
            .src(BE::Asciiplot)
            .msg("expected 1 or more args")
            .attach_note(BE::Asciiplot.usage()));
    }
    // Parse optional options dict at the end
    let mut data_args: Vec<&Value> = args.iter().collect();
    let mut opts = PlotOptions::default();
    if let Some(Value::Dict(_)) = data_args.last()
        && let Some(Value::Dict(map)) = data_args.pop().cloned()
    {
        opts.apply_from_map(&map);
    }
    // Collect series with raw_y available
    let mut parsed: Vec<ParsedSeries> = Vec::new();
    for arg in data_args {
        match arg {
            Value::IntList(arr) if !arr.is_empty() => {
                let y: Vec<f64> = arr.iter().map(|&v| v as f64).collect();
                let xy: Vec<(f64, f64)> =
                    y.iter().enumerate().map(|(i, &v)| (i as f64, v)).collect();
                parsed.push(ParsedSeries { xy });
            }
            Value::List(items)
                if items.iter().all(|it| {
                    if let Value::List(pair) = it {
                        pair.len() == 2 && pair[0].as_f64().is_some() && pair[1].as_f64().is_some()
                    } else {
                        false
                    }
                }) =>
            {
                let xy: Vec<(f64, f64)> = items
                    .iter()
                    .map(|it| {
                        let pair = if let Value::List(pair) = it {
                            pair
                        } else {
                            unreachable!()
                        };
                        (
                            pair[0].as_f64().unwrap() as f64,
                            pair[1].as_f64().unwrap() as f64,
                        )
                    })
                    .collect();
                parsed.push(ParsedSeries { xy });
            }
            Value::List(items)
                if items.iter().all(|v| v.as_f64().is_some()) && !items.is_empty() =>
            {
                let y: Vec<f64> = items.iter().map(|v| v.as_f64().unwrap()).collect();
                let xy: Vec<(f64, f64)> =
                    y.iter().enumerate().map(|(i, &v)| (i as f64, v)).collect();
                parsed.push(ParsedSeries { xy });
            }
            _ => {
                return Err(WqErr::new(WqErrType::Domain).
                    src(BE::Asciiplot).
                    msg("expected each arg to be (a list of numbers) or (a list of 2‑element numeric lists)").attach_note(
                        "e.g. (1;2;3), ((1;2);(2;4))"));
            }
        }
    }
    if parsed.is_empty() {
        return Err(WqErr::new(WqErrType::Domain).src(BE::Asciiplot).msg("expected each arg to be (a list of numbers) or (a list of 2‑element numeric lists)").attach_note(
                "e.g. (1;2;3), ((1;2);(2;4))"));
    }
    let all_series: Vec<Vec<(f64, f64)>> = parsed.iter().map(|p| p.xy.clone()).collect();
    let rendered = render_ascii_plot(&all_series, &opts);
    stdout_println(rendered);
    Ok(Value::unit())
}

#[derive(Clone)]
struct ParsedSeries {
    xy: Vec<(f64, f64)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlotMode {
    Line,
    Scatter,
    Step,
    Bar,
    Area,
}

#[derive(Clone)]
struct PlotOptions {
    width: usize,
    height: usize,
    xlim: Option<(f64, f64)>,
    ylim: Option<(f64, f64)>,
    symbols: Vec<char>,
    legend: bool,
    labels: Option<Vec<String>>,
    mode: PlotMode,
    axes: bool,
    color: bool,
    grid: bool,
    grid_x: usize,
    grid_y: usize,
    palette: Option<Vec<Color>>,
}

impl Default for PlotOptions {
    fn default() -> Self {
        Self {
            width: 80,
            height: 24,
            xlim: None,
            ylim: None,
            symbols: vec!['·'],
            legend: false,
            labels: None,
            mode: PlotMode::Line,
            axes: true,
            color: true,
            grid: false,
            grid_x: 0,
            grid_y: 0,
            palette: None,
        }
    }
}

impl PlotOptions {
    fn apply_from_map(&mut self, map: &indexmap::IndexMap<String, Value>) {
        if let Some(v) = map.get("width").and_then(|v| v.as_i64()) {
            self.width = max(10, v as usize);
        }
        if let Some(v) = map.get("height").and_then(|v| v.as_i64()) {
            self.height = max(5, v as usize);
        }
        if let Some(Value::List(items)) = map.get("xlim")
            && items.len() == 2
            && let (Some(a), Some(b)) = (items[0].as_f64(), items[1].as_f64())
        {
            self.xlim = Some((a, b));
        }
        if let Some(Value::List(items)) = map.get("ylim")
            && items.len() == 2
            && let (Some(a), Some(b)) = (items[0].as_f64(), items[1].as_f64())
        {
            self.ylim = Some((a, b));
        }
        if let Some(Value::List(items)) = map.get("symbols") {
            // eprintln!("DBG symbols: list len = {}", items.len());
            let mut syms = Vec::new();
            for it in items {
                match it {
                    Value::Char(c) => {
                        // eprintln!("DBG symbol item: char '{}" , c);
                        syms.push(*c)
                    }
                    _ => {
                        if let Ok(s) = it.try_to_string() {
                            // eprintln!("DBG symbol item: string '{}', first={:?}", s, s.chars().next());
                            if let Some(c) = s.chars().next() {
                                syms.push(c);
                            }
                        }
                    }
                }
            }
            if !syms.is_empty() {
                self.symbols = syms;
            }
        } else if let Some(val) = map.get("symbols") {
            // Accept a single string/char
            if let Ok(s) = val.try_to_string() {
                if let Some(c) = s.chars().next() {
                    self.symbols = vec![c];
                }
            } else if let Value::Char(c) = val {
                self.symbols = vec![*c];
            }
        }
        if let Some(Value::Bool(b)) = map.get("legend") {
            self.legend = *b;
        }
        if let Some(Value::Bool(b)) = map.get("axes") {
            self.axes = *b;
        }
        if let Some(v) = map.get("mode") {
            let mut s = None;
            if let Value::Symbol(sym) = v {
                s = Some(sym.as_str());
            } else if let Ok(ss) = v.try_to_string() {
                s = Some(Box::leak(ss.into_boxed_str()));
            }
            if let Some(m) = s {
                self.mode = match m {
                    "line" => PlotMode::Line,
                    "scatter" | "points" | "dots" => PlotMode::Scatter,
                    "step" | "stairs" => PlotMode::Step,
                    "bar" | "bars" | "column" => PlotMode::Bar,
                    "area" | "fill" => PlotMode::Area,
                    _ => self.mode,
                };
            }
        }
        if let Some(Value::Bool(b)) = map.get("color") {
            self.color = *b;
        }
        if let Some(Value::Bool(b)) = map.get("grid") {
            self.grid = *b;
        }
        if let Some(v) = map.get("grid_x").and_then(|v| v.as_i64()) {
            self.grid_x = max(0, v as usize);
        }
        if let Some(v) = map.get("grid_y").and_then(|v| v.as_i64()) {
            self.grid_y = max(0, v as usize);
        }
        if let Some(Value::List(items)) = map.get("labels") {
            let mut labs = Vec::new();
            for it in items {
                if let Ok(s) = it.try_to_string() {
                    labs.push(s);
                }
            }
            if !labs.is_empty() {
                self.labels = Some(labs);
            }
        }
        if let Some(p) = map.get("palette") {
            self.palette = parse_palette(p);
        }
    }
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
        0usize
    }; // dummy
    let x0_col = if x0_in {
        let t = (0.0 - xmin) / xspan;
        let col = (t * (width as f64 - 1.0)).round() as isize;
        min(width as isize - 1, max(0, col)) as usize
    } else {
        0usize
    };
    // optional gridlines
    if opts.grid {
        // target counts (same fields you already have)
        let gx = if opts.grid_x > 0 { opts.grid_x } else { 4 };
        let gy = if opts.grid_y > 0 { opts.grid_y } else { 4 };
        // choose tick positions in data space (nice 1–2–5 progression)
        let xticks = nice_ticks(xmin, xmax, gx);
        let yticks = nice_ticks(ymin, ymax, gy);
        // visuals: light/dim color, dashed pattern, neat glyphs
        let grid_color = Some(Color::BrightBlack); // respects opts.color toggle below
        let ch_h = '┈'; // ASCII fallback: '.'
        let ch_v = '┊'; // ASCII fallback: ':'
        // horizontals at yticks
        for yv in yticks {
            let t = (yv - ymin) / yspan;
            let row = (height as f64 - 1.0 - t * (height as f64 - 1.0)).round() as isize;
            let r = std::cmp::min(height as isize - 1, std::cmp::max(0, row)) as usize;
            for x in 0..width {
                // dashed
                if x % 2 == 0 {
                    set_cell_layer(
                        &mut grid,
                        x as isize,
                        r as isize,
                        ch_h,
                        Layer::Grid,
                        grid_color,
                        opts.color, // if color=false, it renders uncolored
                    );
                }
            }
        }
        // verticals at xticks
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
                        opts.color,
                    );
                }
            }
        }
    }
    if opts.axes {
        if y0_in {
            for x in 0..width {
                set_cell_layer(
                    &mut grid,
                    x as isize,
                    y0_row as isize,
                    '-',
                    Layer::Axis,
                    None,
                    opts.color,
                );
            }
        }
        if x0_in {
            for y in 0..height {
                set_cell_layer(
                    &mut grid,
                    x0_col as isize,
                    y as isize,
                    '|',
                    Layer::Axis,
                    None,
                    opts.color,
                );
            }
        }
        if x0_in && y0_in {
            set_cell_layer(
                &mut grid,
                x0_col as isize,
                y0_row as isize,
                '+',
                Layer::Axis,
                None,
                opts.color,
            );
        }
    }
    // draw series
    for (si, series) in series_list.iter().enumerate() {
        if series.is_empty() {
            continue;
        }
        let symbol = opts.symbols[si % opts.symbols.len()];
        let color: Option<Color> = if opts.color {
            if let Some(p) = &opts.palette {
                p.get(si % p.len())
                    .cloned()
                    .or_else(|| Some(series_color(si)))
            } else {
                Some(series_color(si))
            }
        } else {
            None
        };
        // eprintln!("asciiplot: using symbol '{}' for mode {:?}", symbol, opts.mode as u8);
        // map all points first
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
                    set_cell_layer(&mut grid, x, y, symbol, Layer::Data, color, opts.color);
                }
            }
            PlotMode::Scatter => {
                for (x, y) in pts {
                    set_cell_layer(&mut grid, x, y, symbol, Layer::Data, color, opts.color);
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
                    set_cell_layer(&mut grid, x, y, symbol, Layer::Data, color, opts.color);
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
                                opts.color,
                            );
                        }
                    }
                }
                if pts.len() == 1 {
                    let (x, y) = pts[0];
                    let ystart = min(baseline_row, y);
                    let yend = max(baseline_row, y);
                    for yy in ystart..=yend {
                        set_cell_layer(&mut grid, x, yy, symbol, Layer::Data, color, opts.color);
                    }
                }
            }
        }
    }
    // Convert grid to string, with y labels at the end of first and last rows
    let mut out = String::new();
    for (i, row) in grid.iter().enumerate() {
        let mut line = String::new();
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
    // Legend (optional)
    if opts.legend || opts.labels.is_some() {
        let labels = opts.labels.clone().unwrap_or_else(|| {
            (0..series_list.len())
                .map(|i| format!("series {}", i + 1))
                .collect()
        });
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
    // Use a compact formatting with up to 4 significant digits after decimal when needed
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
            for it in items {
                if let Ok(s) = it.try_to_string()
                    && let Some(c) = name_to_color(&s)
                {
                    out.push(c);
                }
            }
            if out.is_empty() { None } else { Some(out) }
        }
        // Single value: "red"
        _ => {
            if let Ok(s) = val.try_to_string() {
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
