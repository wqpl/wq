import { WasmWqSession } from "wq-wasm";
import { createOutputRenderer } from "./ansi.js";
import { createWqEditor } from "./editor.js";
import { named, plotSeriesArg, wqString } from "./viz-codegen.js";
import { DEFAULT_STATE, PRESETS } from "./viz-presets.js";
import {
  alignTurnBody,
  ensureWasm,
  getWqFrontend,
  escapeHtml,
  formatWqError,
  queueEval,
} from "./wq-shared.js";

const instances = new WeakMap();

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
  const unwrapped =
    value.startsWith("(") && value.endsWith(")") ? value.slice(1, -1) : value;
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
  return hydrateLimitState({
    ...DEFAULT_STATE,
    ...preset,
    series: cloneSeries(preset.series || DEFAULT_STATE.series),
    preset: key,
  });
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

function textListOption(name, text) {
  const values = String(text || "")
    .split(/[;,\n]+/)
    .map((value) => value.trim())
    .filter(Boolean);
  if (!values.length) return null;
  if (values.length === 1) return named(name, wqString(values[0]));
  return named(name, `(${values.map(wqString).join(";")})`);
}

function normalizedSeries(state) {
  const series = cloneSeries(state.series).filter((item) => item.expr.trim());
  return series.length ? series : cloneSeries(DEFAULT_STATE.series);
}

function labelsOption(state, series) {
  if (!state.labels) return null;
  const labels = series.map((item) => item.label.trim()).filter(Boolean);
  return labels.length
    ? named("labels", `(${labels.map(wqString).join(";")})`)
    : null;
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
  args.push(axesOption(state), gridOption(state), colorOption(state));
  if (state.unicode) {
    args.push(named("unicode", boolLit(state.unicode)));
  }
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
  return `asciiplot[\n${args.join(";\n")}]`;
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

function buildCode(state) {
  return buildPlotCode(state);
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

function closePresetMenu(instance) {
  if (instance.presetMenuPanel?.contains(document.activeElement)) {
    instance.presetMenuButton?.focus();
  }
  instance.presetMenuButton?.setAttribute("aria-expanded", "false");
  instance.presetMenuPanel?.classList.remove("open");
}

function openPresetMenu(instance) {
  closeAllSelects(instance.root);
  instance.presetMenuButton?.setAttribute("aria-expanded", "true");
  instance.presetMenuPanel?.classList.add("open");
  if (instance.presetMenuPanel) {
    instance.presetMenuPanel.scrollTop = 0;
  }
}

function togglePresetMenu(instance) {
  if (instance.presetMenuPanel?.classList.contains("open")) {
    closePresetMenu(instance);
  } else {
    openPresetMenu(instance);
  }
}

function setActivePreset(instance, key) {
  instance.presetButtons.forEach((button) => {
    const active = button.dataset.vizPreset === key;
    button.classList.toggle("active", active);
    button.setAttribute("aria-checked", active ? "true" : "false");
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
    measureOutputCharWidth.canvas ||
    (measureOutputCharWidth.canvas = document.createElement("canvas"));
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
    output.clientWidth -
    cssPixels(style.paddingLeft) -
    cssPixels(style.paddingRight);
  const charWidth = measureOutputCharWidth(output);
  if (innerWidth <= 0 || !charWidth) return null;
  return clampPlotWidth(
    Math.floor(innerWidth / charWidth) - plotWidthReserve(instance.state),
  );
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
    label.textContent = instance.state.widthAuto
      ? `auto ${plotWidth}`
      : String(manualWidth);
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
  instance.state[key] = Number(value);
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
  if (previousNumber === null || nextNumber === null || partnerNumber === null)
    return;

  const partnerValue = formatLimitNumber(
    partnerNumber + nextNumber - previousNumber,
  );
  setInputValue(instance, meta.partnerKey, partnerValue);
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
    input.rows = 3;
    input.spellcheck = false;
    input.placeholder = "sin, @s x^2, or (1;2;3)";
    input.setAttribute("aria-label", `Series ${idx + 1} expression`);
    input.value = row[key] || "";
    field.append(label, input);

    const editor = createWqEditor(input, {
      multilineMode: "plain",
      frontend: instance.frontend,
    });
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

function makeSeriesSelectField(
  instance,
  row,
  idx,
  key,
  labelText,
  options,
  fieldOptions = {},
) {
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
      options.find(([optionValue]) => optionValue === selectedValue)?.[1] ||
      "default";
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
  return makeSeriesSelectField(
    instance,
    row,
    idx,
    "mode",
    "Mode",
    SERIES_MODE_OPTIONS,
    {
      disabled: !instance.state.seriesOptions,
      disabledReason: "Turn on Per-series to edit row modes",
    },
  );
}

function renderSeriesEditor(instance) {
  if (!instance.seriesList) return;
  if (!instance.state.series.length) {
    instance.state.series = cloneSeries(DEFAULT_STATE.series);
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

  setActivePreset(instance, key);
  closePresetMenu(instance);
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
    instance.codeEl.innerHTML = instance.frontend.highlight_wq(instance.code);
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
  instance.title.textContent = instance.state.title || preset.title;
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
    await queueEval(async () => {
      const session = new WasmWqSession();
      try {
        session.set_stdout_callback((chunk) => {
          renderer.appendStreamOutput(chunk);
          instance.output.scrollTop = instance.output.scrollHeight;
        });
        session.set_stderr_callback((chunk) => {
          renderer.appendStreamOutput(chunk, "error");
          instance.output.scrollTop = instance.output.scrollHeight;
        });
        await session.eval_wq_async(instance.code);
      } finally {
        session.free();
      }
    });
    setStatus(instance, "done", "ok");
  } catch (err) {
    const bar = document.createElement("span");
    bar.className = "repl-bar repl-bar-error";
    bar.textContent = "\u258d ";
    instance.output.appendChild(bar);
    const errorRenderer = createOutputRenderer(instance.output, bar);
    errorRenderer.appendOutput(
      alignTurnBody(formatWqError(err, { rendered: true }) + "\n"),
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
    setSelectValue(instance, key, nextValue);
    if (key === "theme") {
      applyThemePresetToControls(instance, nextValue);
    }
    closeSelect(field);
    updateView(instance);
  });
}

export async function mountViz(root) {
  await ensureWasm();
  const instance = {
    root,
    frontend: getWqFrontend(),
    state: stateForPreset("trig"),
    code: "",
    autoTimer: 0,
    isRunning: false,
    pendingRun: false,
    title: root.querySelector("[data-viz-title]"),
    status: root.querySelector("[data-viz-status]"),
    output: root.querySelector("[data-viz-output]"),
    codeEl: root.querySelector("[data-viz-code]"),
    copyCodeBtn: root.querySelector("[data-viz-copy-code]"),
    copyCodeTimer: 0,
    runBtn: root.querySelector("[data-viz-run]"),
    addSeriesBtn: root.querySelector("[data-viz-add-series]"),
    seriesList: root.querySelector("[data-viz-series-list]"),
    presetMenu: root.querySelector("[data-viz-preset-menu]"),
    presetMenuButton: root.querySelector("[data-viz-preset-toggle]"),
    presetMenuPanel: root.querySelector("[data-viz-preset-panel]"),
    presetButtons: Array.from(root.querySelectorAll("[data-viz-preset]")),
    layoutButtons: Array.from(
      root.querySelectorAll("[data-viz-layout-option]"),
    ),
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
  instance.presetMenuButton?.addEventListener("click", () => {
    togglePresetMenu(instance);
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
  document.addEventListener("click", (event) => {
    if (!root.contains(event.target)) {
      closeAllSelects(root);
      closePresetMenu(instance);
      return;
    }
    if (!event.target.closest("[data-viz-select]")) {
      closeAllSelects(root);
    }
    if (!event.target.closest("[data-viz-preset-menu]")) {
      closePresetMenu(instance);
    }
  });
  document.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    closeAllSelects(root);
    closePresetMenu(instance);
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
