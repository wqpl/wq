import {
  WasmWqSession,
  set_stdout_callback,
  set_stderr_callback,
  highlight_wq,
} from "wq-wasm";
import { createAnsiRenderer } from "./ansi.js";
import { createWqEditor } from "./editor.js";
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
  sourceExpr: buildTableValue("list", 5),
  layout: "below",
  mode: "line",
  complex: "re",
  theme: "none",
  axes: "full",
  grid: "4",
  palette: "classic",
  width: 90,
  height: 24,
  samples: 140,
  xlimText: "0;6.283",
  ylimText: "",
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
    xlimText: "0;6.283",
    ylimText: "",
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
    xlimText: "",
    ylimText: "",
    titleText: "raw values",
    xlabelText: "index",
    ylabelText: "value",
    series: [
      { expr: "(3;7;4;8;5;9;6;11)", label: "north", symbol: "#", mode: "bar" },
      { expr: "(2;5;7;4;10;6;12;8)", label: "south", symbol: "+", mode: "bar" },
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
    xlimText: "-4;4",
    ylimText: "",
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
    xlimText: "0;6.283",
    ylimText: "",
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
    xlimText: "",
    ylimText: "",
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
    xlimText: "-8;8",
    ylimText: "",
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
    sourceExpr: buildTableValue("list", 5),
    tableShape: "list",
    rows: 5,
  },
};

const PALETTES = {
  classic: ["red", "blue", "green", "magenta"],
  bright: ["bright_red", "bright_blue", "bright_green", "bright_magenta"],
  ink: ["cyan", "yellow", "white", "green"],
};

const SERIES_MODE_OPTIONS = [
  ["", "plot"],
  ["line", "line"],
  ["scatter", "scatter"],
  ["step", "step"],
  ["bar", "bar"],
  ["area", "area"],
];

function boolLit(value) {
  return value ? "T" : "F";
}

function wqString(value) {
  return `"${String(value).replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

function wqTag(value) {
  return `\`${value}`;
}

function named(name, value) {
  return `  \`${name}:${value}`;
}

function cloneSeries(series) {
  return (series || []).map((item) => ({
    expr: item.expr || "",
    label: item.label || "",
    symbol: item.symbol || "",
    mode: item.mode || "",
  }));
}

function stateForPreset(key) {
  const preset = PRESETS[key] || PRESETS.trig;
  return {
    ...DEFAULT_STATE,
    ...preset,
    series: cloneSeries(preset.series || DEFAULT_STATE.series),
    preset: key,
  };
}

function colorOption(state) {
  if (state.palette === "off") return named("color", "F");
  const colors = PALETTES[state.palette] || PALETTES.classic;
  return named("color", `(${colors.map(wqString).join(";")})`);
}

function gridOption(state) {
  return named("grid", state.grid === "off" ? "F" : state.grid);
}

function axesOption(state) {
  if (state.axes === "off") return named("axes", "F");
  return named("axes", wqString(state.axes));
}

function tupleLiteral(text) {
  const value = String(text || "").trim();
  if (!value) return "";
  if (value.startsWith("(") && value.endsWith(")")) return value;
  const range = value.match(/^(.+)\.\.(.+)$/);
  if (range) return `(${range[1].trim()};${range[2].trim()})`;
  const normalized = value.replace(",", ";");
  if (normalized.includes(";")) return `(${normalized})`;
  return value;
}

function rangeOption(name, text) {
  const tuple = tupleLiteral(text);
  return tuple ? named(name, tuple) : null;
}

function textOption(name, text) {
  const value = String(text || "").trim();
  return value ? named(name, wqString(value)) : null;
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
    named("size", `(${state.width};${state.height})`),
    named("samples", state.samples),
    axesOption(state),
    gridOption(state),
    named("ascii", boolLit(state.ascii)),
    colorOption(state),
  ];
  if (state.theme !== "none") {
    args.push(named("theme", wqString(state.theme)));
  }
  if (state.complex !== "re") {
    args.push(named("complex", wqString(state.complex)));
  }
  for (const option of [
    rangeOption("xlim", state.xlimText),
    rangeOption("ylim", state.ylimText),
    textOption("title", state.titleText),
    textOption("xlabel", state.xlabelText),
    textOption("ylabel", state.ylabelText),
  ]) {
    if (option) args.push(option);
  }
  return args;
}

function plotSeriesArg(series, state) {
  const expr = series.expr.trim();
  if (!state.seriesOptions) return `  ${expr}`;
  const parts = [`\`fn:${expr}`];
  const symbol = series.symbol.trim();
  const mode = series.mode.trim() || state.mode;
  const label = series.label.trim() || expr.replace(/^@s\s+/, "");
  if (symbol) parts.push(`\`symbol:${wqString(symbol)}`);
  if (mode) parts.push(`\`mode:${wqString(mode)}`);
  if (state.labels && label) parts.push(`\`label:${wqString(label)}`);
  return `  (${parts.join(";")})`;
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

function buildListTableValue(rows) {
  const entries = [
    ["asciiplot", "line", "sin", 100],
    ["asciiplot", "scatter", "cos", 120],
    ["asciiplot", "bar", "values", 8],
    ["asciiplot", "plane", "sqrt", 140],
    ["showtable", "table", "dict", 5],
    ["asciiplot", "area", "wave", 80],
    ["showtable", "sparse", "rows", 6],
    ["asciiplot", "step", "round", 40],
  ].slice(0, rows);
  return `(${entries
    .map(
      ([builtin, mode, data, points]) =>
        `(\`builtin:${wqTag(builtin)};\`mode:${wqTag(mode)};\`data:${wqTag(data)};\`points:${points})`,
    )
    .join(";")})`;
}

function buildDictTableValue(rows) {
  const series = ["sin", "cos", "sqrt", "bars", "table", "area", "sparse", "step"].slice(0, rows);
  const modes = ["line", "scatter", "plane", "bar", "table", "area", "rows", "step"].slice(0, rows);
  const points = [100, 120, 140, 8, 5, 80, 6, 40].slice(0, rows);
  return `(\`series:(${series.map(wqTag).join(";")});\`mode:(${modes
    .map(wqTag)
    .join(";")});\`points:(${points.join(";")}))`;
}

function buildMatrixTableValue(rows) {
  const names = ["plot", "table", "legend", "grid", "axis", "color", "theme", "ascii"].slice(0, rows);
  return `(${names
    .map((name, idx) => {
      const ready = idx % 2 === 0 ? "T" : "F";
      return `\`${name}:(\`builtin:${idx === 1 ? wqTag("showtable") : wqTag("asciiplot")};\`ready:${ready};\`rank:${idx + 1})`;
    })
    .join(";")})`;
}

function buildTableValue(shape, rows) {
  if (shape === "dict") return buildDictTableValue(rows);
  if (shape === "matrix") return buildMatrixTableValue(rows);
  return buildListTableValue(rows);
}

function buildTableCode(state) {
  const source = String(state.sourceExpr || "").trim() || buildTableValue(state.tableShape, state.rows);
  return source.startsWith("showtable") ? source : `showtable ${source}`;
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

function setRangeValue(instance, key, value) {
  instance.state[key] = Number(value);
  const input = instance.ranges[key];
  const label = instance.root.querySelector(`[data-viz-range-value="${key}"]`);
  if (input) input.value = String(value);
  if (label) label.textContent = String(value);
}

function setToggleValue(instance, key, value) {
  instance.state[key] = !!value;
  const input = instance.toggles[key];
  if (input) input.checked = !!value;
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

function syncTableSource(instance) {
  if (instance.state.sourceKind !== "table") return;
  setInputValue(
    instance,
    "sourceExpr",
    buildTableValue(instance.state.tableShape, instance.state.rows),
  );
}

function seedSourceForKind(instance, kind) {
  instance.state.series = kind === "table" ? [] : cloneSeries(defaultSeriesForKind("function"));
  if (kind === "table") {
    setInputValue(
      instance,
      "sourceExpr",
      buildTableValue(instance.state.tableShape, instance.state.rows),
    );
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
  if (!instance.code) renderCode(instance);
  instance.isRunning = true;
  instance.pendingRun = false;
  instance.output.innerHTML = "";
  instance.runBtn.disabled = true;
  setStatus(instance, "running");
  const renderer = createAnsiRenderer(instance.output);
  try {
    await ensureWasm();
    const result = await queueEval(() => {
      set_stdout_callback((chunk) => {
        renderer.append(chunk);
        instance.output.scrollTop = instance.output.scrollHeight;
      });
      set_stderr_callback((chunk) => {
        renderer.append("\x1b[31m" + chunk + "\x1b[0m");
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
      const resultRenderer = createAnsiRenderer(instance.output, bar);
      resultRenderer.append(alignTurnBody(String(result.value)) + "\n");
    }
    setStatus(instance, "done", "ok");
  } catch (err) {
    const bar = document.createElement("span");
    bar.className = "repl-bar repl-bar-error";
    bar.textContent = "\u258d ";
    instance.output.appendChild(bar);
    const errorRenderer = createAnsiRenderer(instance.output, bar);
    errorRenderer.append(alignTurnBody((err?.message ?? String(err)) + "\n"));
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
    if (key === "tableShape") {
      syncTableSource(instance);
    }
    closeSelect(field);
    updateView(instance);
  });
}

export async function mountViz(root) {
  await ensureWasm();
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
    runBtn: root.querySelector("[data-viz-run]"),
    openBtn: root.querySelector("[data-viz-open]"),
    addSeriesBtn: root.querySelector("[data-viz-add-series]"),
    seriesList: root.querySelector("[data-viz-series-list]"),
    presetButtons: Array.from(root.querySelectorAll("[data-viz-preset]")),
    layoutButtons: Array.from(root.querySelectorAll("[data-viz-layout-option]")),
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
  instances.set(root, instance);

  root.querySelectorAll("[data-viz-select]").forEach((field) => {
    wireSelect(instance, field);
  });
  Object.entries(instance.ranges).forEach(([key, input]) => {
    input.addEventListener("input", () => {
      setRangeValue(instance, key, input.value);
      if (key === "rows") {
        syncTableSource(instance);
      }
      updateView(instance);
    });
  });
  Object.entries(instance.toggles).forEach(([key, input]) => {
    input.addEventListener("change", () => {
      setToggleValue(instance, key, input.checked);
      if (key === "labels" || key === "seriesOptions") {
        renderSeriesEditor(instance);
      }
      updateView(instance);
    });
  });
  Object.entries(instance.inputs).forEach(([key, input]) => {
    input.addEventListener("input", () => {
      instance.state[key] = input.value;
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
  instance.openBtn?.addEventListener("click", () => {
    window.navigate(`playground.html?code=${encodeURIComponent(instance.code)}`);
  });
  document.addEventListener("click", (event) => {
    if (!root.contains(event.target)) return;
    if (!event.target.closest("[data-viz-select]")) {
      closeAllSelects(root);
    }
  });

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
