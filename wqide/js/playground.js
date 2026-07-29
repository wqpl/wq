import { WasmWqSession } from "wq-wasm";
import { createOutputRenderer } from "./ansi.js";
import { highlightedSourceHtml } from "./syntax-highlight.js";
import { appendResultPresentation } from "./result-presentation.js";
import { loadPlaygroundExamples } from "./playground-examples.js";
import {
  createPlaygroundEvaluation,
  findPlaygroundExample
} from "./playground-examples-core.js";
import {
  ensureWasm,
  getWqFrontend,
  DEBUG_FLAGS,
  BOX_FLAGS,
  parseDebugFlags,
  formatDebugFlags,
  toggleDebugFlagList,
  parseBoxFlags,
  formatBoxFlags,
  setActive,
  syncDebugButtons,
  syncBoxButtons,
  toggleRuntimePanel,
  closeRuntimePanel,
  positionRuntimePanel,
  alignTurnBody,
  escapeHtml,
  formatWqError,
  queueEval,
  handleTabKey,
  wireTabList
} from "./wq-shared.js";
import { createWqEditor } from "./editor.js";
import {
  createEvaluationController,
  isAbortError,
} from "./eval-lifecycle.js";
import {
  createDomStdinRenderer,
  createStdinRequester,
} from "./stdin-request.js";
import { createSourceMapper } from "./symbol-highlights.js";

function readDebugFlags(instance) {
  return parseDebugFlags(instance.debugFlagsInput?.value || "");
}

function writeDebugFlags(instance, flags) {
  const formatted = formatDebugFlags(flags);
  if (instance.debugFlagsInput) {
    instance.debugFlagsInput.value = formatted;
  }
  syncDebugButtons(instance.debugButtons, flags);
  setActive(instance.debugToggle, flags.length > 0);
}

function toggleDebugFlag(instance, flag) {
  const current = readDebugFlags(instance);
  writeDebugFlags(instance, toggleDebugFlagList(current, flag));
}

function ensureStateSavingSession(instance) {
  if (!instance.stateSavingSession) {
    instance.stateSavingSession = new WasmWqSession();
  }
  return instance.stateSavingSession;
}

function applyBoxMode(targetSession, instance) {
  const spec = ensureStateSavingSession(instance).get_box_flags();
  targetSession.set_box_flags(spec || "0");
}

function readBoxFlags(instance) {
  return parseBoxFlags(ensureStateSavingSession(instance).get_box_flags());
}

function writeBoxFlags(instance, flags) {
  const formatted = formatBoxFlags(flags);
  ensureStateSavingSession(instance).set_box_flags(formatted);
  syncBoxControls(instance);
}

function toggleBoxFlag(instance, flag) {
  const current = readBoxFlags(instance);
  const next = current.includes(flag)
    ? current.filter((item) => item !== flag)
    : [...current, flag];
  writeBoxFlags(instance, next);
}

function syncBoxControls(instance) {
  const flags = readBoxFlags(instance);
  syncBoxButtons(instance.boxButtons, flags);
  setActive(instance.boxBtn, flags.length > 0);
}

const instances = new WeakMap();

function saveActiveExampleFile(instance) {
  if (!instance.activeExample || !instance.activeExamplePath) return;
  instance.activeExample.files.set(instance.activeExamplePath, instance.ta.value);
}

function renderExampleFiles(instance) {
  const host = instance.exampleFiles;
  if (!host) return;
  host.innerHTML = "";
  const example = instance.activeExample;
  if (!example) {
    host.hidden = true;
    return;
  }

  for (const path of example.files.keys()) {
    const button = document.createElement("button");
    const active = path === instance.activeExamplePath;
    button.className = "playground-file";
    button.type = "button";
    button.dataset.exampleFile = path;
    button.setAttribute("aria-pressed", active ? "true" : "false");
    button.textContent = path;
    if (path === example.entryPath) {
      const entry = document.createElement("span");
      entry.className = "playground-file-entry";
      entry.textContent = "entry";
      button.appendChild(entry);
    }
    host.appendChild(button);
  }
  host.hidden = false;
}

function setEditorSource(instance, source, focus = true) {
  instance.ta.value = source;
  instance.ta.dispatchEvent(new Event("input", { bubbles: true }));
  refreshLines(instance);
  scheduleStructureRefresh(instance);
  requestPanelHeightSync(instance);
  if (focus) {
    instance.ta.focus();
    instance.ta.setSelectionRange(0, 0);
  }
}

function selectExampleFile(instance, path, focus = true) {
  if (!instance.activeExample?.files.has(path)) return;
  saveActiveExampleFile(instance);
  instance.activeExamplePath = path;
  renderExampleFiles(instance);
  setEditorSource(instance, instance.activeExample.files.get(path), focus);
}

function selectPlaygroundExample(instance, example) {
  saveActiveExampleFile(instance);
  instance.activeExample = example;
  instance.activeExamplePath = example.initialPath;
  for (const button of instance.templateButtons) {
    const active = button.dataset.template === example.id;
    button.setAttribute("aria-pressed", active ? "true" : "false");
  }
  renderExampleFiles(instance);
  setEditorSource(instance, example.files.get(example.initialPath));
}

function leaveExampleProject(instance) {
  saveActiveExampleFile(instance);
  instance.activeExample = null;
  instance.activeExamplePath = null;
  for (const button of instance.templateButtons) {
    button.setAttribute("aria-pressed", "false");
  }
  renderExampleFiles(instance);
}

function initializePlaygroundExamples(instance) {
  try {
    const loaded = loadPlaygroundExamples();
    instance.examples = loaded.examples;
    instance.exampleSources = loaded.sources;
    for (const button of instance.templateButtons) {
      button.disabled = false;
    }
    instance.exampleStatus.hidden = true;
  } catch (error) {
    console.error("[playground] examples could not be loaded", error);
    instance.examples = [];
    instance.exampleSources = new Map();
    for (const button of instance.templateButtons) {
      button.disabled = true;
    }
    const reason = error instanceof Error ? error.message : String(error);
    instance.exampleMessage.textContent =
      `Examples could not be loaded: ${reason}. Reload examples or use the empty editor.`;
    instance.exampleStatus.hidden = false;
  }
}

function playgroundEvaluation(instance) {
  saveActiveExampleFile(instance);
  if (!instance.activeExample) {
    return {
      modules: null,
      source: instance.ta.value,
      sourcePath: "<playground>"
    };
  }
  return createPlaygroundEvaluation(
    instance.activeExample,
    instance.exampleSources
  );
}

function registerEvaluationModules(session, evaluation) {
  if (!evaluation.modules) return;
  for (const [specifier, source] of evaluation.modules) {
    session.register_module(specifier, source);
  }
}

function syncClearOutputButton(instance, active = false) {
  if (!instance.clearOutBtn || !instance.output) return;
  const hasOutput = Boolean(instance.output.textContent.trim());
  instance.clearOutBtn.disabled = active || !hasOutput;
}

function refreshLines(instance) {
  const lines = instance.ta.value.split("\n").length || 1;
  const gutterWidth = Math.max(56, String(lines).length * 9 + 22);
  instance.editorArea?.style.setProperty("--gutter-width", `${gutterWidth}px`);
  const frag = document.createDocumentFragment();
  for (let i = 1; i <= lines; i++) {
    const div = document.createElement("div");
    div.className = "ln";
    div.textContent = i;
    frag.appendChild(div);
  }
  instance.gutter.innerHTML = "";
  instance.gutter.appendChild(frag);
  syncGutterScroll(instance);
}

function syncGutterScroll(instance) {
  if (!instance.gutter || !instance.ta?.element) return;
  instance.gutter.scrollTop = instance.ta.element.scrollTop;
}

const PLAYGROUND_EDITOR_MIN_HEIGHT = 240;
const PLAYGROUND_OUTPUT_MIN_HEIGHT = 160;
const PLAYGROUND_SPLIT_KEY_STEP = 24;

function playgroundSplitBounds(instance) {
  const splitterHeight = instance.splitter?.getBoundingClientRect().height || 0;
  const available = Math.max(
    0,
    (instance.playgroundMain?.clientHeight || 0) - splitterHeight
  );
  const min = Math.min(PLAYGROUND_EDITOR_MIN_HEIGHT, available);
  const max = Math.max(min, available - PLAYGROUND_OUTPUT_MIN_HEIGHT);
  return { available, min, max };
}

function setPlaygroundEditorHeight(instance, requestedHeight) {
  if (!instance.playgroundMain || !instance.splitter) return;
  if (getComputedStyle(instance.splitter).display === "none") return;

  const { available, min, max } = playgroundSplitBounds(instance);
  if (!available) return;
  const height = Math.max(min, Math.min(max, requestedHeight));
  const percent = Math.round((height / available) * 100);
  const minPercent = Math.round((min / available) * 100);
  const maxPercent = Math.round((max / available) * 100);

  instance.playgroundMain.style.setProperty(
    "--playground-editor-height",
    `${Math.round(height)}px`
  );
  instance.splitter.setAttribute("aria-valuemin", String(minPercent));
  instance.splitter.setAttribute("aria-valuemax", String(maxPercent));
  instance.splitter.setAttribute("aria-valuenow", String(percent));
  instance.splitter.setAttribute(
    "aria-valuetext",
    `Editor ${percent}%, output ${100 - percent}%`
  );
}

function syncPlaygroundSplitter(instance) {
  if (!instance.splitter || !instance.editor) return;
  if (getComputedStyle(instance.splitter).display === "none") return;
  setPlaygroundEditorHeight(
    instance,
    instance.editor.getBoundingClientRect().height
  );
}

function wirePlaygroundSplitter(instance) {
  const splitter = instance.splitter;
  if (!splitter || !instance.playgroundMain) return;

  let pointerId = null;
  let startY = 0;
  let startHeight = 0;

  const finishPointerResize = (event) => {
    if (event.pointerId !== pointerId) return;
    if (splitter.hasPointerCapture(pointerId)) {
      splitter.releasePointerCapture(pointerId);
    }
    pointerId = null;
    instance.playgroundMain.classList.remove("is-resizing");
  };

  splitter.addEventListener("pointerdown", (event) => {
    if (!event.isPrimary || event.button !== 0) return;
    pointerId = event.pointerId;
    startY = event.clientY;
    startHeight = instance.editor.getBoundingClientRect().height;
    splitter.setPointerCapture(pointerId);
    instance.playgroundMain.classList.add("is-resizing");
    event.preventDefault();
  });

  splitter.addEventListener("pointermove", (event) => {
    if (event.pointerId !== pointerId) return;
    setPlaygroundEditorHeight(
      instance,
      startHeight + event.clientY - startY
    );
  });

  splitter.addEventListener("pointerup", finishPointerResize);
  splitter.addEventListener("pointercancel", finishPointerResize);
  splitter.addEventListener("keydown", (event) => {
    const currentHeight = instance.editor.getBoundingClientRect().height;
    const { min, max } = playgroundSplitBounds(instance);
    let nextHeight = null;

    if (event.key === "ArrowUp") {
      nextHeight = currentHeight - PLAYGROUND_SPLIT_KEY_STEP;
    } else if (event.key === "ArrowDown") {
      nextHeight = currentHeight + PLAYGROUND_SPLIT_KEY_STEP;
    } else if (event.key === "Home") {
      nextHeight = min;
    } else if (event.key === "End") {
      nextHeight = max;
    }

    if (nextHeight === null) return;
    event.preventDefault();
    setPlaygroundEditorHeight(instance, nextHeight);
  });
}

function syncPanelHeights(instance) {
  if (!instance.root || !instance.playgroundMain) return;
  const height = instance.playgroundMain.getBoundingClientRect().height;
  if (height > 0) {
    instance.root.style.setProperty(
      "--playground-panel-height",
      `${Math.round(height)}px`
    );
  }
}

function requestPanelHeightSync(instance) {
  if (!instance.root) return;
  if (instance.panelHeightFrame) {
    cancelAnimationFrame(instance.panelHeightFrame);
  }
  instance.panelHeightFrame = requestAnimationFrame(() => {
    instance.panelHeightFrame = null;
    syncPanelHeights(instance);
  });
}

const STRUCTURE_REFRESH_DELAY_MS = 180;
const STRUCTURE_MODE_LABELS = {
  ast: "AST",
  cst: "CST",
};
const SYMBOL_KIND_LABELS = {
  assignment: "var",
  function: "fn",
  parameter: "arg",
  "implicit-parameter": "arg",
  "loop-counter": "loop",
};
const SYMBOL_PROVENANCE_LABELS = {
  global: "global",
  local: "local",
  parameter: "parameter",
  "implicit-parameter": "implicit",
  "loop-counter": "loop",
};

function spanForDef(def, occurrences) {
  if (Array.isArray(def.name_span)) return def.name_span;
  if (Array.isArray(def.span)) return def.span;
  return occurrences.find((occurrence) => occurrence.def === def.index)?.span;
}

function symbolSortKey(def) {
  const span = Array.isArray(def.name_span) ? def.name_span : def.span;
  return Array.isArray(span) ? span[0] : Number.MAX_SAFE_INTEGER;
}

function symbolDisplayName(def) {
  if (def.kind === "function" && Array.isArray(def.params)) {
    return `${def.name}[${def.params.join(";")}]`;
  }
  return def.name;
}

function provenanceLabel(def) {
  if (def.provenance === "local" && def.origin) {
    return `local in ${def.origin}`;
  }
  if (def.provenance === "parameter" && def.origin) {
    return `parameter of ${def.origin}`;
  }
  if (def.provenance === "implicit-parameter" && def.origin) {
    return `implicit of ${def.origin}`;
  }
  return SYMBOL_PROVENANCE_LABELS[def.provenance] || def.provenance || "";
}

function renderSymbolStatus(instance, message, isError = false) {
  if (!instance.symbolStatus) return;
  instance.symbolStatus.textContent = message || "";
  instance.symbolStatus.hidden = !message;
  instance.symbolStatus.classList.toggle("error", !!isError);
}

function formatSymbolError(error) {
  const kind = error?.kind ? `${error.kind}: ` : "";
  return `${kind}${error?.message || "parse error"}`;
}

function renderEmptySymbols(instance, message, isError = false) {
  if (instance.symbolCount) {
    instance.symbolCount.textContent = "0";
  }
  if (instance.symbolList) {
    instance.symbolList.innerHTML = "";
  }
  renderSymbolStatus(instance, message, isError);
}

function renderSymbolPanel(instance, data, code) {
  const defs = Array.isArray(data.defs) ? data.defs : [];
  const occurrences = Array.isArray(data.occurrences) ? data.occurrences : [];
  const errors = Array.isArray(data.errors) ? data.errors : [];
  const mapper = createSourceMapper(code);
  instance.symbolMapper = mapper;
  instance.symbolSource = code;

  if (instance.symbolCount) {
    instance.symbolCount.textContent = String(defs.length);
  }

  if (!defs.length) {
    const message = errors.length
      ? formatSymbolError(errors[0])
      : "No symbols yet.";
    renderEmptySymbols(instance, message, errors.length > 0);
    return;
  }

  if (errors.length) {
    renderSymbolStatus(instance, formatSymbolError(errors[0]), true);
  } else {
    renderSymbolStatus(instance, "");
  }

  const defsByIndex = new Map(defs.map((def) => [def.index, def]));
  const childrenByParent = new Map();
  for (const def of [...defs].sort(
    (a, b) => symbolSortKey(a) - symbolSortKey(b),
  )) {
    const parent = defsByIndex.has(def.parent) ? def.parent : null;
    if (!childrenByParent.has(parent)) {
      childrenByParent.set(parent, []);
    }
    childrenByParent.get(parent).push(def);
  }

  const chunks = [];
  const visited = new Set();
  function renderDef(def, depth) {
    if (visited.has(def.index)) return;
    visited.add(def.index);
    const span = spanForDef(def, occurrences);
    const loc = Array.isArray(span) ? mapper.lineCol(span[0]) : null;
    const meta = [];
    if (loc) meta.push(`${loc.line}:${loc.col}`);
    const provenance = provenanceLabel(def);
    if (provenance) meta.push(provenance);
    if (def.read_count) meta.push(`r${def.read_count}`);
    if (def.write_count) meta.push(`w${def.write_count}`);
    const metaHtml = meta.map((item) => `<span>${escapeHtml(item)}</span>`);
    if (def.ref_capture_count) {
      metaHtml.push(
        `<span class="symbol-ref">ref ${escapeHtml(def.ref_capture_count)}</span>`,
      );
    }

    const dataAttrs = Array.isArray(span)
      ? `data-symbol-start="${span[0]}" data-symbol-end="${span[1]}"`
      : "disabled";
    chunks.push(`
      <div class="symbol-item" style="--symbol-depth:${Math.min(depth, 6)}">
        <button class="symbol-link" type="button" ${dataAttrs}>
          <span class="symbol-kind">${escapeHtml(SYMBOL_KIND_LABELS[def.kind] || def.kind)}</span>
          <span class="symbol-name">${escapeHtml(symbolDisplayName(def))}</span>
        </button>
        <div class="symbol-meta">${metaHtml.join("")}</div>
      </div>
    `);

    for (const child of childrenByParent.get(def.index) || []) {
      renderDef(child, depth + 1);
    }
  }

  for (const def of childrenByParent.get(null) || []) {
    renderDef(def, 0);
  }
  for (const def of defs) {
    renderDef(def, 0);
  }

  if (instance.symbolList) {
    instance.symbolList.innerHTML = chunks.join("");
  }
}

function renderStructureStatus(instance, message, isError = false) {
  if (!instance.structureStatus) return;
  instance.structureStatus.textContent = message || "";
  instance.structureStatus.hidden = !message;
  instance.structureStatus.classList.toggle("error", !!isError);
}

function renderEmptyStructure(instance, message, isError = false) {
  if (instance.structureOutput) {
    instance.structureOutput.textContent = isError ? "" : message;
    instance.structureOutput.classList.toggle("empty", !isError);
  }
  renderStructureStatus(instance, isError ? message : "", isError);
}

function renderStructureOutput(instance, text) {
  if (!instance.structureOutput) return;
  instance.structureOutput.classList.remove("empty");
  instance.structureOutput.innerHTML = "";
  const renderer = createOutputRenderer(instance.structureOutput);
  renderer.appendOutput(text);
}

async function refreshStructure(instance) {
  const seq = (instance.structureRefreshSeq || 0) + 1;
  instance.structureRefreshSeq = seq;
  const code = instance.ta.value;
  const mode = instance.structureMode === "cst" ? "cst" : "ast";
  const modeLabel = STRUCTURE_MODE_LABELS[mode];

  if (!code.trim()) {
    renderEmptyStructure(instance, "No code yet.");
    return;
  }

  if (instance.structureOutput?.classList.contains("empty")) {
    instance.structureOutput.textContent = "";
    instance.structureOutput.classList.remove("empty");
  }
  renderStructureStatus(instance, "Parsing...");

  try {
    await ensureWasm();
    if (instance.structureRefreshSeq !== seq) return;

    const text = instance.frontend.get_wq_syntax_display(code, mode).trimEnd();

    if (instance.structureRefreshSeq !== seq) return;

    if (!text) {
      renderEmptyStructure(instance, `No ${modeLabel} output.`);
      return;
    }

    renderStructureOutput(instance, text);
    renderStructureStatus(instance, "");
  } catch (err) {
    if (instance.structureRefreshSeq !== seq) return;
    renderEmptyStructure(instance, formatWqError(err), true);
  }
}

function scheduleStructureRefresh(
  instance,
  delay = STRUCTURE_REFRESH_DELAY_MS,
) {
  if (instance.structureRefreshTimer) {
    clearTimeout(instance.structureRefreshTimer);
  }
  instance.structureRefreshTimer = window.setTimeout(() => {
    instance.structureRefreshTimer = null;
    refreshStructure(instance);
  }, delay);
}

function setStructureMode(instance, mode) {
  if (!Object.hasOwn(STRUCTURE_MODE_LABELS, mode)) return;
  if (instance.structureMode === mode) return;
  instance.structureMode = mode;
  let selectedButton = null;
  for (const button of instance.structureButtons || []) {
    const active = button.dataset.structureMode === mode;
    button.classList.toggle("active", active);
    button.setAttribute("aria-selected", active ? "true" : "false");
    if (active) selectedButton = button;
  }
  instance.structureTabs?.sync(selectedButton);
  scheduleStructureRefresh(instance, 0);
}

function jumpToSymbol(instance, span) {
  if (!Array.isArray(span) || !instance.symbolMapper) return;
  const [start, end] = instance.symbolMapper.unitRange(span);
  instance.ta.focus();
  instance.ta.setSelectionRange(start, end);
  const loc = instance.symbolMapper.lineCol(span[0]);
  const el = instance.ta.element;
  if (el) {
    el.scrollTop = Math.max(0, (loc.line - 1) * 22 - el.clientHeight / 2);
    syncGutterScroll(instance);
  }
}

async function doEval(instance) {
  instance.output.innerHTML = "";
  instance.inputHost.innerHTML = "";
  syncClearOutputButton(instance);

  // stdout/stderr
  const streamRenderer = createOutputRenderer(instance.output);

  try {
    const evaluation = playgroundEvaluation(instance);
    await ensureWasm();
    const flags = instance.debugFlagsInput?.value || "0";
    const start = performance.now();
    const result = await instance.evaluationController.run(
      ({ signal, setState }) =>
        queueEval(
          async () => {
            setState("running");
            const session = new WasmWqSession();
            try {
              session.set_stdout_callback((chunk) => {
                streamRenderer.appendStreamOutput(chunk);
                instance.output.scrollTop = instance.output.scrollHeight;
              });
              session.set_stderr_callback((chunk) => {
                streamRenderer.appendStreamOutput(chunk, "error");
                instance.output.scrollTop = instance.output.scrollHeight;
              });
              session.set_stdin_callback(async (prompt) => {
                setState("awaiting-input");
                try {
                  return await instance.stdinRequester.request(prompt, {
                    signal,
                  });
                } finally {
                  if (!signal.aborted) setState("running");
                }
              });
              applyBoxMode(session, instance);
              if (flags) {
                session.set_debug_flags(flags);
              }
              registerEvaluationModules(session, evaluation);
              return await session.eval_wq_async(evaluation.source, {
                signal,
                sourcePath: evaluation.sourcePath
              });
            } finally {
              session.free();
            }
          },
          { signal },
        ),
    );
    const end = performance.now();
    if (
      result.display !== undefined &&
      result.display !== null &&
      String(result.display).length
    ) {
      // ensure a newline before the bar if stdout left content on the same line
      // if (instance.output.textContent && !instance.output.textContent.endsWith("\n")) {
      //   instance.output.appendChild(document.createTextNode("\n"));
      // }
      const bar = document.createElement("span");
      bar.className = "repl-bar repl-bar-success";
      bar.textContent = "\u258d ";
      instance.output.appendChild(bar);
      const resultRenderer = createOutputRenderer(instance.output, bar);
      if (
        !appendResultPresentation(instance.output, result.presentation, {
          indent: "  ",
          trailingNewline: true,
        })
      ) {
        resultRenderer.appendOutput(
          alignTurnBody(String(result.display)) + "\n",
        );
      }
      if (readBoxFlags(instance).includes("xray") && result.xray) {
        const xrayBar = document.createElement("span");
        xrayBar.className = "repl-bar repl-bar-info";
        xrayBar.textContent = "\u258d ";
        instance.output.appendChild(xrayBar);
        const xrayRenderer = createOutputRenderer(instance.output, xrayBar);
        xrayRenderer.appendOutput(alignTurnBody(String(result.xray)) + "\n");
      }
      instance.output.scrollTop = instance.output.scrollHeight;
    }
    if (instance.timeMode) {
      const needsNL =
        instance.output.textContent &&
        !instance.output.textContent.endsWith("\n");
      streamRenderer.appendText(
        (needsNL ? "\n" : "") +
          alignTurnBody(`time elapsed: ${end - start}ms\n`),
      );
      instance.output.scrollTop = instance.output.scrollHeight;
    }
    requestPanelHeightSync(instance);
  } catch (err) {
    if (isAbortError(err)) {
      if (!instance.resetRequested) {
        streamRenderer.appendText("Interrupted\n");
      }
    } else {
      console.error(err);
      const bar = document.createElement("span");
      bar.className = "repl-bar repl-bar-error";
      bar.textContent = "\u258d ";
      instance.output.appendChild(bar);
      const errorRenderer = createOutputRenderer(instance.output, bar);
      errorRenderer.appendOutput(
        alignTurnBody(formatWqError(err, { rendered: true }) + "\n"),
        "error",
      );
    }
    requestPanelHeightSync(instance);
    instance.output.scrollTop = instance.output.scrollHeight;
  } finally {
    instance.resetRequested = false;
    syncClearOutputButton(instance);
    requestPanelHeightSync(instance);
  }
}

async function runForPoster(instance) {
  const stdoutDiv = document.createElement("div");
  const stdoutRenderer = createOutputRenderer(stdoutDiv);
  const resultDiv = document.createElement("div");
  const errorDiv = document.createElement("div");

  try {
    const evaluation = playgroundEvaluation(instance);
    instance.inputHost.innerHTML = "";
    await ensureWasm();
    const flags = instance.debugFlagsInput?.value || "0";
    const result = await instance.evaluationController.run(
      ({ signal, setState }) =>
        queueEval(
          async () => {
            setState("running");
            const session = new WasmWqSession();
            try {
              session.set_stdout_callback((chunk) =>
                stdoutRenderer.appendStreamOutput(chunk),
              );
              session.set_stderr_callback((chunk) =>
                stdoutRenderer.appendStreamOutput(chunk, "error"),
              );
              session.set_stdin_callback(async (prompt) => {
                setState("awaiting-input");
                try {
                  return await instance.stdinRequester.request(prompt, {
                    signal,
                  });
                } finally {
                  if (!signal.aborted) setState("running");
                }
              });
              applyBoxMode(session, instance);
              if (flags) session.set_debug_flags(flags);
              registerEvaluationModules(session, evaluation);
              return await session.eval_wq_async(evaluation.source, {
                signal,
                sourcePath: evaluation.sourcePath
              });
            } finally {
              session.free();
            }
          },
          { signal },
        ),
    );
    if (
      result.display !== undefined &&
      result.display !== null &&
      String(result.display).length
    ) {
      const bar = document.createElement("span");
      bar.className = "repl-bar repl-bar-success";
      bar.textContent = "\u258d ";
      resultDiv.appendChild(bar);
      const resultRenderer = createOutputRenderer(resultDiv, bar);
      if (
        !appendResultPresentation(resultDiv, result.presentation, {
          indent: "  ",
          trailingNewline: true,
        })
      ) {
        resultRenderer.appendOutput(
          alignTurnBody(String(result.display)) + "\n",
        );
      }
      if (readBoxFlags(instance).includes("xray") && result.xray) {
        const bar = document.createElement("span");
        bar.className = "repl-bar repl-bar-info";
        bar.textContent = "\u258d ";
        resultDiv.appendChild(bar);
        const resultRenderer = createOutputRenderer(resultDiv, bar);
        resultRenderer.appendOutput(
          alignTurnBody(String(result.xray)) + "\n",
        );
      }
    }
  } catch (err) {
    if (isAbortError(err)) {
      instance.resetRequested = false;
      return null;
    }
    const bar = document.createElement("span");
    bar.className = "repl-bar repl-bar-error";
    bar.textContent = "\u258d ";
    errorDiv.appendChild(bar);
    const errorRenderer = createOutputRenderer(errorDiv, bar);
    errorRenderer.appendOutput(
      alignTurnBody(formatWqError(err, { rendered: true }) + "\n"),
      "error",
    );
  }

  instance.resetRequested = false;
  return {
    stdoutHTML: stdoutDiv.innerHTML,
    resultHTML: resultDiv.innerHTML,
    errorHTML: errorDiv.innerHTML,
  };
}

function createPosterConfigModal() {
  return new Promise((resolve) => {
    const opener = document.activeElement;
    const dialog = document.createElement("dialog");
    dialog.className = "poster-dialog poster-config-dialog";
    dialog.setAttribute("aria-labelledby", "posterConfigHeading");
    dialog.innerHTML = `
      <div class="poster-config-modal">
        <h2 id="posterConfigHeading">Make Poster</h2>
        <div class="poster-field">
          <label for="posterTitle">Title</label>
          <input type="text" id="posterTitle" placeholder="Untitled" />
        </div>
        <div class="poster-field">
          <label for="posterDesc">Description</label>
          <textarea id="posterDesc" rows="3" placeholder="Optional description..."></textarea>
        </div>
        <div class="poster-field poster-field-inline">
          <input type="checkbox" id="posterRunCode" />
          <label for="posterRunCode">Run code and include output</label>
        </div>
        <div class="poster-modal-actions">
          <button class="btn" type="button" id="posterCancel">Cancel</button>
          <button class="btn primary" type="button" id="posterConfirm">Generate</button>
        </div>
      </div>
    `;

    const titleInput = dialog.querySelector("#posterTitle");
    const descInput = dialog.querySelector("#posterDesc");
    const runCheck = dialog.querySelector("#posterRunCode");
    const cancelBtn = dialog.querySelector("#posterCancel");
    const confirmBtn = dialog.querySelector("#posterConfirm");
    let result = null;

    cancelBtn.addEventListener("click", () => {
      dialog.close("cancel");
    });

    confirmBtn.addEventListener("click", () => {
      result = {
        title: titleInput.value.trim() || "Untitled",
        description: descInput.value.trim(),
        runCode: runCheck.checked
      };
      dialog.close("confirm");
    });

    dialog.addEventListener("keydown", (event) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      dialog.close("cancel");
    });

    dialog.addEventListener("close", () => {
      dialog.remove();
      if (opener?.isConnected) opener.focus();
      resolve(dialog.returnValue === "confirm" ? result : null);
    });

    titleInput.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        confirmBtn.click();
      }
    });

    document.body.appendChild(dialog);
    dialog.showModal();
    titleInput.focus();
  });
}

function showPosterModal(posterHTML, title = "poster") {
  const opener = document.activeElement;
  const dialog = document.createElement("dialog");
  dialog.className = "poster-dialog poster-show-dialog";
  dialog.setAttribute("aria-labelledby", "posterDisplayHeading");
  dialog.innerHTML = `
    <div class="poster-show-modal">
      <h2 class="visually-hidden" id="posterDisplayHeading">
        Generated poster: ${escapeHtml(title)}
      </h2>
      <div class="poster-card">
        ${posterHTML}
      </div>
      <div class="poster-modal-actions poster-show-actions">
        <button class="btn primary" type="button" id="posterClose">Close</button>
      </div>
    </div>
  `;

  dialog.addEventListener("close", () => {
    dialog.remove();
    if (opener?.isConnected) opener.focus();
  });
  const closeButton = dialog.querySelector("#posterClose");
  closeButton.addEventListener("click", () => {
    dialog.close();
  });
  dialog.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    dialog.close();
  });

  document.body.appendChild(dialog);
  dialog.showModal();
  closeButton.focus();
}

async function makePoster(instance) {
  await ensureWasm();
  const config = await createPosterConfigModal();
  if (!config) return;

  let runOutput = null;
  if (config.runCode) {
    runOutput = await runForPoster(instance);
    if (!runOutput) return;
  }

  const code = instance.ta.value;
  const highlightedCode = highlightedSourceHtml(
    document,
    instance.frontend,
    code,
  );

  let runSection = "";
  if (config.runCode && runOutput) {
    let outputBlock = "";
    if (runOutput.errorHTML) {
      outputBlock = `<div class="poster-run-result poster-run-error">${runOutput.errorHTML}</div>`;
    } else {
      if (runOutput.stdoutHTML) {
        outputBlock += `<div class="poster-run-stdout">${runOutput.stdoutHTML}</div>`;
      }
      if (runOutput.resultHTML) {
        outputBlock += `<div class="poster-run-result">${runOutput.resultHTML}</div>`;
      }
    }
    if (outputBlock) {
      runSection = `
        <div class="poster-run-section">
          <div class="poster-run-head">Output</div>
          ${outputBlock}
        </div>
      `;
    }
  }

  const hasRunSection = runSection.length > 0;
  const posterContent = `
    <div class="poster-header">
      <h2 class="poster-title">${escapeHtml(config.title)}</h2>
      ${config.description ? `<p class="poster-desc">${escapeHtml(config.description)}</p>` : ""}
    </div>
    <div class="poster-body">
      <div class="poster-code-wrapper${hasRunSection ? " attached" : ""}">
        <div class="poster-code-header"><span class="lang">wq</span></div>
        <pre class="poster-code-pre"><code class="language-wq">${highlightedCode}</code></pre>
      </div>
      ${runSection}
    </div>
    <div class="poster-footer">
      <img src="./wq_transparent_bg.png" alt="wq logo" class="poster-logo" />
      <span class="poster-url">wq-pl.com</span>
    </div>
  `;

  showPosterModal(posterContent, config.title);
}

export async function mountPlayground(root) {
  await ensureWasm();
  const frontend = getWqFrontend();
  const ta = createWqEditor(root.querySelector("textarea.editor-text"), {
    multilineMode: "plain",
    frontend,
  });
  const gutter = root.querySelector(".gutter");
  const output = root.querySelector(".run-output-body");
  const inputHost = root.querySelector("[data-stdin-host]");
  const clearOutBtn = root.querySelector("#clearOutBtn");
  const makePosterBtn = root.querySelector("#makePosterBtn");
  const runBtn = root.querySelector("#runBtn");
  const editor = root.querySelector(".editor");
  const editorArea = root.querySelector(".editor-area");
  const playgroundMain = root.querySelector(".playground-main");
  const splitter = root.querySelector(".playground-splitter");
  const debugFlagsInput = root.querySelector("#playgroundDebugFlags");
  const boxBtn = root.querySelector("#playgroundBoxBtn");
  const boxPanel = root.querySelector("#playgroundBoxPanel");
  const timeBtn = root.querySelector("#playgroundTimeBtn");
  const debugToggle = root.querySelector("#playgroundDebugToggle");
  const debugPanel = root.querySelector("#playgroundDebugPanel");
  const templateButtons = Array.from(root.querySelectorAll("[data-template]"));
  const exampleFiles = root.querySelector("[data-example-files]");
  const exampleStatus = root.querySelector("[data-example-status]");
  const exampleMessage = root.querySelector("[data-example-message]");
  const exampleRetry = root.querySelector("[data-example-retry]");
  const resetBtn = root.querySelector("#resetBtn");
  const symbolList = root.querySelector("[data-symbol-list]");
  const symbolCount = root.querySelector("[data-symbol-count]");
  const symbolStatus = root.querySelector("[data-symbol-status]");
  const structureOutput = root.querySelector("[data-structure-output]");
  const structureStatus = root.querySelector("[data-structure-status]");
  const structureButtons = Array.from(
    root.querySelectorAll("[data-structure-mode]"),
  );
  const structureTabsEl = root.querySelector(".structure-tabs");
  const instance = {
    root,
    frontend,
    ta,
    gutter,
    output,
    inputHost,
    clearOutBtn,
    makePosterBtn,
    runBtn,
    editor,
    editorArea,
    playgroundMain,
    splitter,
    debugFlagsInput,
    boxBtn,
    boxPanel,
    timeBtn,
    debugToggle,
    debugPanel,
    timeMode: false,
    stateSavingSession: null,
    examples: [],
    exampleSources: new Map(),
    activeExample: null,
    activeExamplePath: null,
    exampleFiles,
    exampleStatus,
    exampleMessage,
    exampleRetry,
    boxButtons: Object.fromEntries(
      BOX_FLAGS.map((flag) => [
        flag,
        root.querySelector(`[data-box-flag="${flag}"]`),
      ]),
    ),
    debugButtons: Object.fromEntries(
      DEBUG_FLAGS.map((flag) => [
        flag,
        root.querySelector(`[data-debug-flag="${flag}"]`),
      ]),
    ),
    templateButtons,
    resetBtn,
    symbolList,
    symbolCount,
    symbolStatus,
    symbolMapper: null,
    symbolSource: "",
    structureOutput,
    structureStatus,
    structureButtons,
    structureTabs: null,
    structureMode: "ast",
    structureRefreshSeq: 0,
    structureRefreshTimer: null,
    panelHeightFrame: null,
    panelResizeObserver: null,
    resetRequested: false,
  };
  instance.evaluationController = createEvaluationController((state) => {
    const active = state !== "idle";
    runBtn.textContent = active ? "Stop" : "Exec";
    runBtn.classList.toggle("primary", !active);
    runBtn.classList.toggle("danger", active);
    runBtn.disabled = state === "stopping";
    makePosterBtn.disabled = active;
    runBtn.dataset.evaluationState = state;
    syncClearOutputButton(instance, active);
  });
  syncClearOutputButton(instance);
  instance.stdinRequester = createStdinRequester({
    render: createDomStdinRenderer(inputHost),
  });
  wirePlaygroundSplitter(instance);
  instances.set(root, instance);
  instance.structureTabs = wireTabList(structureTabsEl, {
    onSelect(button) {
      setStructureMode(instance, button.dataset.structureMode);
    }
  });

  ta.addEventListener("input", () => {
    saveActiveExampleFile(instance);
    refreshLines(instance);
    if (!ta.value.trim()) {
      instance.symbolMapper = null;
      instance.symbolSource = ta.value;
      renderEmptySymbols(instance, "No symbols yet.");
    }
    scheduleStructureRefresh(instance);
    requestPanelHeightSync(instance);
  });
  ta.addEventListener("wq-symbol-analysis", (event) => {
    const { analysis, source } = event.detail || {};
    if (!analysis || source !== ta.value) return;
    renderSymbolPanel(instance, analysis, source);
  });
  ta.element?.addEventListener("scroll", () => {
    syncGutterScroll(instance);
  });
  runBtn?.addEventListener("click", async (e) => {
    e.preventDefault();
    if (instance.evaluationController.active) {
      instance.evaluationController.stop("stop requested");
      return;
    }
    await doEval(instance);
  });
  ta.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey || e.shiftKey) && e.key === "Enter") {
      e.preventDefault();
      if (!instance.evaluationController.active) doEval(instance);
    } else if (e.key === "Tab") {
      handleTabKey(e, ta, () => {
        refreshLines(instance);
      });
    }
  });
  clearOutBtn?.addEventListener("click", () => {
    instance.output.innerHTML = "";
    instance.inputHost.innerHTML = "";
    syncClearOutputButton(instance);
    requestPanelHeightSync(instance);
  });
  makePosterBtn?.addEventListener("click", async () => {
    await makePoster(instance);
  });
  symbolList?.addEventListener("click", (e) => {
    const button = e.target.closest("[data-symbol-start]");
    if (!button) return;
    jumpToSymbol(instance, [
      Number(button.dataset.symbolStart),
      Number(button.dataset.symbolEnd),
    ]);
  });
  boxBtn?.addEventListener("click", async () => {
    await ensureWasm();
    toggleRuntimePanel(boxBtn, boxPanel);
  });
  BOX_FLAGS.forEach((flag) => {
    instance.boxButtons[flag]?.addEventListener("click", () => {
      toggleBoxFlag(instance, flag);
      console.log(
        `[playground] box -> ${ensureStateSavingSession(instance).get_box_summary()}\n`,
      );
    });
  });
  timeBtn?.addEventListener("click", () => {
    instance.timeMode = !instance.timeMode;
    setActive(timeBtn, instance.timeMode);
    console.log(
      `[playground] time mode -> ${instance.timeMode ? "on" : "off"}\n`,
    );
  });
  DEBUG_FLAGS.forEach((flag) => {
    instance.debugButtons[flag]?.addEventListener("click", () => {
      toggleDebugFlag(instance, flag);
    });
  });
  debugToggle?.addEventListener("click", () => {
    toggleRuntimePanel(debugToggle, debugPanel);
  });
  document.addEventListener("click", (e) => {
    if (
      boxPanel?.classList.contains("open") &&
      !boxPanel.contains(e.target) &&
      !boxBtn?.contains(e.target)
    ) {
      closeRuntimePanel(boxBtn, boxPanel);
    }
    if (
      debugPanel?.classList.contains("open") &&
      !debugPanel.contains(e.target) &&
      !debugToggle?.contains(e.target)
    ) {
      closeRuntimePanel(debugToggle, debugPanel);
    }
  });
  window.addEventListener("resize", () => {
    positionRuntimePanel(boxBtn, boxPanel);
    positionRuntimePanel(debugToggle, debugPanel);
    syncPlaygroundSplitter(instance);
    requestPanelHeightSync(instance);
  });
  if (window.ResizeObserver && playgroundMain) {
    instance.panelResizeObserver = new ResizeObserver(() => {
      syncPlaygroundSplitter(instance);
      requestPanelHeightSync(instance);
    });
    instance.panelResizeObserver.observe(playgroundMain);
  }
  templateButtons.forEach((button) => {
    button.addEventListener("click", () => {
      const example = findPlaygroundExample(
        instance.examples,
        button.dataset.template
      );
      if (!example) return;
      selectPlaygroundExample(instance, example);
    });
  });
  exampleFiles?.addEventListener("click", (event) => {
    const button = event.target.closest("[data-example-file]");
    if (!button) return;
    selectExampleFile(instance, button.dataset.exampleFile);
  });
  exampleRetry?.addEventListener("click", () => {
    initializePlaygroundExamples(instance);
  });
  resetBtn?.addEventListener("click", () => {
    instance.resetRequested = instance.evaluationController.stop(
      "playground reset",
    );
    leaveExampleProject(instance);
    ta.value = "";
    instance.output.innerHTML = "";
    instance.inputHost.innerHTML = "";
    syncClearOutputButton(instance);
    refreshLines(instance);
    instance.timeMode = false;
    setActive(timeBtn, false);
    writeDebugFlags(instance, []);
    ensureStateSavingSession(instance).set_box_flags("box,axis,color");
    syncBoxControls(instance);
    renderEmptySymbols(instance, "No symbols yet.");
    scheduleStructureRefresh(instance);
    requestPanelHeightSync(instance);
    ta.focus();
  });
  root.addEventListener("wqide:deactivate", () => {
    instance.evaluationController.stop("view closed");
  });
  initializePlaygroundExamples(instance);
  refreshLines(instance);
  await ensureWasm();
  syncBoxControls(instance);
  setActive(timeBtn, instance.timeMode);
  writeDebugFlags(instance, []);
  await refreshStructure(instance);
  syncPlaygroundSplitter(instance);
  requestPanelHeightSync(instance);
}

export async function activatePlayground(root) {
  const instance = instances.get(root);
  if (!instance) return;
  await ensureWasm();
  syncBoxControls(instance);
  setActive(instance.timeBtn, instance.timeMode);
  scheduleStructureRefresh(instance, 0);
  requestPanelHeightSync(instance);
}

export function applyPlaygroundRoute(root, params) {
  const instance = instances.get(root);
  if (!instance) return;
  const code = params.get("code");

  if (code) {
    leaveExampleProject(instance);
    instance.ta.value = code;
    instance.ta.dispatchEvent(new Event("input", { bubbles: true }));
    refreshLines(instance);
    scheduleStructureRefresh(instance);
    requestPanelHeightSync(instance);
  }
}
