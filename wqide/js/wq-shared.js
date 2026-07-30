import init, {
  doc_index,
  get_doc_markdown,
  get_wq_ver,
  WasmFrontend,
} from "wq-wasm";
import { nextPopupIndex, nextTabIndex } from "./ui-navigation.js";
import { wireSegmentedControl } from "./ui-segmented.js";
export { queueEval } from "./eval-lifecycle.js";
export { nextPopupIndex, nextTabIndex } from "./ui-navigation.js";
export { wireSegmentedControl } from "./ui-segmented.js";

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
  if (el.classList.contains("runtime-segment")) {
    el.setAttribute("aria-pressed", String(!!on));
  }
}

export function wireTabList(tablist, { onSelect } = {}) {
  if (!tablist) return null;
  let segmentedControl = null;

  function tabs() {
    return Array.from(tablist.querySelectorAll('[role="tab"]'));
  }

  function sync(selectedTab) {
    const items = tabs();
    const selected =
      selectedTab ||
      items.find((tab) => tab.getAttribute("aria-selected") === "true") ||
      items[0];
    for (const tab of items) {
      tab.tabIndex = tab === selected ? 0 : -1;
    }
    segmentedControl?.sync(selected);
  }

  function activate(tab, focus = false) {
    if (!tab) return;
    onSelect?.(tab);
    sync(tab);
    if (focus) tab.focus();
  }

  segmentedControl = wireSegmentedControl(tablist, {
    optionSelector: '[role="tab"]',
    isSelected: (tab) => tab.getAttribute("aria-selected") === "true",
    onSelect(tab) {
      activate(tab);
    }
  });

  tablist.addEventListener("keydown", (event) => {
    const tab = event.target.closest('[role="tab"]');
    if (!tab || !tablist.contains(tab)) return;
    const items = tabs();
    const nextIndex = nextTabIndex(event.key, items.indexOf(tab), items.length);
    if (nextIndex === null) return;
    event.preventDefault();
    activate(items[nextIndex], true);
  });

  sync();
  return { sync };
}

export function wirePopupSelection({
  trigger,
  popup,
  optionSelector,
  onOpen,
  onClose,
  onSelect
}) {
  if (!trigger || !popup) return null;

  function options() {
    return Array.from(popup.querySelectorAll(optionSelector));
  }

  function selectedOption() {
    const items = options();
    return (
      items.find(
        (option) =>
          option.getAttribute("aria-selected") === "true" ||
          option.getAttribute("aria-checked") === "true" ||
          option.classList.contains("active")
      ) || items[0]
    );
  }

  function syncOptions() {
    for (const option of options()) {
      option.tabIndex = -1;
    }
  }

  function open(focus = "selected") {
    onOpen?.();
    syncOptions();
    const items = options();
    const target =
      focus === "first"
        ? items[0]
        : focus === "last"
          ? items[items.length - 1]
          : selectedOption();
    target?.focus();
  }

  function close({ restoreFocus = false } = {}) {
    onClose?.();
    if (restoreFocus) trigger.focus();
  }

  trigger.addEventListener("click", () => {
    if (popup.classList.contains("open")) {
      close();
    } else {
      open();
    }
  });

  trigger.addEventListener("keydown", (event) => {
    const openKey =
      event.key === "Enter" ||
      event.key === " " ||
      event.key === "Spacebar" ||
      event.key === "ArrowDown" ||
      event.key === "ArrowUp";
    if (!openKey) return;
    event.preventDefault();
    open(event.key === "ArrowUp" ? "last" : "selected");
  });

  popup.addEventListener("click", (event) => {
    const option = event.target.closest(optionSelector);
    if (!option || !popup.contains(option)) return;
    onSelect?.(option);
    close({ restoreFocus: true });
  });

  popup.addEventListener("keydown", (event) => {
    const option = event.target.closest(optionSelector);
    if (!option || !popup.contains(option)) return;
    if (event.key === "Escape") {
      event.preventDefault();
      close({ restoreFocus: true });
      return;
    }
    if (event.key === "Tab") {
      close();
      return;
    }
    if (
      event.key === "Enter" ||
      event.key === " " ||
      event.key === "Spacebar"
    ) {
      event.preventDefault();
      onSelect?.(option);
      close({ restoreFocus: true });
      return;
    }
    const items = options();
    const nextIndex = nextPopupIndex(
      event.key,
      items.indexOf(option),
      items.length
    );
    if (nextIndex === null) return;
    event.preventDefault();
    items[nextIndex].focus();
  });

  syncOptions();
  return { close, open };
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
