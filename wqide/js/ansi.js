const ANSI_BASE_COLORS = [
  "#2a2a2a",
  "#b03030",
  "#308030",
  "#a08000",
  "#304880",
  "#803080",
  "#208080",
  "#c0c0c0",
];

const ANSI_BRIGHT_COLORS = [
  "#808080",
  "#e06060",
  "#60c060",
  "#d4a017",
  "#6090e0",
  "#c060c0",
  "#60c0c0",
  "#ffffff",
];

function createState() {
  return {
    bold: false,
    dim: false,
    italic: false,
    underline: false,
    inverse: false,
    strikethrough: false,
    fg: null,
    bg: null,
  };
}

function resetState(state) {
  Object.assign(state, createState());
}

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

function cubeLevel(index) {
  return index === 0 ? 0 : 55 + index * 40;
}

function ansi256Color(index) {
  const clamped = clamp(index, 0, 255);
  if (clamped < 8) return ANSI_BASE_COLORS[clamped];
  if (clamped < 16) return ANSI_BRIGHT_COLORS[clamped - 8];
  if (clamped < 232) {
    const value = clamped - 16;
    const r = Math.floor(value / 36);
    const g = Math.floor((value % 36) / 6);
    const b = value % 6;
    return `rgb(${cubeLevel(r)}, ${cubeLevel(g)}, ${cubeLevel(b)})`;
  }
  const level = 8 + (clamped - 232) * 10;
  return `rgb(${level}, ${level}, ${level})`;
}

function parseExtendedColor(codes, index) {
  const mode = codes[index + 1];
  if (mode === 5 && Number.isFinite(codes[index + 2])) {
    return {
      color: ansi256Color(codes[index + 2]),
      nextIndex: index + 2,
    };
  }
  if (
    mode === 2 &&
    Number.isFinite(codes[index + 2]) &&
    Number.isFinite(codes[index + 3]) &&
    Number.isFinite(codes[index + 4])
  ) {
    const r = clamp(codes[index + 2], 0, 255);
    const g = clamp(codes[index + 3], 0, 255);
    const b = clamp(codes[index + 4], 0, 255);
    return {
      color: `rgb(${r}, ${g}, ${b})`,
      nextIndex: index + 4,
    };
  }
  return { color: null, nextIndex: index };
}

function applySgrCodes(state, params) {
  const codes =
    params.length === 0
      ? [0]
      : params.map((part) => {
          const parsed = Number.parseInt(part, 10);
          return Number.isFinite(parsed) ? parsed : 0;
        });

  for (let i = 0; i < codes.length; i++) {
    const code = codes[i];
    if (code === 0) {
      resetState(state);
      continue;
    }
    if (code === 1) {
      state.bold = true;
      continue;
    }
    if (code === 2) {
      state.dim = true;
      continue;
    }
    if (code === 3) {
      state.italic = true;
      continue;
    }
    if (code === 4) {
      state.underline = true;
      continue;
    }
    if (code === 7) {
      state.inverse = true;
      continue;
    }
    if (code === 9) {
      state.strikethrough = true;
      continue;
    }
    if (code === 22) {
      state.bold = false;
      state.dim = false;
      continue;
    }
    if (code === 23) {
      state.italic = false;
      continue;
    }
    if (code === 24) {
      state.underline = false;
      continue;
    }
    if (code === 27) {
      state.inverse = false;
      continue;
    }
    if (code === 29) {
      state.strikethrough = false;
      continue;
    }
    if (code === 39) {
      state.fg = null;
      continue;
    }
    if (code === 49) {
      state.bg = null;
      continue;
    }
    if (code >= 30 && code <= 37) {
      state.fg = ANSI_BASE_COLORS[code - 30];
      continue;
    }
    if (code >= 40 && code <= 47) {
      state.bg = ANSI_BASE_COLORS[code - 40];
      continue;
    }
    if (code >= 90 && code <= 97) {
      state.fg = ANSI_BRIGHT_COLORS[code - 90];
      continue;
    }
    if (code >= 100 && code <= 107) {
      state.bg = ANSI_BRIGHT_COLORS[code - 100];
      continue;
    }
    if (code === 38 || code === 48) {
      const parsed = parseExtendedColor(codes, i);
      if (parsed.color) {
        if (code === 38) {
          state.fg = parsed.color;
        } else {
          state.bg = parsed.color;
        }
      }
      i = parsed.nextIndex;
    }
  }
}

function hasTextStyle(state) {
  return !!(
    state.bold ||
    state.dim ||
    state.italic ||
    state.underline ||
    state.inverse ||
    state.strikethrough ||
    state.fg ||
    state.bg
  );
}

function appendParsedText(fragment, documentRef, state, text) {
  if (!text) return;
  if (!hasTextStyle(state)) {
    fragment.appendChild(documentRef.createTextNode(text));
    return;
  }

  const span = documentRef.createElement("span");
  const fg = state.fg ?? "var(--ansi-fg-default, currentColor)";
  const bg = state.bg ?? "var(--ansi-bg-default, transparent)";

  if (state.inverse) {
    span.style.color = bg;
    span.style.backgroundColor = fg;
  } else {
    if (state.fg) span.style.color = state.fg;
    if (state.bg) span.style.backgroundColor = state.bg;
  }
  if (state.bold) span.style.fontWeight = "700";
  if (state.dim) span.style.opacity = "0.68";
  if (state.italic) span.style.fontStyle = "italic";

  const decorations = [];
  if (state.underline) decorations.push("underline");
  if (state.strikethrough) decorations.push("line-through");
  if (decorations.length) {
    span.style.textDecoration = decorations.join(" ");
  }

  span.textContent = text;
  fragment.appendChild(span);
}

function splitIncompleteEscape(input) {
  const escIndex = input.lastIndexOf("\u001b");
  if (escIndex === -1) {
    return { content: input, tail: "" };
  }

  const tail = input.slice(escIndex);
  if (/^\u001b\[[0-9;]*$/.test(tail) || tail === "\u001b" || tail === "\u001b[") {
    return {
      content: input.slice(0, escIndex),
      tail,
    };
  }

  return { content: input, tail: "" };
}

function renderChunk(documentRef, state, input) {
  const fragment = documentRef.createDocumentFragment();
  let cursor = 0;

  while (cursor < input.length) {
    const escIndex = input.indexOf("\u001b", cursor);
    if (escIndex === -1) {
      appendParsedText(fragment, documentRef, state, input.slice(cursor));
      break;
    }

    appendParsedText(fragment, documentRef, state, input.slice(cursor, escIndex));

    if (input[escIndex + 1] !== "[") {
      appendParsedText(fragment, documentRef, state, "\u001b");
      cursor = escIndex + 1;
      continue;
    }

    let end = escIndex + 2;
    while (end < input.length && !/[\x40-\x7e]/.test(input[end])) {
      end++;
    }
    if (end >= input.length) {
      break;
    }

    const finalByte = input[end];
    if (finalByte === "m") {
      const body = input.slice(escIndex + 2, end);
      const params = body.length ? body.split(";") : [];
      applySgrCodes(state, params);
    }
    cursor = end + 1;
  }

  return fragment;
}

function outputTextClass(options) {
  if (!options) return "";
  if (typeof options === "string") return `output-text-${options}`;
  if (options.className) return String(options.className);
  if (options.kind) return `output-text-${options.kind}`;
  return "";
}

function appendPlainText(root, chunk, options = null) {
  const text = String(chunk);
  if (!text) return;

  const className = outputTextClass(options);
  if (!className) {
    root.appendChild(root.ownerDocument.createTextNode(text));
    return;
  }

  const span = root.ownerDocument.createElement("span");
  span.className = className;
  span.textContent = text;
  root.appendChild(span);
}

function containsAnsiEscape(input) {
  return String(input).includes("\u001b");
}

export function createOutputRenderer(root, prefixNode = null) {
  const state = createState();
  let pending = "";

  const appendLegacyAnsi = (chunk) => {
    const { content, tail } = splitIncompleteEscape(pending + String(chunk));
    pending = tail;
    if (!content) return;
    root.appendChild(renderChunk(root.ownerDocument, state, content));
  };

  const appendText = (chunk, options = null) => {
    pending = "";
    resetState(state);
    appendPlainText(root, chunk, options);
  };

  const appendStreamOutput = (chunk, options = null) => {
    const text = String(chunk);
    if (!text) return;
    if (pending || hasTextStyle(state) || containsAnsiEscape(text)) {
      appendLegacyAnsi(text);
    } else {
      appendText(text, options);
    }
  };

  const appendOutput = (chunk, options = null) => {
    if (containsAnsiEscape(chunk)) {
      appendLegacyAnsi(chunk);
    } else {
      appendText(chunk, options);
    }
  };

  return {
    append: appendLegacyAnsi,
    appendLegacyAnsi,
    appendOutput,
    appendStreamOutput,
    appendText,
    appendStyledText: appendText,
    clear() {
      pending = "";
      resetState(state);
      root.textContent = "";
      if (prefixNode) {
        root.appendChild(prefixNode);
      }
    },
  };
}

// Deprecated: ANSI parsing is now a legacy compatibility path. New callers should
// import createOutputRenderer and choose appendText/appendStyledText for UI-owned
// strings, appendOutput for complete backend strings, or appendLegacyAnsi only
// for streamed backend output that may still contain SGR.
export function createAnsiRenderer(root, prefixNode = null) {
  return createOutputRenderer(root, prefixNode);
}

export function renderAnsiToText(input) {
  const state = createState();
  const { content } = splitIncompleteEscape(String(input));
  const plain = [];
  let cursor = 0;

  while (cursor < content.length) {
    const escIndex = content.indexOf("\u001b", cursor);
    if (escIndex === -1) {
      plain.push(content.slice(cursor));
      break;
    }
    plain.push(content.slice(cursor, escIndex));
    if (content[escIndex + 1] !== "[") {
      plain.push("\u001b");
      cursor = escIndex + 1;
      continue;
    }
    let end = escIndex + 2;
    while (end < content.length && !/[\x40-\x7e]/.test(content[end])) {
      end++;
    }
    if (end >= content.length) {
      break;
    }
    if (content[end] === "m") {
      const body = content.slice(escIndex + 2, end);
      applySgrCodes(state, body.length ? body.split(";") : []);
    }
    cursor = end + 1;
  }

  return plain.join("");
}
