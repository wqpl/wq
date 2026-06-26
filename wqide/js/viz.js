import {
  WasmWqSession,
  set_stdout_callback,
  set_stderr_callback,
  highlight_wq,
} from "wq-wasm";
import { createOutputRenderer } from "./ansi.js";
import { createWqEditor } from "./editor.js";
import { named, plotSeriesArg, wqString } from "./viz-codegen.js";
import { alignTurnBody, ensureWasm, escapeHtml, queueEval } from "./wq-shared.js";

const instances = new WeakMap();

const SOURCE_KIND_LABELS = {
  plot: "mixed series",
  table: "table",
};

const DEFAULT_STATE = {
  title: "Function plot",
  subtitle: "asciiplot / function source",
  sourceKind: "plot",
  sourceExpr: "",
  layout: "below",
  mode: "line",
  complex: "re",
  theme: "none",
  axes: "full",
  grid: "4",
  palette: "classic",
  width: 90,
  widthAuto: true,
  height: 24,
  samples: 140,
  xlimMinText: "0",
  xlimMaxText: "6.283",
  ylimMinText: "",
  ylimMaxText: "",
  xlimLocked: false,
  ylimLocked: false,
  tableXText: "",
  tableYText: "",
  titleText: "sin and cos",
  xlabelText: "x",
  ylabelText: "y",
  labels: true,
  seriesOptions: true,
  ascii: false,
  autoRun: true,
  series: [
    { expr: "sin", label: "sin", symbol: "s", mode: "line" },
    { expr: "cos", label: "cos", symbol: "c", mode: "line" },
  ],
  tableShape: "list",
  rows: 5,
  tableColsText: "",
  tableLimitText: "",
  tableWidthText: "",
  tableStyle: "plain",
  tableMissingText: "",
};

const PRESETS = {
  trig: {
    title: "Function plot",
    subtitle: "asciiplot / function source",
    sourceKind: "plot",
    mode: "line",
    complex: "re",
    theme: "none",
    axes: "full",
    grid: "4",
    palette: "classic",
    width: 90,
    height: 24,
    samples: 140,
    xlimMinText: "0",
    xlimMaxText: "6.283",
    ylimMinText: "",
    ylimMaxText: "",
    titleText: "sin and cos",
    xlabelText: "x",
    ylabelText: "y",
    series: [
      { expr: "sin", label: "sin", symbol: "s", mode: "line" },
      { expr: "cos", label: "cos", symbol: "c", mode: "line" },
    ],
  },
  data: {
    title: "Data series",
    subtitle: "asciiplot / raw values",
    sourceKind: "plot",
    mode: "bar",
    complex: "re",
    theme: "none",
    axes: "minimal",
    grid: "off",
    palette: "ink",
    width: 86,
    height: 20,
    samples: 80,
    xlimMinText: "",
    xlimMaxText: "",
    ylimMinText: "",
    ylimMaxText: "",
    titleText: "raw values",
    xlabelText: "index",
    ylabelText: "value",
    series: [
      { expr: "(3;7;4;8;5;9;6;11)", label: "north", symbol: "#", mode: "bar" },
      { expr: "(2;5;7;4;10;6;12;8)", label: "south", symbol: "+", mode: "bar" },
    ],
    seriesOptions: false,
  },
  tablePlot: {
    title: "Table plot",
    subtitle: "asciiplot / table columns",
    sourceKind: "plot",
    mode: "line",
    complex: "re",
    theme: "none",
    axes: "full",
    grid: "4",
    palette: "bright",
    width: 90,
    height: 22,
    samples: 80,
    xlimMinText: "0",
    xlimMaxText: "5",
    ylimMinText: "",
    ylimMaxText: "",
    tableXText: "x",
    tableYText: "sin;cos",
    titleText: "table columns",
    xlabelText: "x",
    ylabelText: "value",
    series: [
      {
        expr: "(`x:(0;1;2;3;4;5);`sin:(0;0.84;0.91;0.14;-0.76;-0.96);`cos:(1;0.54;-0.42;-0.99;-0.65;0.28))",
        label: "",
        symbol: "",
        mode: "",
      },
    ],
    seriesOptions: false,
  },
  cas: {
    title: "CAS curve",
    subtitle: "asciiplot / symbolic source",
    sourceKind: "plot",
    mode: "line",
    complex: "re",
    theme: "none",
    axes: "full",
    grid: "4",
    palette: "bright",
    width: 92,
    height: 24,
    samples: 170,
    xlimMinText: "-4",
    xlimMaxText: "4",
    ylimMinText: "",
    ylimMaxText: "",
    titleText: "symbolic curves",
    xlabelText: "x",
    ylabelText: "y",
    series: [
      { expr: "@s x^2-2*x", label: "quadratic", symbol: "q", mode: "line" },
      { expr: "@s 1/(x^2+1)", label: "inverse", symbol: "i", mode: "scatter" },
    ],
  },
  modes: {
    title: "Mode mixer",
    subtitle: "asciiplot / per-series options",
    sourceKind: "plot",
    mode: "line",
    complex: "re",
    theme: "none",
    axes: "full",
    grid: "4",
    palette: "bright",
    width: 92,
    height: 24,
    samples: 160,
    xlimMinText: "0",
    xlimMaxText: "6.283",
    ylimMinText: "",
    ylimMaxText: "",
    titleText: "mode mixer",
    xlabelText: "x",
    ylabelText: "y",
    series: [
      { expr: "sin", label: "line", symbol: "l", mode: "line" },
      { expr: "cos", label: "scatter", symbol: "s", mode: "scatter" },
    ],
  },
  bars: {
    title: "Bars",
    subtitle: "asciiplot / value list",
    sourceKind: "plot",
    mode: "bar",
    complex: "re",
    theme: "none",
    axes: "minimal",
    grid: "off",
    palette: "ink",
    width: 78,
    height: 18,
    samples: 80,
    xlimMinText: "",
    xlimMaxText: "",
    ylimMinText: "",
    ylimMaxText: "",
    titleText: "bar values",
    xlabelText: "index",
    ylabelText: "value",
    series: [{ expr: "(3;7;4;8;5;9;6;11)", label: "score", symbol: "#", mode: "bar" }],
    seriesOptions: false,
  },
  complex: {
    title: "Complex plane",
    subtitle: "asciiplot / complex projection",
    sourceKind: "plot",
    mode: "scatter",
    complex: "plane",
    theme: "none",
    axes: "minimal",
    grid: "4",
    palette: "classic",
    width: 90,
    height: 24,
    samples: 180,
    xlimMinText: "-8",
    xlimMaxText: "8",
    ylimMinText: "",
    ylimMaxText: "",
    titleText: "sqrt complex plane",
    xlabelText: "real",
    ylabelText: "imag",
    series: [{ expr: "sqrt", label: "sqrt", symbol: "*", mode: "scatter" }],
    seriesOptions: false,
  },
  table: {
    title: "Show table",
    subtitle: "showtable / table source",
    sourceKind: "table",
    sourceExpr: "",
    tableShape: "text",
    rows: 5,
    tableColsText: "species;tissue;observation;markers",
    tableLimitText: "",
    tableWidthText: "",
    tableStyle: "plain",
    tableMissingText: "",
  },
  tableMap: {
    title: "Math map",
    subtitle: "showtable / dict of dicts",
    sourceKind: "table",
    sourceExpr: "",
    tableShape: "matrix",
    rows: 5,
    tableColsText: "row;kind;dim;invariant;value",
    tableLimitText: "",
    tableWidthText: "",
    tableStyle: "plain",
    tableMissingText: "",
  },
};

const PALETTES = {
  classic: ["red", "blue", "green", "magenta"],
  bright: ["bright_red", "bright_blue", "bright_green", "bright_magenta"],
  ink: ["cyan", "yellow", "white", "green"],
};

const THEME_PRESETS = {
  minimal: {
    axes: "off",
    grid: "off",
    palette: "classic",
  },
  maximal: {
    axes: "full",
    grid: "4",
    palette: "classic",
  },
};

const PLOT_WIDTH_MIN = 40;
const PLOT_WIDTH_MAX = 120;
const PLOT_WIDTH_LABEL_GUTTER = 10;

const SERIES_MODE_OPTIONS = [
  ["", "plot"],
  ["line", "line"],
  ["scatter", "scatter"],
  ["step", "step"],
  ["bar", "bar"],
  ["area", "area"],
];

const TABLE_SHAPE_DEFAULTS = {
  text: {
    tableColsText: "species;tissue;observation;markers",
    tableLimitText: "",
    tableWidthText: "",
    tableStyle: "plain",
    tableMissingText: "",
  },
  list: {
    tableColsText: "system;observable;unit;value",
    tableLimitText: "",
    tableWidthText: "",
    tableStyle: "plain",
    tableMissingText: "",
  },
  dict: {
    tableColsText: "element;symbol;z;mass",
    tableLimitText: "",
    tableWidthText: "",
    tableStyle: "plain",
    tableMissingText: "",
  },
  matrix: {
    tableColsText: "row;kind;dim;invariant;value",
    tableLimitText: "",
    tableWidthText: "",
    tableStyle: "plain",
    tableMissingText: "",
  },
};

const LIMIT_INPUTS = {
  xlimMinText: {
    partnerKey: "xlimMaxText",
    lockKey: "xlimLocked",
  },
  xlimMaxText: {
    partnerKey: "xlimMinText",
    lockKey: "xlimLocked",
  },
  ylimMinText: {
    partnerKey: "ylimMaxText",
    lockKey: "ylimLocked",
  },
  ylimMaxText: {
    partnerKey: "ylimMinText",
    lockKey: "ylimLocked",
  },
};

function boolLit(value) {
  return value ? "T" : "F";
}

function wqTag(value) {
  return `\`${value}`;
}

function cloneSeries(series) {
  return (series || []).map((item) => ({
    expr: item.expr || "",
    label: item.label || "",
    symbol: item.symbol || "",
    mode: item.mode || "",
  }));
}

function splitLimitText(text) {
  const value = String(text || "").trim();
  if (!value) return ["", ""];
  const unwrapped = value.startsWith("(") && value.endsWith(")") ? value.slice(1, -1) : value;
  const range = unwrapped.match(/^(.+)\.\.(.+)$/);
  if (range) return [range[1].trim(), range[2].trim()];
  const parts = unwrapped.replace(",", ";").split(";");
  if (parts.length < 2) return [value, ""];
  return [parts[0].trim(), parts[1].trim()];
}

function hydrateLimitState(state) {
  if (state.xlimText !== undefined) {
    [state.xlimMinText, state.xlimMaxText] = splitLimitText(state.xlimText);
    delete state.xlimText;
  }
  if (state.ylimText !== undefined) {
    [state.ylimMinText, state.ylimMaxText] = splitLimitText(state.ylimText);
    delete state.ylimText;
  }
  return state;
}

function stateForPreset(key) {
  const preset = PRESETS[key] || PRESETS.trig;
  const state = hydrateLimitState({
    ...DEFAULT_STATE,
    ...preset,
    series: cloneSeries(preset.series || DEFAULT_STATE.series),
    preset: key,
  });
  if (state.sourceKind === "table" && !state.sourceExpr) {
    state.sourceExpr = buildTableValue(state.tableShape, state.rows);
  }
  return state;
}

function colorOption(state) {
  if (state.palette === "off") return named("color", "F");
  const colors = PALETTES[state.palette] || PALETTES.classic;
  return named("color", `(${colors.map(wqString).join(";")})`);
}

function clampPlotWidth(value) {
  const number = Math.round(Number(value));
  if (!Number.isFinite(number)) return DEFAULT_STATE.width;
  return Math.max(PLOT_WIDTH_MIN, Math.min(PLOT_WIDTH_MAX, number));
}

function effectivePlotWidth(state) {
  if (!state.widthAuto) return clampPlotWidth(state.width);
  return clampPlotWidth(state.computedWidth ?? state.width);
}

function textColumnCount(value) {
  return Array.from(String(value || "").trim()).length;
}

function plotWidthReserve(state) {
  const ylabelWidth = textColumnCount(state.ylabelText);
  return PLOT_WIDTH_LABEL_GUTTER + (ylabelWidth ? ylabelWidth + 1 : 0);
}

function gridOption(state) {
  return named("grid", state.grid === "off" ? "F" : state.grid);
}

function axesOption(state) {
  if (state.axes === "off") return named("axes", "F");
  return named("axes", wqString(state.axes));
}

function limitOption(name, minText, maxText) {
  const minValue = String(minText || "").trim();
  const maxValue = String(maxText || "").trim();
  return minValue && maxValue ? named(name, `(${minValue};${maxValue})`) : null;
}

function textOption(name, text) {
  const value = String(text || "").trim();
  return value ? named(name, wqString(value)) : null;
}

function numberTextOption(name, text) {
  const value = String(text || "").trim();
  return value ? named(name, value) : null;
}

function textListOption(name, text) {
  const values = String(text || "")
    .split(/[;,\n]+/)
    .map((value) => value.trim())
    .filter(Boolean);
  if (!values.length) return null;
  if (values.length === 1) return named(name, wqString(values[0]));
  return named(name, `(${values.map(wqString).join(";")})`);
}

function indentWqBlock(text) {
  return String(text)
    .split("\n")
    .map((line) => `  ${line}`)
    .join("\n");
}

function defaultSeriesForKind(kind) {
  if (kind === "data") {
    return [{ expr: "(3;7;4;8;5;9;6;11)", label: "score", symbol: "#", mode: "bar" }];
  }
  if (kind === "cas") {
    return [
      { expr: "@s x^2-2*x", label: "quadratic", symbol: "q", mode: "line" },
      { expr: "@s 1/(x^2+1)", label: "inverse", symbol: "i", mode: "scatter" },
    ];
  }
  return [
    { expr: "sin", label: "sin", symbol: "s", mode: "line" },
    { expr: "cos", label: "cos", symbol: "c", mode: "line" },
  ];
}

function normalizedSeries(state) {
  const series = cloneSeries(state.series).filter((item) => item.expr.trim());
  return series.length ? series : defaultSeriesForKind("function");
}

function labelsOption(state, series) {
  if (!state.labels) return null;
  const labels = series.map((item) => item.label.trim()).filter(Boolean);
  return labels.length ? named("labels", `(${labels.map(wqString).join(";")})`) : null;
}

function symbolsOption(series) {
  const symbols = series.map((item) => item.symbol.trim()).filter(Boolean);
  if (!symbols.length) return null;
  if (symbols.length === 1) return named("symbols", wqString(symbols[0]));
  return named("symbols", `(${symbols.map(wqString).join(";")})`);
}

function plotOptions(state) {
  const args = [
    named("mode", wqString(state.mode)),
    named("size", `(${effectivePlotWidth(state)};${state.height})`),
    named("samples", state.samples),
  ];
  if (state.theme !== "none") {
    args.push(named("theme", wqString(state.theme)));
  }
  args.push(
    axesOption(state),
    gridOption(state),
    named("ascii", boolLit(state.ascii)),
    colorOption(state),
  );
  if (state.complex !== "re") {
    args.push(named("complex", wqString(state.complex)));
  }
  for (const option of [
    limitOption("xlim", state.xlimMinText, state.xlimMaxText),
    limitOption("ylim", state.ylimMinText, state.ylimMaxText),
    textOption("x", state.tableXText),
    textListOption("y", state.tableYText),
    textOption("title", state.titleText),
    textOption("xlabel", state.xlabelText),
    textOption("ylabel", state.ylabelText),
  ]) {
    if (option) args.push(option);
  }
  return args;
}

function plotCall(args) {
  return `asciiplot[\n${args.join(";\n")}\n]`;
}

function buildPlotCode(state) {
  const series = normalizedSeries(state);
  const args = series.map((item) => plotSeriesArg(item, state));
  const useGlobalSeriesOptions = !state.seriesOptions;
  const labels = useGlobalSeriesOptions ? labelsOption(state, series) : null;
  const symbols = useGlobalSeriesOptions ? symbolsOption(series) : null;
  if (labels) args.push(labels);
  if (symbols) args.push(symbols);
  args.push(...plotOptions(state));
  return plotCall(args);
}

const BIOLOGY_ROWS = [
  ["yeast", "bud", "cell cycle", ["cdc28", "cln3"]],
  ["arabidopsis", "leaf", "stomata", ["guard", "chlorophyll"]],
  ["zebrafish", "embryo", "somite wave", ["notch", "fgf"]],
  ["e coli", "culture", "log phase", ["lac", "oriC"]],
  ["drosophila", "wing", "pattern", ["hedgehog", "wingless"]],
  ["mouse", "hippocampus", "slice", ["ca1", "synapse"]],
  ["human", "fibroblast", "repair", ["brca1", "p53"]],
  ["xenopus", "oocyte", "gradient", ["bmp", "wnt"]],
];

const PHYSICS_ROWS = [
  ["pendulum", "period", "s", "2.01"],
  ["spring", "frequency", "Hz", "3.18"],
  ["capacitor", "charge", "uC", "47.2"],
  ["photon", "energy", "eV", "1.91"],
  ["orbit", "speed", "km/s", "7.67"],
  ["gas", "pressure", "kPa", "101.3"],
  ["coil", "field", "mT", "12.4"],
  ["slab", "flux", "W/m2", "1361"],
];

const CHEMISTRY_COLUMNS = [
  {
    key: "element",
    values: ["hydrogen", "oxygen", "carbon", "sodium", "chlorine", "iron", "copper", "calcium"].map(
      wqString,
    ),
  },
  {
    key: "symbol",
    values: ["H", "O", "C", "Na", "Cl", "Fe", "Cu", "Ca"].map(wqTag),
  },
  {
    key: "z",
    values: ["1", "8", "6", "11", "17", "26", "29", "20"],
  },
  {
    key: "mass",
    values: ["1.008", "15.999", "12.011", "22.990", "35.450", "55.845", "63.546", "40.078"],
  },
];

const MATH_ROWS = [
  ["identity", "matrix", "2x2", "det", "1"],
  ["rotation", "matrix", "2x2", "trace", "1.414"],
  ["prime", "number", "atom", "mod", "7"],
  ["fibonacci", "sequence", "n", "term", "55"],
  ["gaussian", "function", "R", "area", "1"],
  ["fourier", "basis", "N", "mode", "3"],
  ["euler", "formula", "C", "phase", "3.142"],
  ["simplex", "polytope", "3D", "faces", "4"],
];

function inlineWqList(items) {
  return `(${items.join(";")})`;
}

function tableDict(fields) {
  return `(${fields.map(([key, value]) => `${wqTag(key)}:${value}`).join(";")})`;
}

function formatListRows(rows) {
  if (!rows.length) return "()";
  return `(\n${rows.map((row) => `  ${row}`).join(";\n")}\n)`;
}

function formatDictEntries(entries) {
  if (!entries.length) return "()";
  return `(\n${entries.map((entry) => `  ${entry.key}:${entry.value}`).join(";\n")}\n)`;
}

function buildTextTableRow([species, tissue, observation, markers]) {
  return tableDict([
    ["species", wqString(species)],
    ["tissue", wqString(tissue)],
    ["observation", wqString(observation)],
    ["markers", inlineWqList(markers.map(wqString))],
  ]);
}

function buildPhysicsTableRow([system, observable, unit, value]) {
  return tableDict([
    ["system", wqString(system)],
    ["observable", wqString(observable)],
    ["unit", wqString(unit)],
    ["value", value],
  ]);
}

function buildMathTableEntry([key, kind, dim, invariant, value]) {
  return {
    key: wqTag(key),
    value: tableDict([
      ["kind", wqString(kind)],
      ["dim", wqString(dim)],
      ["invariant", wqString(invariant)],
      ["value", value],
    ]),
  };
}

function tableRowsForShape(shape) {
  if (shape === "text") return BIOLOGY_ROWS.map(buildTextTableRow);
  if (shape === "matrix") {
    return MATH_ROWS.map(buildMathTableEntry).map((entry) => `${entry.key}:${entry.value}`);
  }
  return PHYSICS_ROWS.map(buildPhysicsTableRow);
}

function buildListTableValue(rows) {
  return formatListRows(tableRowsForShape("list").slice(0, clampRows(rows)));
}

function buildDictTableValue(rows) {
  const rowCount = clampRows(rows);
  return formatDictEntries(
    CHEMISTRY_COLUMNS.map((column) => ({
      key: wqTag(column.key),
      value: inlineWqList(column.values.slice(0, rowCount)),
    })),
  );
}

function buildMatrixTableValue(rows) {
  const entries = MATH_ROWS.slice(0, clampRows(rows)).map(buildMathTableEntry);
  return formatDictEntries(entries);
}

function buildTextTableValue(rows) {
  return formatListRows(tableRowsForShape("text").slice(0, clampRows(rows)));
}

function buildTableValue(shape, rows) {
  if (shape === "text") return buildTextTableValue(rows);
  if (shape === "dict") return buildDictTableValue(rows);
  if (shape === "matrix") return buildMatrixTableValue(rows);
  return buildListTableValue(rows);
}

function scanWqTopLevel(text, visit) {
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (let idx = 0; idx < text.length; idx += 1) {
    const char = text[idx];
    if (inString) {
      if (escaped) {
        escaped = false;
      } else if (char === "\\") {
        escaped = true;
      } else if (char === '"') {
        inString = false;
      }
      continue;
    }
    if (char === '"') {
      inString = true;
      continue;
    }
    if (char === "(" || char === "[" || char === "{") {
      depth += 1;
    } else if (char === ")" || char === "]" || char === "}") {
      depth -= 1;
      if (depth < 0) return false;
    }
    if (visit(char, idx, depth) === false) return false;
  }
  return depth === 0 && !inString;
}

function unwrapWqParens(text) {
  const value = String(text || "").trim();
  if (!value.startsWith("(") || !value.endsWith(")")) return null;
  let validOuter = false;
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (let idx = 0; idx < value.length; idx += 1) {
    const char = value[idx];
    if (inString) {
      if (escaped) {
        escaped = false;
      } else if (char === "\\") {
        escaped = true;
      } else if (char === '"') {
        inString = false;
      }
      continue;
    }
    if (char === '"') {
      inString = true;
      continue;
    }
    if (char === "(" || char === "[" || char === "{") {
      depth += 1;
    } else if (char === ")" || char === "]" || char === "}") {
      depth -= 1;
      if (depth < 0) return null;
      if (depth === 0) {
        if (idx !== value.length - 1) return null;
        validOuter = true;
      }
    }
  }
  return validOuter && depth === 0 && !inString ? value.slice(1, -1).trim() : null;
}

function splitWqTopLevel(text, delimiter = ";") {
  const parts = [];
  let start = 0;
  const valid = scanWqTopLevel(text, (char, idx, depth) => {
    if (char === delimiter && depth === 0) {
      parts.push(text.slice(start, idx).trim());
      start = idx + 1;
    }
    return true;
  });
  if (!valid) return null;
  const tail = text.slice(start).trim();
  if (tail) parts.push(tail);
  return parts.filter(Boolean);
}

function topLevelIndexOf(text, needle) {
  let found = -1;
  scanWqTopLevel(text, (char, idx, depth) => {
    if (char === needle && depth === 0) {
      found = idx;
      return false;
    }
    return true;
  });
  return found;
}

function parseWqListItems(text) {
  const body = unwrapWqParens(text);
  if (body === null) return null;
  if (!body) return [];
  return splitWqTopLevel(body);
}

function keyNameFromWq(key) {
  const value = key.trim();
  return value.startsWith("`") ? value.slice(1) : value;
}

function parseWqDictEntries(text) {
  const items = parseWqListItems(text);
  if (!items) return null;
  const entries = [];
  for (const item of items) {
    const colon = topLevelIndexOf(item, ":");
    if (colon < 0) return null;
    const key = item.slice(0, colon).trim();
    const value = item.slice(colon + 1).trim();
    if (!key || !value) return null;
    entries.push({
      key,
      keyName: keyNameFromWq(key),
      value,
    });
  }
  return entries;
}

function resizeListRows(source, shape, rows) {
  const currentRows = parseWqListItems(source);
  if (!currentRows) return null;
  const generatedRows = tableRowsForShape(shape);
  const nextRows = currentRows.slice(0, rows);
  for (let idx = currentRows.length; idx < rows; idx += 1) {
    nextRows.push(generatedRows[idx] || generatedRows[generatedRows.length - 1]);
  }
  return formatListRows(nextRows);
}

function resizeDictColumns(source, rows) {
  const entries = parseWqDictEntries(source);
  if (!entries) return null;
  const generatedColumns = new Map(CHEMISTRY_COLUMNS.map((column) => [column.key, column.values]));
  const nextEntries = entries.map((entry) => {
    const values = parseWqListItems(entry.value);
    if (!values) return entry;
    const generatedValues = generatedColumns.get(entry.keyName);
    const nextValues = values.slice(0, rows);
    for (let idx = values.length; idx < rows; idx += 1) {
      nextValues.push(generatedValues?.[idx] || '""');
    }
    return {
      ...entry,
      value: inlineWqList(nextValues),
    };
  });
  return formatDictEntries(nextEntries);
}

function resizeDictRows(source, rows) {
  const entries = parseWqDictEntries(source);
  if (!entries) return null;
  const generatedEntries = MATH_ROWS.map(buildMathTableEntry);
  const nextEntries = entries.slice(0, rows);
  for (let idx = entries.length; idx < rows; idx += 1) {
    nextEntries.push(generatedEntries[idx] || generatedEntries[generatedEntries.length - 1]);
  }
  return formatDictEntries(nextEntries);
}

function resizeTableValue(source, shape, rows) {
  const rowCount = clampRows(rows);
  if (shape === "dict") return resizeDictColumns(source, rowCount);
  if (shape === "matrix") return resizeDictRows(source, rowCount);
  return resizeListRows(source, shape, rowCount);
}

function buildTableCode(state) {
  const source = String(state.sourceExpr || "").trim() || buildTableValue(state.tableShape, state.rows);
  if (source.startsWith("showtable")) return source;
  const args = [
    indentWqBlock(source),
    textListOption("cols", state.tableColsText),
    numberTextOption("limit", state.tableLimitText),
    numberTextOption("width", state.tableWidthText),
    named("style", wqString(state.tableStyle || "plain")),
    textOption("missing", state.tableMissingText),
  ].filter(Boolean);
  return `showtable[\n${args.join(";\n")}\n]`;
}

function buildCode(state) {
  return state.sourceKind === "table" ? buildTableCode(state) : buildPlotCode(state);
}

function setStatus(instance, text, tone = "") {
  if (!instance.status) return;
  instance.status.textContent = text;
  instance.status.dataset.tone = tone;
}

function closeSelect(field) {
  const button = field?.querySelector(".viz-select-button");
  const menu = field?.querySelector(".viz-select-menu");
  button?.setAttribute("aria-expanded", "false");
  menu?.classList.remove("open");
}

function openSelect(field) {
  const button = field?.querySelector(".viz-select-button");
  const menu = field?.querySelector(".viz-select-menu");
  button?.setAttribute("aria-expanded", "true");
  menu?.classList.add("open");
}

function closeAllSelects(root, except) {
  root.querySelectorAll("[data-viz-select]").forEach((field) => {
    if (field !== except) closeSelect(field);
  });
}

function setSelectValue(instance, key, value) {
  instance.state[key] = value;
  const field = instance.selects[key];
  if (!field) return;
  const valueEl = field.querySelector("[data-viz-select-value]");
  const option = Array.from(field.querySelectorAll("[data-viz-option]")).find(
    (button) => button.dataset.vizOption === value,
  );
  valueEl.textContent = option?.textContent?.trim() || value;
  field.querySelectorAll("[data-viz-option]").forEach((button) => {
    const active = button.dataset.vizOption === value;
    button.classList.toggle("active", active);
    button.setAttribute("aria-selected", active ? "true" : "false");
  });
}

function applyThemePresetToControls(instance, theme) {
  const preset = THEME_PRESETS[theme];
  if (!preset) return;
  for (const [key, value] of Object.entries(preset)) {
    setSelectValue(instance, key, value);
  }
}

function cssPixels(value) {
  const number = Number.parseFloat(value);
  return Number.isFinite(number) ? number : 0;
}

function outputFontForMeasure(output) {
  const style = window.getComputedStyle(output);
  return [
    style.fontStyle,
    style.fontVariant,
    style.fontWeight,
    style.fontSize,
    style.fontFamily,
  ].join(" ");
}

function measureOutputCharWidth(output) {
  if (!output || typeof document === "undefined") return null;
  const canvas =
    measureOutputCharWidth.canvas || (measureOutputCharWidth.canvas = document.createElement("canvas"));
  const context = canvas.getContext("2d");
  if (!context) return null;
  context.font = outputFontForMeasure(output);
  const width = context.measureText("0000000000").width / 10;
  return width > 0 ? width : null;
}

function measureOutputPlotWidth(instance) {
  const output = instance.output;
  if (!output || typeof window === "undefined") return null;
  const style = window.getComputedStyle(output);
  const innerWidth =
    output.clientWidth - cssPixels(style.paddingLeft) - cssPixels(style.paddingRight);
  const charWidth = measureOutputCharWidth(output);
  if (innerWidth <= 0 || !charWidth) return null;
  return clampPlotWidth(Math.floor(innerWidth / charWidth) - plotWidthReserve(instance.state));
}

function syncWidthControl(instance) {
  const input = instance.ranges.width;
  const label = instance.root.querySelector('[data-viz-range-value="width"]');
  const manualWidth = clampPlotWidth(instance.state.width);
  const plotWidth = effectivePlotWidth(instance.state);

  if (input) {
    input.value = String(manualWidth);
    input.disabled = !!instance.state.widthAuto;
    input.title = instance.state.widthAuto ? "Auto width is on" : "";
  }
  if (label) {
    label.textContent = instance.state.widthAuto ? `auto ${plotWidth}` : String(manualWidth);
  }
}

function refreshAutoWidth(instance) {
  const nextWidth = measureOutputPlotWidth(instance);
  if (nextWidth === null) {
    syncWidthControl(instance);
    return false;
  }
  const changed = nextWidth !== instance.state.computedWidth;
  instance.state.computedWidth = nextWidth;
  syncWidthControl(instance);
  return changed && !!instance.state.widthAuto;
}

function setRangeValue(instance, key, value) {
  if (key === "width") {
    instance.state.width = clampPlotWidth(value);
    syncWidthControl(instance);
    return;
  }
  instance.state[key] = key === "rows" ? clampRows(value) : Number(value);
  const input = instance.ranges[key];
  const label = instance.root.querySelector(`[data-viz-range-value="${key}"]`);
  if (input) input.value = String(instance.state[key]);
  if (label) label.textContent = String(instance.state[key]);
}

function setToggleValue(instance, key, value) {
  instance.state[key] = !!value;
  const input = instance.toggles[key];
  if (input) input.checked = !!value;
  if (key === "widthAuto") {
    syncWidthControl(instance);
  }
}

function setLayoutValue(instance, value) {
  instance.state.layout = value === "side" ? "side" : "below";
  instance.root.dataset.vizLayout = instance.state.layout;
  instance.layoutButtons.forEach((button) => {
    const active = button.dataset.vizLayoutOption === instance.state.layout;
    button.classList.toggle("active", active);
    button.setAttribute("aria-pressed", active ? "true" : "false");
  });
}

function setInputValue(instance, key, value) {
  instance.state[key] = value;
  const input = instance.inputs[key];
  if (input) input.value = value;
}

function clampRows(value) {
  const number = Math.round(Number(value));
  if (!Number.isFinite(number)) return 1;
  return Math.max(1, Math.min(8, number));
}

function finiteNumber(text) {
  const value = Number(String(text || "").trim());
  return Number.isFinite(value) ? value : null;
}

function formatLimitNumber(value) {
  if (!Number.isFinite(value)) return "";
  const rounded = Number(value.toFixed(6));
  return String(rounded);
}

function setLimitInputValue(instance, key, value) {
  const meta = LIMIT_INPUTS[key];
  if (!meta) {
    setInputValue(instance, key, value);
    return;
  }

  const previousValue = instance.state[key] || "";
  setInputValue(instance, key, value);
  if (!instance.state[meta.lockKey]) return;

  const previousNumber = finiteNumber(previousValue);
  const nextNumber = finiteNumber(value);
  const partnerNumber = finiteNumber(instance.state[meta.partnerKey]);
  if (previousNumber === null || nextNumber === null || partnerNumber === null) return;

  const partnerValue = formatLimitNumber(partnerNumber + nextNumber - previousNumber);
  setInputValue(instance, meta.partnerKey, partnerValue);
}

function syncTableSource(instance, options = {}) {
  if (instance.state.sourceKind !== "table") return;
  instance.state.rows = clampRows(instance.state.rows);
  const current = String(instance.state.sourceExpr || "");
  const next =
    options.preserveCurrent === false || !current.trim()
      ? buildTableValue(instance.state.tableShape, instance.state.rows)
      : resizeTableValue(current, instance.state.tableShape, instance.state.rows);
  if (next !== null) {
    setInputValue(instance, "sourceExpr", next);
  }
}

function syncTableDisplayDefaults(instance, shape) {
  const defaults = TABLE_SHAPE_DEFAULTS[shape] || TABLE_SHAPE_DEFAULTS.text;
  for (const [key, value] of Object.entries(defaults)) {
    setInputValue(instance, key, value);
  }
  setSelectValue(instance, "tableStyle", defaults.tableStyle);
}

function seedSourceForKind(instance, kind) {
  instance.state.series = kind === "table" ? [] : cloneSeries(defaultSeriesForKind("function"));
  if (kind === "table") {
    syncTableDisplayDefaults(instance, instance.state.tableShape);
    syncTableSource(instance, { preserveCurrent: false });
  }
  renderSeriesEditor(instance);
}

function makeSeriesTextField(instance, row, idx, key, labelText, options = {}) {
  const field = document.createElement(key === "expr" ? "div" : "label");
  field.className = `viz-series-field viz-series-field-${key}`;
  field.classList.toggle("is-disabled", Boolean(options.disabled));
  const label = document.createElement("span");
  label.textContent = labelText;

  if (key === "expr") {
    const input = document.createElement("textarea");
    input.className = "viz-series-expr-input editor-text";
    input.rows = 1;
    input.spellcheck = false;
    input.placeholder = "sin, @s x^2, or (1;2;3)";
    input.setAttribute("aria-label", `Series ${idx + 1} expression`);
    input.value = row[key] || "";
    field.append(label, input);

    const editor = createWqEditor(input, { multilineMode: "none" });
    editor.addEventListener("input", () => {
      instance.state.series[idx][key] = editor.value;
      updateView(instance);
    });
    return field;
  }

  const input = document.createElement("input");
  input.type = "text";
  input.spellcheck = false;
  input.value = row[key] || "";
  input.disabled = Boolean(options.disabled);
  if (options.disabled && options.disabledReason) {
    field.title = options.disabledReason;
    input.title = options.disabledReason;
  }
  input.addEventListener("input", () => {
    instance.state.series[idx][key] = input.value;
    updateView(instance);
  });
  field.append(label, input);
  return field;
}

function makeSeriesSelectField(instance, row, idx, key, labelText, options, fieldOptions = {}) {
  const field = document.createElement("div");
  field.className = `viz-series-field viz-series-field-${key} viz-field`;
  field.dataset.vizSelect = "";
  field.classList.toggle("is-disabled", Boolean(fieldOptions.disabled));
  if (fieldOptions.disabled && fieldOptions.disabledReason) {
    field.title = fieldOptions.disabledReason;
  }

  const label = document.createElement("span");
  label.textContent = labelText;

  const button = document.createElement("button");
  button.className = "viz-select-button";
  button.type = "button";
  button.disabled = Boolean(fieldOptions.disabled);
  button.setAttribute("aria-haspopup", "listbox");
  button.setAttribute("aria-expanded", "false");
  if (fieldOptions.disabled && fieldOptions.disabledReason) {
    button.title = fieldOptions.disabledReason;
  }

  const valueLabel = document.createElement("span");
  valueLabel.dataset.vizSelectValue = "";
  button.appendChild(valueLabel);

  const menu = document.createElement("div");
  menu.className = "viz-select-menu";
  menu.role = "listbox";

  const setValue = (value) => {
    const selectedValue = value || "";
    instance.state.series[idx][key] = selectedValue;
    valueLabel.textContent =
      options.find(([optionValue]) => optionValue === selectedValue)?.[1] || "default";
    menu.querySelectorAll("[data-viz-option]").forEach((option) => {
      const active = option.dataset.vizOption === selectedValue;
      option.classList.toggle("active", active);
      option.setAttribute("aria-selected", active ? "true" : "false");
    });
  };

  options.forEach(([value, text]) => {
    const option = document.createElement("button");
    option.type = "button";
    option.role = "option";
    option.dataset.vizOption = value;
    option.textContent = text;
    menu.appendChild(option);
  });

  button.addEventListener("click", () => {
    if (fieldOptions.disabled) return;
    const isOpen = menu.classList.contains("open");
    closeAllSelects(instance.root, isOpen ? null : field);
    if (isOpen) {
      closeSelect(field);
    } else {
      openSelect(field);
    }
  });

  menu.addEventListener("click", (event) => {
    if (fieldOptions.disabled) return;
    const option = event.target.closest("[data-viz-option]");
    if (!option) return;
    setValue(option.dataset.vizOption);
    closeSelect(field);
    updateView(instance);
  });

  setValue(row[key] || "");
  field.append(label, button, menu);
  return field;
}

function makeSeriesModeField(instance, row, idx) {
  return makeSeriesSelectField(instance, row, idx, "mode", "Mode", SERIES_MODE_OPTIONS, {
    disabled: !instance.state.seriesOptions,
    disabledReason: "Turn on Per-series to edit row modes",
  });
}

function renderSeriesEditor(instance) {
  if (!instance.seriesList) return;
  if (instance.state.sourceKind === "table") {
    instance.seriesList.innerHTML = "";
    return;
  }
  if (!instance.state.series.length) {
    instance.state.series = cloneSeries(defaultSeriesForKind("function"));
  }
  instance.seriesList.innerHTML = "";
  instance.state.series.forEach((row, idx) => {
    const item = document.createElement("div");
    item.className = "viz-series-row";
    item.append(
      makeSeriesTextField(instance, row, idx, "expr", "Expr"),
      makeSeriesTextField(instance, row, idx, "label", "Label", {
        disabled: !instance.state.labels,
        disabledReason: "Turn on Labels to edit row labels",
      }),
      makeSeriesTextField(instance, row, idx, "symbol", "Symbol"),
      makeSeriesModeField(instance, row, idx),
    );
    const remove = document.createElement("button");
    remove.className = "viz-series-remove";
    remove.type = "button";
    remove.textContent = "Remove";
    remove.disabled = instance.state.series.length === 1;
    remove.addEventListener("click", () => {
      instance.state.series.splice(idx, 1);
      renderSeriesEditor(instance);
      updateView(instance);
    });
    item.appendChild(remove);
    instance.seriesList.appendChild(item);
  });
}

function applyPreset(instance, key, options = {}) {
  instance.state = stateForPreset(key);

  instance.presetButtons.forEach((button) => {
    button.classList.toggle("active", button.dataset.vizPreset === key);
  });
  for (const selectKey of Object.keys(instance.selects)) {
    if (instance.state[selectKey] !== undefined) {
      setSelectValue(instance, selectKey, String(instance.state[selectKey]));
    }
  }
  for (const rangeKey of Object.keys(instance.ranges)) {
    if (instance.state[rangeKey] !== undefined) {
      setRangeValue(instance, rangeKey, instance.state[rangeKey]);
    }
  }
  for (const toggleKey of Object.keys(instance.toggles)) {
    if (instance.state[toggleKey] !== undefined) {
      setToggleValue(instance, toggleKey, instance.state[toggleKey]);
    }
  }
  setLayoutValue(instance, instance.state.layout);
  for (const inputKey of Object.keys(instance.inputs)) {
    if (instance.state[inputKey] !== undefined) {
      setInputValue(instance, inputKey, String(instance.state[inputKey]));
    }
  }
  renderSeriesEditor(instance);
  updateView(instance, options);
}

function renderCode(instance) {
  instance.code = buildCode(instance.state);
  if (!instance.codeEl) return;
  try {
    instance.codeEl.innerHTML = highlight_wq(instance.code);
  } catch (_err) {
    instance.codeEl.innerHTML = escapeHtml(instance.code);
  }
}

function setCopyCodeState(instance, text, tone = "") {
  if (!instance.copyCodeBtn) return;
  clearTimeout(instance.copyCodeTimer);
  instance.copyCodeBtn.textContent = text;
  instance.copyCodeBtn.dataset.tone = tone;
  if (text !== "Copy") {
    instance.copyCodeTimer = setTimeout(() => {
      instance.copyCodeBtn.textContent = "Copy";
      instance.copyCodeBtn.dataset.tone = "";
    }, 1400);
  }
}

async function writeClipboardText(text) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const scratch = document.createElement("textarea");
  scratch.value = text;
  scratch.setAttribute("readonly", "");
  scratch.style.position = "fixed";
  scratch.style.opacity = "0";
  document.body.appendChild(scratch);
  scratch.select();
  try {
    document.execCommand("copy");
  } finally {
    scratch.remove();
  }
}

async function copyCode(instance) {
  if (!instance.code) renderCode(instance);
  try {
    await writeClipboardText(instance.code || "");
    setCopyCodeState(instance, "Copied", "ok");
  } catch (_err) {
    setCopyCodeState(instance, "Failed", "error");
  }
}

function scheduleRun(instance, delay = 160) {
  clearTimeout(instance.autoTimer);
  if (!instance.state.autoRun) {
    setStatus(instance, "ready");
    return;
  }
  setStatus(instance, "queued");
  instance.autoTimer = setTimeout(() => {
    runViz(instance);
  }, delay);
}

function updateView(instance, options = {}) {
  const preset = PRESETS[instance.state.preset] || PRESETS.trig;
  const sourceKind = instance.state.sourceKind || "plot";
  const builtin = sourceKind === "table" ? "showtable" : "asciiplot";
  instance.title.textContent = instance.state.title || preset.title;
  instance.subtitle.textContent = `${builtin} / ${SOURCE_KIND_LABELS[sourceKind] || sourceKind} source`;
  instance.builtin.textContent = builtin;
  instance.root.dataset.vizBuiltin = builtin;
  instance.root.dataset.vizSourceKind = sourceKind;
  instance.root.dataset.vizLayout = instance.state.layout || "below";
  refreshAutoWidth(instance);
  renderCode(instance);
  if (options.run === false) {
    setStatus(instance, "ready");
  } else {
    scheduleRun(instance, options.delay);
  }
}

async function runViz(instance) {
  clearTimeout(instance.autoTimer);
  if (instance.isRunning) {
    instance.pendingRun = true;
    setStatus(instance, "queued");
    return;
  }
  if (refreshAutoWidth(instance) || !instance.code) renderCode(instance);
  instance.isRunning = true;
  instance.pendingRun = false;
  instance.output.innerHTML = "";
  instance.runBtn.disabled = true;
  setStatus(instance, "running");
  const renderer = createOutputRenderer(instance.output);
  try {
    await ensureWasm();
    const result = await queueEval(() => {
      set_stdout_callback((chunk) => {
        renderer.appendLegacyAnsi(chunk);
        instance.output.scrollTop = instance.output.scrollHeight;
      });
      set_stderr_callback((chunk) => {
        renderer.appendStyledText(chunk, "error");
        instance.output.scrollTop = instance.output.scrollHeight;
      });
      const session = new WasmWqSession();
      try {
        session.set_box_flags("0");
        return session.eval_wq_result(instance.code);
      } finally {
        session.free();
      }
    });
    if (result.value !== undefined && result.value !== null && String(result.value).length) {
      const bar = document.createElement("span");
      bar.className = "repl-bar repl-bar-success";
      bar.textContent = "\u258d ";
      instance.output.appendChild(bar);
      const resultRenderer = createOutputRenderer(instance.output, bar);
      resultRenderer.appendOutput(alignTurnBody(String(result.value)) + "\n");
    }
    setStatus(instance, "done", "ok");
  } catch (err) {
    const bar = document.createElement("span");
    bar.className = "repl-bar repl-bar-error";
    bar.textContent = "\u258d ";
    instance.output.appendChild(bar);
    const errorRenderer = createOutputRenderer(instance.output, bar);
    errorRenderer.appendOutput(
      alignTurnBody((err?.message ?? String(err)) + "\n"),
      "error",
    );
    setStatus(instance, "error", "error");
  } finally {
    instance.isRunning = false;
    instance.runBtn.disabled = false;
    instance.output.scrollTop = instance.output.scrollHeight;
    if (instance.pendingRun && instance.state.autoRun) {
      instance.pendingRun = false;
      scheduleRun(instance, 60);
    }
  }
}

function wireSelect(instance, field) {
  const key = field.dataset.vizSelect;
  const button = field.querySelector(".viz-select-button");
  const menu = field.querySelector(".viz-select-menu");
  instance.selects[key] = field;
  button?.addEventListener("click", () => {
    const isOpen = menu?.classList.contains("open");
    closeAllSelects(instance.root, isOpen ? null : field);
    if (isOpen) {
      closeSelect(field);
    } else {
      openSelect(field);
    }
  });
  menu?.addEventListener("click", (event) => {
    const option = event.target.closest("[data-viz-option]");
    if (!option) return;
    const nextValue = option.dataset.vizOption;
    const previousValue = instance.state[key];
    setSelectValue(instance, key, nextValue);
    if (key === "sourceKind" && nextValue !== previousValue) {
      seedSourceForKind(instance, nextValue);
    }
    if (key === "theme" && nextValue !== previousValue) {
      applyThemePresetToControls(instance, nextValue);
    }
    if (key === "tableShape") {
      syncTableDisplayDefaults(instance, nextValue);
      syncTableSource(instance, { preserveCurrent: false });
    }
    closeSelect(field);
    updateView(instance);
  });
}

export async function mountViz(root) {
  await ensureWasm();
  const tableSourceTextarea = root.querySelector('textarea[data-viz-input="sourceExpr"]');
  const tableSourceEditor = tableSourceTextarea
    ? createWqEditor(tableSourceTextarea, { multilineMode: "plain" })
    : null;
  if (tableSourceEditor) {
    tableSourceEditor.element.dataset.vizInput = "sourceExpr";
  }
  const instance = {
    root,
    state: stateForPreset("trig"),
    code: "",
    autoTimer: 0,
    isRunning: false,
    pendingRun: false,
    title: root.querySelector("[data-viz-title]"),
    subtitle: root.querySelector("[data-viz-subtitle]"),
    builtin: root.querySelector("[data-viz-builtin]"),
    status: root.querySelector("[data-viz-status]"),
    output: root.querySelector("[data-viz-output]"),
    codeEl: root.querySelector("[data-viz-code]"),
    copyCodeBtn: root.querySelector("[data-viz-copy-code]"),
    copyCodeTimer: 0,
    runBtn: root.querySelector("[data-viz-run]"),
    openBtn: root.querySelector("[data-viz-open]"),
    addSeriesBtn: root.querySelector("[data-viz-add-series]"),
    seriesList: root.querySelector("[data-viz-series-list]"),
    presetButtons: Array.from(root.querySelectorAll("[data-viz-preset]")),
    layoutButtons: Array.from(root.querySelectorAll("[data-viz-layout-option]")),
    stepButtons: Array.from(root.querySelectorAll("[data-viz-step]")),
    selects: {},
    ranges: Object.fromEntries(
      Array.from(root.querySelectorAll("[data-viz-range]")).map((input) => [
        input.dataset.vizRange,
        input,
      ]),
    ),
    toggles: Object.fromEntries(
      Array.from(root.querySelectorAll("[data-viz-toggle]")).map((input) => [
        input.dataset.vizToggle,
        input,
      ]),
    ),
    inputs: Object.fromEntries(
      Array.from(root.querySelectorAll("[data-viz-input]")).map((input) => [
        input.dataset.vizInput,
        input,
      ]),
    ),
  };
  if (tableSourceEditor) {
    instance.inputs.sourceExpr = tableSourceEditor;
  }
  instances.set(root, instance);

  root.querySelectorAll("[data-viz-select]").forEach((field) => {
    wireSelect(instance, field);
  });
  Object.entries(instance.ranges).forEach(([key, input]) => {
    input.addEventListener("input", () => {
      if (key === "width") {
        setToggleValue(instance, "widthAuto", false);
      }
      setRangeValue(instance, key, input.value);
      if (key === "rows") {
        syncTableSource(instance);
      }
      updateView(instance);
    });
  });
  instance.stepButtons.forEach((button) => {
    button.addEventListener("click", () => {
      const key = button.dataset.vizStep;
      const input = instance.ranges[key];
      const delta = Number(button.dataset.vizStepDelta) || 0;
      const current = Number(input?.value || instance.state[key] || 0);
      if (key === "width") {
        setToggleValue(instance, "widthAuto", false);
      }
      setRangeValue(instance, key, current + delta);
      if (key === "rows") {
        syncTableSource(instance);
      }
      updateView(instance);
    });
  });
  Object.entries(instance.toggles).forEach(([key, input]) => {
    input.addEventListener("change", () => {
      setToggleValue(instance, key, input.checked);
      if (key === "widthAuto" && input.checked) {
        refreshAutoWidth(instance);
      }
      if (key === "labels" || key === "seriesOptions") {
        renderSeriesEditor(instance);
      }
      updateView(instance);
    });
  });
  Object.entries(instance.inputs).forEach(([key, input]) => {
    input.addEventListener("input", () => {
      setLimitInputValue(instance, key, input.value);
      updateView(instance);
    });
  });
  instance.layoutButtons.forEach((button) => {
    button.addEventListener("click", () => {
      setLayoutValue(instance, button.dataset.vizLayoutOption);
    });
  });
  instance.addSeriesBtn?.addEventListener("click", () => {
    instance.state.series.push({
      expr: "",
      label: "",
      symbol: "",
      mode: instance.state.mode,
    });
    renderSeriesEditor(instance);
    updateView(instance);
  });
  instance.presetButtons.forEach((button) => {
    button.addEventListener("click", () => {
      applyPreset(instance, button.dataset.vizPreset || "trig");
    });
  });
  instance.runBtn?.addEventListener("click", async () => {
    await runViz(instance);
  });
  instance.copyCodeBtn?.addEventListener("click", async () => {
    await copyCode(instance);
  });
  instance.openBtn?.addEventListener("click", () => {
    window.navigate(`playground.html?code=${encodeURIComponent(instance.code)}`);
  });
  document.addEventListener("click", (event) => {
    if (!root.contains(event.target)) return;
    if (!event.target.closest("[data-viz-select]")) {
      closeAllSelects(root);
    }
  });

  if (typeof ResizeObserver !== "undefined" && instance.output) {
    instance.widthObserver = new ResizeObserver(() => {
      if (refreshAutoWidth(instance)) {
        updateView(instance, { delay: 80 });
      }
    });
    instance.widthObserver.observe(instance.output);
  }

  applyPreset(instance, "trig", { run: false });
  await runViz(instance);
}

export async function activateViz(root) {
  const instance = instances.get(root);
  if (!instance) return;
  await ensureWasm();
  updateView(instance, { run: false });
}

export function applyVizRoute(root, params) {
  const instance = instances.get(root);
  if (!instance) return;
  const preset = params.get("preset");
  if (preset && PRESETS[preset] && preset !== instance.state.preset) {
    applyPreset(instance, preset);
  }
}
