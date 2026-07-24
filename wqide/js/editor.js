import {
  activeBindingHighlights,
  createSourceMapper,
} from "./symbol-highlights.js";

function escapeEditorText(text) {
  return String(text)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function renderHighlightedText(frontend, text) {
  if (!frontend) return escapeEditorText(text);
  try {
    return frontend.highlight_wq(text);
  } catch (err) {
    console.warn("[wqide] highlight failed; falling back to plain text", err);
    return escapeEditorText(text);
  }
}

function clampToLength(length, offset) {
  return Math.max(0, Math.min(length, Number(offset) || 0));
}

function clampOffset(value, offset) {
  return clampToLength(value.length, offset);
}

function normalizeEditorText(text) {
  return String(text).replace(/\r\n?/g, "\n");
}

export function isImeCompositionKey(event, compositionActive = false) {
  return compositionActive || event.isComposing || event.keyCode === 229;
}

function ownsSelection(el) {
  const sel = window.getSelection();
  if (!sel || sel.rangeCount === 0) return false;
  const range = sel.getRangeAt(0);
  return el.contains(range.startContainer) && el.contains(range.endContainer);
}

function selectionOffsets(el, valueOrLength) {
  if (!ownsSelection(el)) {
    return null;
  }

  const maxLength =
    typeof valueOrLength === "number" ? valueOrLength : valueOrLength.length;
  const sel = window.getSelection();
  const range = sel.getRangeAt(0);
  const beforeStart = document.createRange();
  beforeStart.selectNodeContents(el);
  beforeStart.setEnd(range.startContainer, range.startOffset);

  const beforeEnd = document.createRange();
  beforeEnd.selectNodeContents(el);
  beforeEnd.setEnd(range.endContainer, range.endOffset);

  return {
    start: clampToLength(maxLength, beforeStart.toString().length),
    end: clampToLength(maxLength, beforeEnd.toString().length),
  };
}

function pointForOffset(el, offset, { forwardAtBoundary = false } = {}) {
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
  let node = walker.nextNode();
  let remaining = offset;
  let lastTextNode = null;

  while (node) {
    const len = node.nodeValue.length;
    if (remaining < len || (remaining === len && !forwardAtBoundary)) {
      return { node, offset: remaining };
    }
    remaining -= len;
    lastTextNode = node;
    node = walker.nextNode();
  }

  if (lastTextNode) {
    return {
      node: lastTextNode,
      offset: lastTextNode.nodeValue.length,
    };
  }

  return { node: el, offset: 0 };
}

function setDomSelection(el, value, start, end) {
  const safeStart = clampOffset(value, start);
  const safeEnd = clampOffset(value, end);
  const range = document.createRange();
  const from = pointForOffset(el, safeStart);
  const to = pointForOffset(el, safeEnd);
  range.setStart(from.node, from.offset);
  range.setEnd(to.node, to.offset);

  const sel = window.getSelection();
  sel.removeAllRanges();
  sel.addRange(range);
}

function dispatchEditorInput(el) {
  el.dispatchEvent(new InputEvent("input", { bubbles: true }));
}

export function createWqEditor(textarea, options = {}) {
  const el = document.createElement("div");
  const frontend = options.frontend || null;
  const multilineMode = options.multilineMode || "plain";
  const singleLineMode = multilineMode === "none";
  const normalizeValue = (text) => {
    const normalized = normalizeEditorText(text);
    return singleLineMode ? normalized.replace(/\n+/g, " ") : normalized;
  };
  const initialValue = normalizeValue(textarea.value || "");
  let value = initialValue;
  let selection = {
    start: value.length,
    end: value.length,
  };
  let composing = false;
  let compositionCommitFrame = null;
  let symbolAnalysis = null;
  let symbolAnalysisSource = null;
  let symbolAnalysisTimer = null;

  el.className = textarea.className;
  el.classList.add("wq-editor");
  el.id = textarea.id;
  el.setAttribute("role", "textbox");
  el.setAttribute("aria-multiline", singleLineMode ? "false" : "true");
  el.setAttribute("contenteditable", "true");
  el.setAttribute("spellcheck", textarea.getAttribute("spellcheck") || "false");
  el.setAttribute("autocapitalize", "off");
  el.setAttribute("autocomplete", "off");
  el.setAttribute("autocorrect", "off");
  el.tabIndex = textarea.tabIndex || 0;
  for (const attr of ["aria-label", "aria-labelledby", "enterkeyhint"]) {
    const value = textarea.getAttribute(attr);
    if (value) el.setAttribute(attr, value);
  }
  if (textarea.placeholder) {
    el.dataset.placeholder = textarea.placeholder;
  }

  textarea.replaceWith(el);

  function removeSymbolOverlays() {
    for (const overlay of el.querySelectorAll(".wq-symbol-occurrence")) {
      overlay.replaceWith(...overlay.childNodes);
    }
    el.normalize();
  }

  function addSymbolOverlays(caretUnit) {
    if (symbolAnalysisSource !== value || !symbolAnalysis) return;
    const mapper = createSourceMapper(value);
    const highlights = activeBindingHighlights(
      symbolAnalysis,
      mapper.byteAtUnit(caretUnit),
    )
      .map((highlight) => ({
        ...highlight,
        unitSpan: mapper.unitRange(highlight.span),
      }))
      .sort((a, b) => b.unitSpan[0] - a.unitSpan[0]);

    for (const highlight of highlights) {
      const [start, end] = highlight.unitSpan;
      if (start >= end) continue;
      const range = document.createRange();
      const from = pointForOffset(el, start, { forwardAtBoundary: true });
      const to = pointForOffset(el, end);
      range.setStart(from.node, from.offset);
      range.setEnd(to.node, to.offset);
      const overlay = document.createElement("span");
      overlay.className = [
        "wq-symbol-occurrence",
        `wq-symbol-occurrence-${highlight.role}`,
        highlight.current ? "wq-symbol-occurrence-current" : "",
      ]
        .filter(Boolean)
        .join(" ");
      try {
        range.surroundContents(overlay);
      } catch (error) {
        console.warn("[wqide] could not render a symbol occurrence", error);
      }
    }
  }

  function refreshSymbolOverlays() {
    if (composing || compositionCommitFrame !== null) return;
    const nextSelection = selectionOffsets(el, value) || selection;
    removeSymbolOverlays();
    addSymbolOverlays(nextSelection.start);
    selection = {
      start: clampOffset(value, nextSelection.start),
      end: clampOffset(value, nextSelection.end),
    };
    if (document.activeElement === el || ownsSelection(el)) {
      setDomSelection(el, value, selection.start, selection.end);
    }
  }

  function scheduleSymbolAnalysis() {
    if (symbolAnalysisTimer !== null) {
      window.clearTimeout(symbolAnalysisTimer);
    }
    symbolAnalysis = null;
    symbolAnalysisSource = null;
    if (!frontend?.analyze_symbols || !value) {
      symbolAnalysisTimer = null;
      return;
    }

    const source = value;
    symbolAnalysisTimer = window.setTimeout(() => {
      symbolAnalysisTimer = null;
      try {
        const analysis = frontend.analyze_symbols(source);
        if (source !== value) return;
        symbolAnalysis = analysis;
        symbolAnalysisSource = source;
        refreshSymbolOverlays();
        el.dispatchEvent(
          new CustomEvent("wq-symbol-analysis", {
            detail: { analysis, source },
          }),
        );
      } catch (error) {
        console.warn("[wqide] symbol analysis failed", error);
      }
    }, 80);
  }

  function shouldInsertNewline(event) {
    if (event.altKey || event.ctrlKey || event.metaKey) return false;
    if (multilineMode === "shift") return event.shiftKey;
    if (multilineMode === "plain") return !event.shiftKey;
    return false;
  }

  function isImeCompositionActive(event) {
    return isImeCompositionKey(
      event,
      composing || compositionCommitFrame !== null,
    );
  }

  function render({ preserveSelection = true } = {}) {
    const nextSelection =
      preserveSelection && ownsSelection(el)
        ? selectionOffsets(el, value) || selection
        : selection;

    el.innerHTML = value ? renderHighlightedText(frontend, value) : "";
    if (value.endsWith("\n")) {
      el.appendChild(document.createElement("br"));
    }
    addSymbolOverlays(nextSelection.start);

    selection = {
      start: clampOffset(value, nextSelection.start),
      end: clampOffset(value, nextSelection.end),
    };
    if (document.activeElement === el || ownsSelection(el)) {
      setDomSelection(el, value, selection.start, selection.end);
    }
  }

  function setValue(nextValue, opts = {}) {
    value = normalizeValue(nextValue);
    if (opts.selectionStart !== undefined || opts.selectionEnd !== undefined) {
      const start = opts.selectionStart ?? selection.start;
      const end = opts.selectionEnd ?? start;
      selection = {
        start: clampOffset(value, start),
        end: clampOffset(value, end),
      };
    } else if (!opts.preserveSelection) {
      selection = {
        start: value.length,
        end: value.length,
      };
    } else {
      selection = {
        start: clampOffset(value, selection.start),
        end: clampOffset(value, selection.end),
      };
    }
    render({ preserveSelection: opts.preserveSelection !== false });
    scheduleSymbolAnalysis();
    if (opts.emitInput) {
      dispatchEditorInput(el);
    }
  }

  function replaceSelection(text) {
    const currentSelection = selectionOffsets(el, value) || selection;
    const start = Math.min(currentSelection.start, currentSelection.end);
    const end = Math.max(currentSelection.start, currentSelection.end);
    const insert = normalizeValue(text);
    value = value.slice(0, start) + insert + value.slice(end);
    const caret = start + insert.length;
    selection = { start: caret, end: caret };
    render({ preserveSelection: false });
    scheduleSymbolAnalysis();
    dispatchEditorInput(el);
  }

  function syncFromDom() {
    const nextValue = normalizeValue(el.textContent || "");
    const currentSelection = selectionOffsets(el, nextValue.length);
    value = nextValue;
    if (currentSelection) {
      selection = {
        start: clampOffset(value, currentSelection.start),
        end: clampOffset(value, currentSelection.end),
      };
    }
    render({ preserveSelection: true });
    scheduleSymbolAnalysis();
  }

  el.addEventListener("focus", () => {
    setDomSelection(el, value, selection.start, selection.end);
    refreshSymbolOverlays();
  });

  el.addEventListener("keyup", refreshSymbolOverlays);
  el.addEventListener("mouseup", refreshSymbolOverlays);

  el.addEventListener("keydown", (event) => {
    if (isImeCompositionActive(event)) return;
    if (event.key === "Enter" && singleLineMode) {
      event.preventDefault();
      return;
    }
    if (event.key === "Enter" && shouldInsertNewline(event)) {
      event.preventDefault();
      replaceSelection("\n");
    }
  });

  el.addEventListener("beforeinput", (event) => {
    const isLineBreak =
      event.inputType === "insertParagraph" || event.inputType === "insertLineBreak";
    if (singleLineMode && isLineBreak) {
      event.preventDefault();
      return;
    }
    if (multilineMode === "plain" && isLineBreak) {
      event.preventDefault();
      replaceSelection("\n");
    }
  });

  el.addEventListener("paste", (event) => {
    const text = event.clipboardData?.getData("text/plain");
    if (text === undefined) return;
    event.preventDefault();
    replaceSelection(text);
  });

  el.addEventListener("compositionstart", () => {
    if (compositionCommitFrame !== null) {
      window.cancelAnimationFrame(compositionCommitFrame);
      compositionCommitFrame = null;
    }
    composing = true;
  });

  el.addEventListener("compositionend", () => {
    composing = false;
    compositionCommitFrame = window.requestAnimationFrame(() => {
      compositionCommitFrame = null;
      if (!composing) syncFromDom();
    });
  });

  el.addEventListener("input", () => {
    if (composing || compositionCommitFrame !== null) {
      value = normalizeValue(el.textContent || "");
      return;
    }
    syncFromDom();
  });

  const editor = {
    element: el,
    get isComposing() {
      return composing || compositionCommitFrame !== null;
    },
    get value() {
      return value;
    },
    set value(nextValue) {
      setValue(nextValue);
    },
    get selectionStart() {
      return (selectionOffsets(el, value) || selection).start;
    },
    set selectionStart(nextStart) {
      selection = {
        start: clampOffset(value, nextStart),
        end: clampOffset(value, selection.end),
      };
      setDomSelection(el, value, selection.start, selection.end);
      refreshSymbolOverlays();
    },
    get selectionEnd() {
      return (selectionOffsets(el, value) || selection).end;
    },
    set selectionEnd(nextEnd) {
      selection = {
        start: clampOffset(value, selection.start),
        end: clampOffset(value, nextEnd),
      };
      setDomSelection(el, value, selection.start, selection.end);
      refreshSymbolOverlays();
    },
    get scrollHeight() {
      return el.scrollHeight;
    },
    get style() {
      return el.style;
    },
    addEventListener(...args) {
      return el.addEventListener(...args);
    },
    dispatchEvent(...args) {
      return el.dispatchEvent(...args);
    },
    focus(...args) {
      return el.focus(...args);
    },
    setSelectionRange(start, end = start) {
      selection = {
        start: clampOffset(value, start),
        end: clampOffset(value, end),
      };
      setDomSelection(el, value, selection.start, selection.end);
      refreshSymbolOverlays();
    },
    setValue,
    insertText(text) {
      replaceSelection(text);
    },
  };

  render({ preserveSelection: false });
  scheduleSymbolAnalysis();
  return editor;
}
