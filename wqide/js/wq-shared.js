import init, { get_doc_index_json, get_doc_markdown, get_wq_ver } from "wq-wasm";

// ========== WASM Initialization ==========

let wasmInitPromise = null;
let wqVersion = "";

export async function ensureWasm() {
  if (!wasmInitPromise) {
    wasmInitPromise = init().then(() => {
      wqVersion = get_wq_ver();
    });
  }
  return wasmInitPromise;
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
  return JSON.parse(get_doc_index_json());
}

// ========== Debug Flags ==========

export const DEBUG_FLAGS = ["inst", "ast", "token"];

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
