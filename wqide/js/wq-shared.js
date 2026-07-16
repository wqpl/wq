import init, {
  doc_index,
  get_doc_markdown,
  get_wq_ver,
  WasmFrontend,
} from "wq-wasm";

// ========== WASM Initialization ==========

let wasmInitPromise = null;
let wqVersion = "";
let defaultFrontend = null;

export async function ensureWasm() {
  if (!wasmInitPromise) {
    wasmInitPromise = init().then(() => {
      wqVersion = get_wq_ver();
      defaultFrontend = new WasmFrontend();
    });
  }
  return wasmInitPromise;
}

export function getWqFrontend() {
  if (!defaultFrontend) {
    throw new Error("wq WASM must be initialized before using its frontend");
  }
  return defaultFrontend;
}

export function createWqFrontend() {
  if (!defaultFrontend) {
    throw new Error("wq WASM must be initialized before creating a frontend");
  }
  return new WasmFrontend();
}

export function getWqVersion() {
  return wqVersion;
}

export async function getDocMarkdown(query) {
  await ensureWasm();
  return get_doc_markdown(query);
}

export async function getDocIndex() {
  await ensureWasm();
  return Array.from(doc_index());
}

export function formatWqError(error, { rendered = false } = {}) {
  const detail = rendered ? error?.rendered : error?.message;
  return String(detail ?? error?.rendered ?? error?.message ?? error);
}

// ========== Debug Flags ==========

export const DEBUG_FLAGS = [
  "token",
  "cst",
  "ast",
  "ast-v",
  "inst",
  "inst-v",
  "wqdb",
  "wqdb-v",
  "value",
  "cas",
  "cas-v",
];

export const DEBUG_ALIASES = [
  ["0", "off"],
  ["1", "inst"],
  ["2", "inst,ast"],
  ["3", "inst,ast,value"],
  ["4", "inst,ast,value,inst-v,ast-v"],
];

const DEBUG_BASE_FOR_VERBOSE = {
  "ast-v": "ast",
  "inst-v": "inst",
  "wqdb-v": "wqdb",
  "cas-v": "cas",
};

const DEBUG_VERBOSE_FOR_BASE = Object.fromEntries(
  Object.entries(DEBUG_BASE_FOR_VERBOSE).map(([verbose, base]) => [
    base,
    verbose,
  ]),
);

export function parseDebugFlags(spec) {
  if (!spec || spec === "off" || spec === "0") return [];
  return spec
    .split(",")
    .map((flag) => flag.trim())
    .filter(Boolean);
}

export function formatDebugFlags(flags) {
  const next = DEBUG_FLAGS.filter((flag) => flags.includes(flag));
  return next.length ? next.join(",") : "0";
}

export function toggleDebugFlagList(flags, flag) {
  const next = new Set(flags);
  if (next.has(flag)) {
    next.delete(flag);
    const base = DEBUG_BASE_FOR_VERBOSE[flag];
    if (base) next.delete(base);
    const verbose = DEBUG_VERBOSE_FOR_BASE[flag];
    if (verbose) next.delete(verbose);
  } else {
    next.add(flag);
    const base = DEBUG_BASE_FOR_VERBOSE[flag];
    if (base) next.add(base);
  }
  return DEBUG_FLAGS.filter((item) => next.has(item));
}

// ========== UI Helpers ==========

export function setActive(el, on) {
  if (!el) return;
  el.classList.toggle("active", !!on);
  el.classList.toggle("inactive", !on);
}

export function syncDebugButtons(buttonsMap, activeFlags) {
  DEBUG_FLAGS.forEach((flag) => {
    setActive(buttonsMap?.[flag], activeFlags.includes(flag));
  });
}

// ========== Box Flags ==========

export const BOX_FLAGS = ["box", "axis", "color", "xray"];

export function parseBoxFlags(spec) {
  if (!spec || spec === "off" || spec === "0") return [];
  return spec
    .split(",")
    .map((flag) => flag.trim())
    .filter(Boolean);
}

export function formatBoxFlags(flags) {
  const next = BOX_FLAGS.filter((flag) => flags.includes(flag));
  return next.length ? next.join(",") : "0";
}

export function syncBoxButtons(buttonsMap, activeFlags) {
  BOX_FLAGS.forEach((flag) => {
    setActive(buttonsMap?.[flag], activeFlags.includes(flag));
  });
}

// ========== Runtime Panels ==========

const PANEL_MARGIN = 8;

export function positionRuntimePanel(button, panel) {
  if (!button || !panel?.classList.contains("open")) return;
  panel.style.setProperty("--runtime-panel-shift", "0px");
  panel.style.removeProperty("--runtime-panel-max-h");
  requestAnimationFrame(() => {
    if (!panel.classList.contains("open")) return;
    const rect = panel.getBoundingClientRect();
    let shift = 0;
    if (rect.left < PANEL_MARGIN) {
      shift = PANEL_MARGIN - rect.left;
    } else if (rect.right > window.innerWidth - PANEL_MARGIN) {
      shift = window.innerWidth - PANEL_MARGIN - rect.right;
    }
    panel.style.setProperty("--runtime-panel-shift", `${shift}px`);

    const shiftedBottom = rect.bottom;
    if (shiftedBottom > window.innerHeight - PANEL_MARGIN) {
      const maxHeight = Math.max(
        80,
        window.innerHeight - rect.top - PANEL_MARGIN,
      );
      panel.style.setProperty("--runtime-panel-max-h", `${maxHeight}px`);
    }
  });
}

export function closeRuntimePanel(button, panel) {
  panel?.classList.remove("open");
  panel?.style.removeProperty("--runtime-panel-shift");
  panel?.style.removeProperty("--runtime-panel-max-h");
  button?.setAttribute("aria-expanded", "false");
}

export function openRuntimePanel(button, panel) {
  if (!button || !panel) return;
  panel.classList.add("open");
  button.setAttribute("aria-expanded", "true");
  positionRuntimePanel(button, panel);
}

export function toggleRuntimePanel(button, panel) {
  if (!button || !panel) return;
  if (panel.classList.contains("open")) {
    closeRuntimePanel(button, panel);
  } else {
    openRuntimePanel(button, panel);
  }
}

// ========== Text Helpers ==========

export function alignTurnBody(text) {
  return String(text).replace(/\n(?![\n\r]*$)/g, "\n  ");
}

export function escapeHtml(str) {
  return String(str)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

// ========== Editor Helpers ==========

export function insertTextAtCursor(textarea, text) {
  const start = textarea.selectionStart;
  const end = textarea.selectionEnd;
  const value = textarea.value;
  textarea.value = value.slice(0, start) + text + value.slice(end);
  textarea.selectionStart = textarea.selectionEnd = start + text.length;
}

export function handleTabKey(e, textarea, onUpdate) {
  const ta = textarea;
  const start = ta.selectionStart;
  const end = ta.selectionEnd;
  const value = ta.value;

  if (start === end || value.slice(start, end).indexOf("\n") === -1) {
    e.preventDefault();
    if (e.shiftKey) {
      const lineStart = value.lastIndexOf("\n", start - 1) + 1;
      const beforeCursor = value.slice(lineStart, start);
      const toRemove = beforeCursor.match(/^( {1,2})/)?.[1]?.length || 0;
      if (toRemove > 0) {
        ta.value = value.slice(0, start - toRemove) + value.slice(end);
        ta.selectionStart = ta.selectionEnd = start - toRemove;
      }
    } else {
      insertTextAtCursor(ta, "  ");
    }
    if (onUpdate) onUpdate();
    return;
  }

  e.preventDefault();
  const selStart = start;
  const selEnd = end;
  const lineStart = value.lastIndexOf("\n", selStart - 1) + 1;
  const block = value.slice(lineStart, selEnd);
  const lines = block.split("\n");

  if (e.shiftKey) {
    const dedented = lines
      .map((line) => line.replace(/^ {1,2}/, ""))
      .join("\n");
    ta.value = value.slice(0, lineStart) + dedented + value.slice(selEnd);
    const removedTotal = block.length - dedented.length;
    ta.selectionStart = selStart - Math.min(removedTotal, selStart - lineStart);
    ta.selectionEnd = selEnd - removedTotal;
  } else {
    const indented = lines.map((line) => "  " + line).join("\n");
    ta.value = value.slice(0, lineStart) + indented + value.slice(selEnd);
    ta.selectionStart = selStart + 2;
    ta.selectionEnd = selEnd + lines.length * 2;
  }
  if (onUpdate) onUpdate();
}

// ========== Output Helpers ==========

export function createOutputBar(type, documentRef = document) {
  const bar = documentRef.createElement("span");
  bar.className = `repl-bar repl-bar-${type}`;
  bar.textContent = "\u258d ";
  return bar;
}

// ========== Eval Queue ==========

let evalQueue = Promise.resolve();

export function queueEval(taskFn) {
  const p = evalQueue.then(taskFn);
  evalQueue = p.catch(() => {});
  return p;
}

// ========== Copy Helpers ==========

export async function fallbackCopyText(text) {
  if (navigator.clipboard && window.isSecureContext) {
    return navigator.clipboard.writeText(text);
  }
  const textArea = document.createElement("textarea");
  textArea.value = text;
  textArea.style.position = "fixed";
  textArea.style.opacity = "0";
  document.body.appendChild(textArea);
  textArea.focus();
  textArea.select();
  const success = document.execCommand("copy");
  textArea.remove();
  if (!success) throw new Error("Fallback copy failed");
}
