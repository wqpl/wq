import { createSourceMapper } from "./symbol-highlights.js";

const HIGHLIGHT_KIND = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const LAYOUT_KINDS = new Set(["axis", "index", "fence"]);

function alignedPresentation(presentation, indent) {
  const text = String(presentation.text);
  if (!indent || !text.includes("\n")) {
    return {
      text,
      highlights: presentation.highlights,
      layout: presentation.layout,
    };
  }

  const mapper = createSourceMapper(text);
  const insertions = [];
  for (const match of text.matchAll(/\n(?![\n\r]*$)/g)) {
    insertions.push(mapper.byteAtUnit(match.index + 1));
  }
  if (!insertions.length) {
    return {
      text,
      highlights: presentation.highlights,
      layout: presentation.layout,
    };
  }

  const indentBytes = new TextEncoder().encode(indent).length;
  const mapOffset = (offset, inclusive) => {
    const additions = insertions.filter((point) =>
      inclusive ? point <= offset : point < offset,
    ).length;
    return offset + additions * indentBytes;
  };
  const mapSpans = (spans) =>
    (Array.isArray(spans) ? spans : []).map((span) => ({
      ...span,
      span: [
        mapOffset(span.span[0], true),
        mapOffset(span.span[1], false),
      ],
    }));

  return {
    text: text.replace(/\n(?![\n\r]*$)/g, `\n${indent}`),
    highlights: mapSpans(presentation.highlights),
    layout: mapSpans(presentation.layout),
  };
}

function presentationSpans(presentation) {
  const spans = [];
  for (const highlight of Array.isArray(presentation.highlights)
    ? presentation.highlights
    : []) {
    if (
      !Array.isArray(highlight?.span) ||
      highlight.span.length !== 2 ||
      typeof highlight.kind !== "string" ||
      !HIGHLIGHT_KIND.test(highlight.kind)
    ) {
      continue;
    }
    spans.push({
      span: highlight.span,
      className: `hl-${highlight.kind}`,
    });
  }
  for (const layout of Array.isArray(presentation.layout)
    ? presentation.layout
    : []) {
    if (
      !Array.isArray(layout?.span) ||
      layout.span.length !== 2 ||
      !LAYOUT_KINDS.has(layout.kind)
    ) {
      continue;
    }
    const classes = ["result-layout", `result-layout-${layout.kind}`];
    if (
      (layout.kind === "axis" || layout.kind === "index") &&
      Number.isSafeInteger(layout.axis) &&
      layout.axis >= 0
    ) {
      classes.push(`result-layout-axis-${layout.axis % 6}`);
    }
    spans.push({
      span: layout.span,
      className: classes.join(" "),
    });
  }
  return spans;
}

function appendSpannedText(target, presentation) {
  const text = String(presentation.text);
  const mapper = createSourceMapper(text);
  const spans = presentationSpans(presentation)
    .map((span) => {
      const [start, end] = mapper.unitRange(span.span);
      return {
        start: Math.max(0, Math.min(text.length, start)),
        end: Math.max(0, Math.min(text.length, end)),
        className: span.className,
      };
    })
    .filter((span) => span.start < span.end)
    .sort((left, right) => left.start - right.start || left.end - right.end);

  const fragment = target.ownerDocument.createDocumentFragment();
  let cursor = 0;
  for (const span of spans) {
    const start = Math.max(cursor, span.start);
    if (start >= span.end) continue;
    if (cursor < start) {
      fragment.appendChild(
        target.ownerDocument.createTextNode(text.slice(cursor, start)),
      );
    }
    const element = target.ownerDocument.createElement("span");
    element.className = span.className;
    element.textContent = text.slice(start, span.end);
    fragment.appendChild(element);
    cursor = span.end;
  }
  if (cursor < text.length) {
    fragment.appendChild(
      target.ownerDocument.createTextNode(text.slice(cursor)),
    );
  }
  target.appendChild(fragment);
}

export function appendResultPresentation(
  target,
  presentation,
  { indent = "", trailingNewline = false } = {},
) {
  if (!presentation || typeof presentation.text !== "string") return false;
  appendSpannedText(target, alignedPresentation(presentation, indent));
  if (trailingNewline) {
    target.appendChild(target.ownerDocument.createTextNode("\n"));
  }
  return true;
}
