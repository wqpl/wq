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

function pointForOffset(el, offset) {
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
  let node = walker.nextNode();
  let remaining = offset;
  let lastTextNode = null;

  while (node) {
    const len = node.nodeValue.length;
    if (remaining <= len) {
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

  function shouldInsertNewline(event) {
    if (event.altKey || event.ctrlKey || event.metaKey) return false;
    if (multilineMode === "shift") return event.shiftKey;
    if (multilineMode === "plain") return !event.shiftKey;
    return false;
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
    dispatchEditorInput(el);
  }

  el.addEventListener("focus", () => {
    setDomSelection(el, value, selection.start, selection.end);
  });

  el.addEventListener("keydown", (event) => {
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
    composing = true;
  });

  el.addEventListener("compositionend", () => {
    composing = false;
    const nextValue = normalizeValue(el.textContent || "");
    const nextSelection = selectionOffsets(el, nextValue.length) || {
      start: nextValue.length,
      end: nextValue.length,
    };
    setValue(nextValue, {
      selectionStart: nextSelection.start,
      selectionEnd: nextSelection.end,
      preserveSelection: true,
    });
  });

  el.addEventListener("input", () => {
    if (composing) {
      value = normalizeValue(el.textContent || "");
      return;
    }
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
  });

  const editor = {
    element: el,
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
    },
    setValue,
    insertText(text) {
      replaceSelection(text);
    },
  };

  render({ preserveSelection: false });
  return editor;
}
