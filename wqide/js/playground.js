import {
  WasmWqSession,
  set_stdout_callback,
  set_stdin_callback,
  set_stderr_callback,
  highlight_wq,
  get_symbol_index_json,
} from "wq-wasm";
import { createAnsiRenderer } from "./ansi.js";
import { getPlaygroundExample } from "./playground-examples.js";
import {
  ensureWasm,
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
  queueEval,
  handleTabKey,
} from "./wq-shared.js";
import { createWqEditor } from "./editor.js";

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

function syncPanelHeights(instance) {
  if (!instance.root || !instance.editor) return;
  const height = instance.editor.getBoundingClientRect().height;
  if (height > 0) {
    instance.root.style.setProperty(
      "--playground-panel-height",
      `${Math.round(height)}px`,
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

const SYMBOL_REFRESH_DELAY_MS = 120;
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

function utf8ByteLength(ch) {
  const codePoint = ch.codePointAt(0) || 0;
  if (codePoint <= 0x7f) return 1;
  if (codePoint <= 0x7ff) return 2;
  if (codePoint <= 0xffff) return 3;
  return 4;
}

function createSourceMapper(src) {
  const points = [{ byte: 0, unit: 0 }];
  let byte = 0;
  let unit = 0;
  for (const ch of src) {
    byte += utf8ByteLength(ch);
    unit += ch.length;
    points.push({ byte, unit });
  }

  function unitAt(byteOffset) {
    const target = Math.max(0, Number(byteOffset) || 0);
    let lo = 0;
    let hi = points.length - 1;
    while (lo <= hi) {
      const mid = Math.floor((lo + hi) / 2);
      const point = points[mid];
      if (point.byte === target) return point.unit;
      if (point.byte < target) {
        lo = mid + 1;
      } else {
        hi = mid - 1;
      }
    }
    return points[Math.max(0, Math.min(hi, points.length - 1))].unit;
  }

  function lineCol(byteOffset) {
    const offset = unitAt(byteOffset);
    const before = src.slice(0, offset);
    const line = before.split("\n").length;
    const lastNewline = before.lastIndexOf("\n");
    const col = lastNewline === -1 ? offset + 1 : offset - lastNewline;
    return { line, col };
  }

  return {
    lineCol,
    unitRange(span) {
      return [unitAt(span[0]), unitAt(span[1])];
    },
  };
}

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
    const message = errors.length ? formatSymbolError(errors[0]) : "No symbols yet.";
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
  for (const def of [...defs].sort((a, b) => symbolSortKey(a) - symbolSortKey(b))) {
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

async function refreshSymbols(instance) {
  const seq = (instance.symbolRefreshSeq || 0) + 1;
  instance.symbolRefreshSeq = seq;
  const code = instance.ta.value;
  if (!code.trim()) {
    instance.symbolMapper = null;
    instance.symbolSource = code;
    renderEmptySymbols(instance, "No symbols yet.");
    return;
  }

  try {
    await ensureWasm();
    if (instance.symbolRefreshSeq !== seq) return;
    const data = JSON.parse(get_symbol_index_json(code));
    renderSymbolPanel(instance, data, code);
  } catch (err) {
    if (instance.symbolRefreshSeq !== seq) return;
    renderEmptySymbols(instance, err?.message ?? String(err), true);
    renderSymbolStatus(instance, err?.message ?? String(err), true);
  }
}

function scheduleSymbolRefresh(instance) {
  if (instance.symbolRefreshTimer) {
    clearTimeout(instance.symbolRefreshTimer);
  }
  instance.symbolRefreshTimer = window.setTimeout(() => {
    instance.symbolRefreshTimer = null;
    refreshSymbols(instance);
  }, SYMBOL_REFRESH_DELAY_MS);
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
  instance.runBtn.disabled = true;
  instance.output.innerHTML = "";

  // stdout/stderr — no bar
  const streamRenderer = createAnsiRenderer(instance.output);

  try {
    const code = instance.ta.value;
    const stdinArr = instance.stdinInput.value
      ? instance.stdinInput.value.replace(/\\n/g, "\n").split(/\r?\n/)
      : [];
    await ensureWasm();
    const flags = instance.debugFlagsInput?.value || "0";
    const start = performance.now();
    const result = await queueEval(() => {
      set_stdout_callback((chunk) => {
        streamRenderer.append(chunk);
        instance.output.scrollTop = instance.output.scrollHeight;
      });
      set_stderr_callback((chunk) => {
        streamRenderer.append("\x1b[31m" + chunk + "\x1b[0m");
        instance.output.scrollTop = instance.output.scrollHeight;
      });
      const queue = [...stdinArr];
      set_stdin_callback((_prompt) =>
        queue.length ? String(queue.shift()) : null,
      );
      const session = new WasmWqSession();
      try {
        applyBoxMode(session, instance);
        if (flags) {
          session.set_debug_flags(flags);
        }
        return session.eval_wq_result(code);
      } finally {
        session.free();
      }
    });
    const end = performance.now();
    if (
      result.value !== undefined &&
      result.value !== null &&
      String(result.value).length
    ) {
      // ensure a newline before the bar if stdout left content on the same line
      // if (instance.output.textContent && !instance.output.textContent.endsWith("\n")) {
      //   instance.output.appendChild(document.createTextNode("\n"));
      // }
      if (result.is_cas) {
        const bar = document.createElement("span");
        bar.className = "repl-bar repl-bar-success";
        bar.textContent = "\u258d ";
        instance.output.appendChild(bar);
        const casSpan = document.createElement("span");
        casSpan.innerHTML = highlight_wq(alignTurnBody(result.value));
        instance.output.appendChild(casSpan);
      } else {
        const bar = document.createElement("span");
        bar.className = "repl-bar repl-bar-success";
        bar.textContent = "\u258d ";
        instance.output.appendChild(bar);
        const resultRenderer = createAnsiRenderer(instance.output, bar);
        resultRenderer.append(alignTurnBody(String(result.value)) + "\n");
      }
      if (readBoxFlags(instance).includes("xray") && result.xray) {
        const xrayBar = document.createElement("span");
        xrayBar.className = "repl-bar repl-bar-info";
        xrayBar.textContent = "\u258d ";
        instance.output.appendChild(xrayBar);
        const xrayRenderer = createAnsiRenderer(instance.output, xrayBar);
        xrayRenderer.append(alignTurnBody(String(result.xray)) + "\n");
      }
      instance.output.scrollTop = instance.output.scrollHeight;
    }
    if (instance.timeMode) {
      const needsNL =
        instance.output.textContent &&
        !instance.output.textContent.endsWith("\n");
      streamRenderer.append(
        (needsNL ? "\n" : "") +
          alignTurnBody(`time elapsed: ${end - start}ms\n`),
      );
      instance.output.scrollTop = instance.output.scrollHeight;
    }
    requestPanelHeightSync(instance);
  } catch (err) {
    console.error(err);
    const bar = document.createElement("span");
    bar.className = "repl-bar repl-bar-error";
    bar.textContent = "\u258d ";
    instance.output.appendChild(bar);
    const errorRenderer = createAnsiRenderer(instance.output, bar);
    errorRenderer.append(alignTurnBody((err?.message ?? String(err)) + "\n"));
    requestPanelHeightSync(instance);
    instance.output.scrollTop = instance.output.scrollHeight;
  } finally {
    instance.runBtn.disabled = false;
    requestPanelHeightSync(instance);
  }
}

async function runForPoster(instance) {
  const stdoutDiv = document.createElement("div");
  const stdoutRenderer = createAnsiRenderer(stdoutDiv);
  const resultDiv = document.createElement("div");
  const errorDiv = document.createElement("div");

  try {
    const code = instance.ta.value;
    const stdinArr = instance.stdinInput.value
      ? instance.stdinInput.value.replace(/\\n/g, "\n").split(/\r?\n/)
      : [];
    await ensureWasm();
    set_stdout_callback((chunk) => stdoutRenderer.append(chunk));
    set_stderr_callback((chunk) =>
      stdoutRenderer.append("\x1b[31m" + chunk + "\x1b[0m"),
    );
    const queue = [...stdinArr];
    set_stdin_callback((_prompt) =>
      queue.length ? String(queue.shift()) : null,
    );
    const flags = instance.debugFlagsInput?.value || "0";
    const session = new WasmWqSession();
    try {
      applyBoxMode(session, instance);
      if (flags) session.set_debug_flags(flags);
      const result = session.eval_wq_result(code);
      if (
        result.value !== undefined &&
        result.value !== null &&
        String(result.value).length
      ) {
        if (result.is_cas) {
          const bar = document.createElement("span");
          bar.className = "repl-bar repl-bar-success";
          bar.textContent = "\u258d ";
          resultDiv.appendChild(bar);
          const casSpan = document.createElement("span");
          casSpan.innerHTML = highlight_wq(alignTurnBody(result.value));
          resultDiv.appendChild(casSpan);
        } else {
          const bar = document.createElement("span");
          bar.className = "repl-bar repl-bar-success";
          bar.textContent = "\u258d ";
          resultDiv.appendChild(bar);
          const resultRenderer = createAnsiRenderer(resultDiv, bar);
          resultRenderer.append(alignTurnBody(String(result.value)) + "\n");
        }
        if (readBoxFlags(instance).includes("xray") && result.xray) {
          const bar = document.createElement("span");
          bar.className = "repl-bar repl-bar-info";
          bar.textContent = "\u258d ";
          resultDiv.appendChild(bar);
          const resultRenderer = createAnsiRenderer(resultDiv, bar);
          resultRenderer.append(alignTurnBody(String(result.xray)) + "\n");
        }
      }
    } finally {
      session.free();
    }
  } catch (err) {
    const bar = document.createElement("span");
    bar.className = "repl-bar repl-bar-error";
    bar.textContent = "\u258d ";
    errorDiv.appendChild(bar);
    const errorRenderer = createAnsiRenderer(errorDiv, bar);
    errorRenderer.append(alignTurnBody((err?.message ?? String(err)) + "\n"));
  }

  return {
    stdoutHTML: stdoutDiv.innerHTML,
    resultHTML: resultDiv.innerHTML,
    errorHTML: errorDiv.innerHTML,
  };
}

function createPosterConfigModal() {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "poster-modal-overlay";
    overlay.innerHTML = `
      <div class="poster-config-modal">
        <h3>Make Poster</h3>
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

    const titleInput = overlay.querySelector("#posterTitle");
    const descInput = overlay.querySelector("#posterDesc");
    const runCheck = overlay.querySelector("#posterRunCode");
    const cancelBtn = overlay.querySelector("#posterCancel");
    const confirmBtn = overlay.querySelector("#posterConfirm");

    function close() {
      overlay.remove();
    }

    cancelBtn.addEventListener("click", () => {
      close();
      resolve(null);
    });

    confirmBtn.addEventListener("click", () => {
      const data = {
        title: titleInput.value.trim() || "Untitled",
        description: descInput.value.trim(),
        runCode: runCheck.checked,
      };
      close();
      resolve(data);
    });

    overlay.addEventListener("click", (e) => {
      if (e.target === overlay) {
        close();
        resolve(null);
      }
    });

    titleInput.addEventListener("keydown", (e) => {
      if (e.key === "Enter") confirmBtn.click();
    });

    document.body.appendChild(overlay);
    titleInput.focus();
  });
}

function showPosterModal(posterHTML, title = "poster") {
  const overlay = document.createElement("div");
  overlay.className = "poster-modal-overlay poster-show-overlay";
  overlay.innerHTML = `
    <div class="poster-show-modal">
      <div class="poster-card">
        ${posterHTML}
      </div>
      <div class="poster-modal-actions" style="justify-content:center;margin-top:0;">
        <button class="btn primary" type="button" id="posterClose">Close</button>
      </div>
    </div>
  `;

  const card = overlay.querySelector(".poster-card");

  function close() {
    overlay.remove();
  }
  overlay.addEventListener("click", (e) => {
    if (e.target === overlay) close();
  });
  document.addEventListener("keydown", function onKey(e) {
    if (e.key === "Escape") {
      close();
      document.removeEventListener("keydown", onKey);
    }
  });

  overlay.querySelector("#posterClose")?.addEventListener("click", close);

  document.body.appendChild(overlay);
}

async function makePoster(instance) {
  await ensureWasm();
  const config = await createPosterConfigModal();
  if (!config) return;

  let runOutput = null;
  if (config.runCode) {
    runOutput = await runForPoster(instance);
  }

  const code = instance.ta.value;
  const highlightedCode = highlight_wq(code);

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
  const ta = createWqEditor(root.querySelector("textarea.editor-text"), {
    multilineMode: "plain",
  });
  const gutter = root.querySelector(".gutter");
  const output = root.querySelector(".run-output-body");
  const stdinInput = root.querySelector("#stdin");
  const clearOutBtn = root.querySelector("#clearOutBtn");
  const makePosterBtn = root.querySelector("#makePosterBtn");
  const runBtn = root.querySelector("#runBtn");
  const editor = root.querySelector(".editor");
  const editorArea = root.querySelector(".editor-area");
  const debugFlagsInput = root.querySelector("#playgroundDebugFlags");
  const boxBtn = root.querySelector("#playgroundBoxBtn");
  const boxPanel = root.querySelector("#playgroundBoxPanel");
  const timeBtn = root.querySelector("#playgroundTimeBtn");
  const debugToggle = root.querySelector("#playgroundDebugToggle");
  const debugPanel = root.querySelector("#playgroundDebugPanel");
  const templateButtons = Array.from(root.querySelectorAll("[data-template]"));
  const resetBtn = root.querySelector("#resetBtn");
  const openInReplBtn = root.querySelector("#openInReplBtn");
  const symbolList = root.querySelector("[data-symbol-list]");
  const symbolCount = root.querySelector("[data-symbol-count]");
  const symbolStatus = root.querySelector("[data-symbol-status]");
  const instance = {
    root,
    ta,
    gutter,
    output,
    stdinInput,
    clearOutBtn,
    runBtn,
    editor,
    editorArea,
    debugFlagsInput,
    boxBtn,
    boxPanel,
    timeBtn,
    debugToggle,
    debugPanel,
    timeMode: false,
    stateSavingSession: null,
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
    openInReplBtn,
    symbolList,
    symbolCount,
    symbolStatus,
    symbolMapper: null,
    symbolSource: "",
    symbolRefreshSeq: 0,
    symbolRefreshTimer: null,
    panelHeightFrame: null,
    panelResizeObserver: null,
  };
  instances.set(root, instance);

  await ensureWasm();

  ta.addEventListener("input", () => {
    refreshLines(instance);
    scheduleSymbolRefresh(instance);
    requestPanelHeightSync(instance);
  });
  ta.element?.addEventListener("scroll", () => {
    syncGutterScroll(instance);
  });
  runBtn?.addEventListener("click", async (e) => {
    e.preventDefault();
    await doEval(instance);
  });
  ta.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey || e.shiftKey) && e.key === "Enter") {
      e.preventDefault();
      doEval(instance);
    } else if (e.key === "Tab") {
      handleTabKey(e, ta, () => {
        refreshLines(instance);
      });
    }
  });
  clearOutBtn?.addEventListener("click", () => {
    instance.output.innerHTML = "";
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
    requestPanelHeightSync(instance);
  });
  if (window.ResizeObserver && editor) {
    instance.panelResizeObserver = new ResizeObserver(() => {
      requestPanelHeightSync(instance);
    });
    instance.panelResizeObserver.observe(editor);
  }
  templateButtons.forEach((button) => {
    button.addEventListener("click", () => {
      const example = getPlaygroundExample(button.dataset.template);
      if (!example) return;
      ta.value = example.code;
      stdinInput.value = example.stdin;
      refreshLines(instance);
      scheduleSymbolRefresh(instance);
      requestPanelHeightSync(instance);
      ta.focus();
      ta.setSelectionRange(ta.value.length, ta.value.length);
    });
  });
  resetBtn?.addEventListener("click", () => {
    ta.value = "";
    stdinInput.value = "";
    instance.output.innerHTML = "";
    refreshLines(instance);
    instance.timeMode = false;
    setActive(timeBtn, false);
    writeDebugFlags(instance, []);
    ensureStateSavingSession(instance).set_box_flags("box,axis,color");
    syncBoxControls(instance);
    scheduleSymbolRefresh(instance);
    requestPanelHeightSync(instance);
    ta.focus();
  });
  openInReplBtn?.addEventListener("click", () => {
    const code = ta.value.trim();
    if (!code) return;
    window.navigate(`repl.html?input=${encodeURIComponent(code)}`);
  });

  refreshLines(instance);
  await ensureWasm();
  syncBoxControls(instance);
  setActive(timeBtn, instance.timeMode);
  writeDebugFlags(instance, []);
  await refreshSymbols(instance);
  requestPanelHeightSync(instance);
}

export async function activatePlayground(root) {
  const instance = instances.get(root);
  if (!instance) return;
  await ensureWasm();
  syncBoxControls(instance);
  setActive(instance.timeBtn, instance.timeMode);
  requestPanelHeightSync(instance);
}

export function applyPlaygroundRoute(root, params) {
  const instance = instances.get(root);
  if (!instance) return;
  const code = params.get("code");
  const sin = params.get("stdin");

  if (code) {
    instance.ta.value = code;
    instance.ta.dispatchEvent(new Event("input", { bubbles: true }));
    refreshLines(instance);
    scheduleSymbolRefresh(instance);
    requestPanelHeightSync(instance);
    if (sin) {
      instance.stdinInput.value = sin;
    }
  }
}
