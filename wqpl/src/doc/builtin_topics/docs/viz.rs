use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const SHOWTABLE_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Print a one-row dict as a table",
        code: "showtable (`name:`ada;`age:37)",
        expectation: ExampleExpectation::NoRun("writes a table to stdout"),
    },
    DocExample {
        title: "Select columns, limit rows, and write Markdown",
        code: "grades:(`name:(\"ada\";\"grace\";\"katherine\");`score:(97;99;98);`note:(\"compiler\";\"navy\";\"orbit\")); showtable[grades;`cols:(\"name\";\"score\");`limit:2;`style:\"markdown\"]",
        expectation: ExampleExpectation::NoRun("writes a Markdown table to stdout"),
    },
];

const ASCIIPLOT_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Plot a sampled function",
        code: "asciiplot[sin;`xlim:(0;6.283);`size:(40;10);`samples:80;`grid:4;`ticklabels:T;`ascii:T;`color:F;`symbols:\"*\";`caption:(\"Sine\";\"x\";\"y\");`labels:(\"sin\")]",
        expectation: ExampleExpectation::NoRun("writes a terminal plot to stdout"),
    },
    DocExample {
        title: "Plot columns from table-shaped data",
        code: "data:(`x:(0;1;2);`sin:(0;0.84;0.91);`cos:(1;0.54;-0.42)); asciiplot[data;`x:\"x\";`y:(\"sin\";\"cos\")]",
        expectation: ExampleExpectation::NoRun("writes a terminal plot to stdout"),
    },
    DocExample {
        title: "Mix per-series plot modes",
        code: "asciiplot[(`data:(1;3;2);`mode:\"bar\";`label:\"count\");(`data:sin;`mode:\"line\";`xlim:(0;3);`label:\"sin\");`size:(40;10);`color:T]",
        expectation: ExampleExpectation::NoRun("writes a terminal plot to stdout"),
    },
];

pub(super) const SHOWTABLE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Showtable,
    summary: "Print table-shaped values as aligned text.",
    details:
"`showtable[table;opts]` accepts a dict of atom values, a list of dicts, a dict of lists, or a dict of dicts.
Dict keys become column headers, dict-of-dicts outer keys become the `row` column, sparse rows are padded with empty cells, and the formatted table is written to stdout.

Strings render as text cells; other lists in dict-of-lists inputs expand as columns.
Numeric columns are right-aligned using display width, so wide Unicode cells line up with narrow cells.

Options:

- `cols`: select named columns in the given order; errors when a selected column is absent.
- `limit`: print the first N rows and append an omitted-row footer when rows remain.
- `width`: truncate each cell to N display columns.
- `missing`: replace sparse empty cells.
- `style`: `\"plain\"`, `\"markdown\"`, or `\"md\"`.

The result value is unit.",
    examples: SHOWTABLE_EXAMPLES,
    related: &["asciiplot", "keys", "zip"],
};

pub(super) const ASCIIPLOT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Asciiplot,
    summary: "Print one or more numeric series as a terminal plot.",
    details:
"`asciiplot[data+;opts]` writes a plot to stdout and returns unit.

Data args:

- Y-value lists: non-empty `(y0;y1;...)` values use row index as x.
- Point lists: non-empty `((x;y);...)` values plot explicit coordinates.
- Callables and CAS expressions: sampled over `xlim` (default -10..10) using `samples` points, or the plot width when `samples` is unset; skipped points and sharp discontinuities split line segments.
- Table-shaped data: dict-of-lists or list-of-dicts; `x` selects the x column, otherwise row index is used; `y` selects y columns, otherwise all numeric columns except x are plotted.
- Series config dicts: values like ``(`data:sin;`xlim:(0;6.283);`label:\"sin\")`` or ``(`data:(1;3;2);`mode:\"bar\")``. Use `data` as the unified source key for callables, CAS, y-value lists, explicit `((x;y);...)` points, or table-shaped values. Paired numeric `x` and `y` lists are also accepted when the dict has a config option such as `mode` or `label`. `fn`, `cas`, `expr`, `values`, and `points` remain aliases for older or more explicit snippets. Config dicts accept per-series `xlim`, `symbol`, `mode`, and `label`.

Global options:

- Size: `size:(w;h)`, `width`, `height`, or the current terminal when unset.
- Bounds and sampling: `xlim`, `ylim`, `samples`, and `complex`.
- Table columns: `x` and `y`.
- Styling: `symbols`, `labels`, `mode`, `axes`, `color`, `grid`, `theme`, `ascii`, `ticklabels`, `title`, `xlabel`, `ylabel`, and `caption`.

Option values:

- Keyword option values can be strings or tags, e.g. `` `mode:\"line\" `` or `` `mode:`line ``.
- `mode`: `line`, `scatter`, `step`, `bar`, or `area` (also `l`, `sc`, `st`, `b`, `a`). `line` connects samples and is best for continuous lists, callables, and CAS. `scatter` marks only samples and works well for point clouds, noisy table columns, and `complex:\"plane\"`. `step` draws horizontal-then-vertical segments for piecewise or sample-and-hold data; for callables/CAS, `samples` controls the stair count. `bar` draws vertical bars from zero when visible, otherwise from the bottom edge; it is clearest with discrete y-lists, tables, or low `samples`. `area` fills between the curve and baseline; overlapping area fills are marked and their ANSI colors are mixed automatically.
- `axes`: `T`, `F`, `full`, `minimal`, `off`, or `none`.
- `grid`: `T`, `F`, an integer density, or `(x;y)` densities.
- `ticklabels:T`: add numeric labels for interior tick positions.
- `color`: `T`, `F`, a color name, or a list of color names; known names include black, red, green, yellow, blue, magenta, cyan, white, gray/grey, and bright_* variants.
- `theme`: apply a preset before other named options; `minimal` sets axes/grid off with color on, and `maximal` sets full axes, grid on, and color on. Later `axes`, `grid`, or `color` options override the preset.
- `complex`: `re`/`real`, `im`/`imag`/`imaginary`, `abs`, `arg`, or `plane`; `plane` plots complex outputs as `(real;imag)` points.
- `caption:(title;xlabel;ylabel)`: shortcut for the three label options.",
    examples: ASCIIPLOT_EXAMPLES,
    related: &["showtable", "fmt", "numeric"],
};
