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
        code: "asciiplot[(`data:(1;3;2);`mode:\"bar\";`label:\"count\");(`fn:sin;`mode:\"line\";`xlim:(0;3);`label:\"sin\");`size:(40;10);`color:T]",
        expectation: ExampleExpectation::NoRun("writes a terminal plot to stdout"),
    },
];

pub(super) const SHOWTABLE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Showtable,
    summary: "Print table-shaped values as aligned text.",
    details: concat!(
        "`showtable[table;opts]` accepts a dict of scalars, a list of dicts, a dict of lists, ",
        "or a dict of dicts. Dict keys become column headers, dict-of-dicts outer keys become ",
        "the `row` column, sparse rows are padded with empty cells, and the formatted table is ",
        "written to stdout. Strings and char-only lists render as scalar cells; other lists in ",
        "dict-of-lists inputs expand as columns. Numeric columns are right-aligned using display ",
        "width, so wide Unicode cells line up with narrow cells. Options are `cols`, `limit`, ",
        "`width`, `style`, and `missing`. `cols` selects named columns in the given order and ",
        "errors when a selected column is absent. `limit` prints the first N rows and appends an ",
        "omitted-row footer when rows remain. `width` truncates each cell to N display columns. ",
        "`missing` replaces sparse empty cells. `style` is `\"plain\"`, `\"markdown\"`, or `\"md\"`. ",
        "The result value is unit."
    ),
    examples: SHOWTABLE_EXAMPLES,
    related: &["asciiplot", "keys", "zip"],
};

pub(super) const ASCIIPLOT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Asciiplot,
    summary: "Print one or more numeric series as a terminal plot.",
    details: concat!(
        "`asciiplot[data+;opts]` writes a plot to stdout and returns unit.\n\n",
        "Data args:\n",
        "- Y-value lists: non-empty `(y0;y1;...)` values use row index as x.\n",
        "- Point lists: non-empty `((x;y);...)` values plot explicit coordinates.\n",
        "- Callables and CAS expressions: sampled over `xlim` (default -10..10) using ",
        "`samples` points, or the plot width when `samples` is unset; skipped points and sharp ",
        "discontinuities split line segments.\n",
        "- Table-shaped data: dict-of-lists or list-of-dicts; `x` selects the x column, otherwise ",
        "row index is used; `y` selects y columns, otherwise all numeric columns except x are plotted.\n",
        "- Series config dicts: values like ``(`fn:sin;`xlim:(0;6.283);`label:\"sin\")`` ",
        "or ``(`data:(1;3;2);`mode:\"bar\")``. Use `fn` for callables or CAS, `cas`/`expr` ",
        "for CAS-only spelling, `data`/`values` for y-value lists, `points` for explicit ",
        "`((x;y);...)` points, or paired numeric `x` and `y` lists when the dict also has a ",
        "config option such as `mode` or `label`. Config dicts accept per-series `xlim`, ",
        "`symbol`, `mode`, and `label`.\n\n",
        "Global options:\n",
        "- Size: `size:(w;h)`, `width`, `height`, or the current terminal when unset.\n",
        "- Bounds and sampling: `xlim`, `ylim`, `samples`, and `complex`.\n",
        "- Table columns: `x` and `y`.\n",
        "- Styling: `symbols`, `labels`, `mode`, `axes`, `color`, `grid`, `theme`, `ascii`, ",
        "`ticklabels`, `title`, `xlabel`, `ylabel`, and `caption`.\n\n",
        "Option values:\n",
        "- `mode`: `line`, `scatter`, `step`, `bar`, or `area` (also `l`, `sc`, `st`, `b`, `a`).\n",
        "  `line` connects samples and is best for continuous lists, callables, and CAS. ",
        "`scatter` marks only samples and works well for point clouds, noisy table columns, and ",
        "`complex:\"plane\"`. `step` draws horizontal-then-vertical segments for piecewise or ",
        "sample-and-hold data; for callables/CAS, `samples` controls the stair count. `bar` draws ",
        "vertical bars from zero when visible, otherwise from the bottom edge; it is clearest with ",
        "discrete y-lists, tables, or low `samples`. `area` fills between the curve and baseline; ",
        "overlapping area fills are marked and their ANSI colors are mixed automatically.\n",
        "- `axes`: `T`, `F`, `\"full\"`, or `\"minimal\"`.\n",
        "- `grid`: `T`, `F`, an integer density, or `(x;y)` densities.\n",
        "- `ticklabels:T`: add numeric labels for interior tick positions.\n",
        "- `color`: `T`, `F`, a color name, or a list of color names; known names include black, ",
        "red, green, yellow, blue, magenta, cyan, white, gray/grey, and bright_* variants.\n",
        "- `theme`: apply a preset before other named options; `minimal` sets axes/grid off with ",
        "color on, and `maximal` sets full axes, grid on, and color on. Later `axes`, `grid`, ",
        "or `color` options override the preset.\n",
        "- `complex`: `re`, `im`, `abs`, `arg`, or `plane`; `plane` plots complex outputs as ",
        "`(real;imag)` points.\n",
        "- `caption:(title;xlabel;ylabel)`: shortcut for the three label options."
    ),
    examples: ASCIIPLOT_EXAMPLES,
    related: &["showtable", "fmt", "numeric"],
};
