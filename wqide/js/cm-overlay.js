import { handleTabKey } from "./wq-shared.js";

/**
 * Lightweight overlay code editor.
 * A transparent textarea sits on top of a <pre> highlight layer.
 * Uses native caret/selection; highlight is purely visual.
 * The editor grows vertically with content (no internal scrolling).
 */

export function createCmOverlay(root, options = {}) {
  const pre = root.querySelector(".cm-content");
  const code = root.querySelector(".cm-content code");
  const input = root.querySelector(".cm-input");

  if (!pre || !code || !input) {
    throw new Error("cm-overlay: missing .cm-content, code, or .cm-input");
  }

  const highlightFn = options.highlight || ((v) => v);
  const onInput = options.onInput || (() => {});

  function updateHighlight() {
    const html = highlightFn(input.value);
    code.innerHTML = html;
  }

  function autoResize() {
    input.style.height = "auto";
    const h = Math.max(input.scrollHeight, pre.offsetHeight);
    input.style.height = h + "px";
  }

  function onInputEvent() {
    updateHighlight();
    pre.getBoundingClientRect(); // force reflow so pre.offsetHeight is up-to-date
    autoResize();
    onInput();
  }

  function onKeyDown(e) {
    if (e.key === "Tab") {
      e.preventDefault();
      handleTabKey(e, input, onInputEvent);
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      if (options.onExec) options.onExec();
      return;
    }
  }

  input.addEventListener("input", onInputEvent);
  input.addEventListener("keydown", onKeyDown);

  // Ensure both layers have identical metrics by copying textarea styles to pre
  function syncMetrics() {
    const cs = getComputedStyle(input);
    pre.style.padding = cs.padding;
    pre.style.fontSize = cs.fontSize;
    pre.style.lineHeight = cs.lineHeight;
    pre.style.fontFamily = cs.fontFamily;
    pre.style.whiteSpace = cs.whiteSpace;
    pre.style.wordBreak = cs.wordBreak;
  }

  // Initial sync
  syncMetrics();
  updateHighlight();
  autoResize();

  return {
    get value() {
      return input.value;
    },
    set value(v) {
      input.value = v;
      updateHighlight();
      autoResize();
    },
    get el() {
      return input;
    },
    focus() {
      input.focus();
    },
    setSelectionRange(start, end) {
      input.setSelectionRange(start, end);
    },
    update() {
      updateHighlight();
      autoResize();
    },
    resize() {
      autoResize();
    },
    destroy() {
      input.removeEventListener("input", onInputEvent);
      input.removeEventListener("keydown", onKeyDown);
    },
  };
}
