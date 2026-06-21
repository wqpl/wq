use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const SHOWTABLE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Print a one-row dict as a table",
    code: "showtable (`name:`ada;`age:37)",
    expectation: ExampleExpectation::NoRun("writes a table to stdout"),
}];

const ASCIIPLOT_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Plot a sampled function",
        code: "asciiplot[sin;`xlim:(0;6.283);`size:(40;10);`samples:80;`grid:4;`ascii:T;`color:F;`symbols:\"*\";`caption:(\"Sine\";\"x\";\"y\");`labels:(\"sin\")]",
        expectation: ExampleExpectation::NoRun("writes a terminal plot to stdout"),
    },
    DocExample {
        title: "Plot columns from table-shaped data",
        code: "data:(`x:(0;1;2);`sin:(0;0.84;0.91);`cos:(1;0.54;-0.42)); asciiplot[data;`x:\"x\";`y:(\"sin\";\"cos\")]",
        expectation: ExampleExpectation::NoRun("writes a terminal plot to stdout"),
    },
];

pub(super) const SHOWTABLE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Showtable,
    summary: "Print table-shaped values as aligned text.",
    details: "`showtable[table]` accepts a dict of scalars, a list of dicts, a dict of lists, or a dict of dicts. Dict keys become column headers, dict-of-dicts outer keys become row labels, sparse rows are padded with empty cells, and the formatted table is written to stdout. The result value is unit.",
    examples: SHOWTABLE_EXAMPLES,
    related: &["asciiplot", "keys", "zip"],
};

pub(super) const ASCIIPLOT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Asciiplot,
    summary: "Print one or more numeric series as a terminal plot.",
    details: concat!(
        "`asciiplot[data+;opts]` writes a plot to stdout and returns unit. Data args can be ",
        "non-empty y-value lists, non-empty `((x;y);...)` point lists, callables, CAS ",
        "expressions, table-shaped dict-of-lists or list-of-dicts values, or series config dicts ",
        "like ``(`fn:sin;`xlim:(0;6.283);`label:\"sin\")``; ",
        "config dicts require `fn`, use their own `xlim` when sampling, and set ",
        "`symbol`, `mode`, and `label` for that series. Callables and CAS expressions ",
        "sample `samples` points over `xlim` (default -10..10, default sample count is the plot ",
        "width) and insert adaptive points across skipped gaps. Table-shaped inputs use row index ",
        "values for x when `x` is unset, use the named x column when `x` is set, plot all numeric ",
        "columns except the x column when `y` is unset, and plot only the named y column or columns ",
        "when `y` is set. Size comes from `size:(w;h)`, ",
        "`width`, `height`, or the current terminal when unset. Global options are `xlim`, ",
        "`ylim`, `x`, `y`, `symbols`, `labels`, `mode`, `axes`, `color`, `grid`, `samples`, `theme`, ",
        "`complex`, `ascii`, `title`, `xlabel`, `ylabel`, and `caption`. `mode` is `line`, ",
        "`scatter`, `step`, `bar`, or `area` (also `l`, `sc`, `st`, `b`, `a`). `axes` accepts ",
        "`T`, `F`, `\"full\"`, or `\"minimal\"`; `grid` accepts `T`, `F`, an integer density, ",
        "or `(x;y)` densities. `color` accepts `T`, `F`, a color name, or a list of color names; ",
        "known names include black, red, green, yellow, blue, magenta, cyan, white, gray/grey, ",
        "and bright_* variants. `theme` presets axes, grid, and color after named options are ",
        "read; implemented themes are `minimal`, `scientific`, and `dark`. `complex` is `re`, ",
        "`im`, `abs`, `arg`, or `plane`; `plane` plots complex outputs as `(real;imag)` points. ",
        "`caption:(title;xlabel;ylabel)` is a shortcut for the three label options."
    ),
    examples: ASCIIPLOT_EXAMPLES,
    related: &["showtable", "fmt", "numeric"],
};
