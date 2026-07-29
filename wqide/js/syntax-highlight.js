import { createSourceMapper } from "./symbol-highlights.js";

const HIGHLIGHT_KIND = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

function clampOffset(length, offset) {
  return Math.max(0, Math.min(length, Number(offset) || 0));
}

export function normalizeHighlightSpans(source, spans) {
  const text = String(source);
  const mapper = createSourceMapper(text);
  const normalized = [];

  for (const span of Array.isArray(spans) ? spans : []) {
    if (
      !Array.isArray(span?.span) ||
      span.span.length !== 2 ||
      typeof span.kind !== "string" ||
      !HIGHLIGHT_KIND.test(span.kind)
    ) {
      continue;
    }
    const [rawStart, rawEnd] = mapper.unitRange(span.span);
    const start = clampOffset(text.length, rawStart);
    const end = clampOffset(text.length, rawEnd);
    if (start >= end) continue;
    normalized.push({ start, end, kind: span.kind });
  }

  normalized.sort((left, right) => left.start - right.start || left.end - right.end);
  return normalized;
}

function appendHighlightedRange(
  fragment,
  documentRef,
  source,
  spans,
  rangeStart,
  rangeEnd,
) {
  let cursor = rangeStart;
  for (const span of spans) {
    if (span.end <= rangeStart) continue;
    if (span.start >= rangeEnd) break;
    const start = Math.max(cursor, rangeStart, span.start);
    const end = Math.min(rangeEnd, span.end);
    if (start >= end) continue;
    if (cursor < start) {
      fragment.appendChild(
        documentRef.createTextNode(source.slice(cursor, start)),
      );
    }
    const element = documentRef.createElement("span");
    element.className = `hl-${span.kind}`;
    element.textContent = source.slice(start, end);
    fragment.appendChild(element);
    cursor = end;
  }
  if (cursor < rangeEnd) {
    fragment.appendChild(
      documentRef.createTextNode(source.slice(cursor, rangeEnd)),
    );
  }
}

function frontendHighlightSpans(frontend, source) {
  if (!frontend?.highlight_spans) return [];
  return frontend.highlight_spans(source);
}

export function createHighlightedSourceFragment(
  documentRef,
  frontend,
  source,
) {
  const text = String(source);
  const fragment = documentRef.createDocumentFragment();
  let spans = [];
  try {
    spans = normalizeHighlightSpans(
      text,
      frontendHighlightSpans(frontend, text),
    );
  } catch (error) {
    console.warn("[wqide] highlight failed; falling back to plain text", error);
  }
  appendHighlightedRange(
    fragment,
    documentRef,
    text,
    spans,
    0,
    text.length,
  );
  return fragment;
}

export function renderHighlightedSource(target, frontend, source) {
  target.textContent = "";
  target.appendChild(
    createHighlightedSourceFragment(target.ownerDocument, frontend, source),
  );
}

export function highlightedSourceHtml(documentRef, frontend, source) {
  const container = documentRef.createElement("code");
  container.appendChild(
    createHighlightedSourceFragment(documentRef, frontend, source),
  );
  return container.innerHTML;
}

export function highlightedSourceLineFragments(
  documentRef,
  frontend,
  source,
) {
  const text = String(source);
  let spans = [];
  try {
    spans = normalizeHighlightSpans(
      text,
      frontendHighlightSpans(frontend, text),
    );
  } catch (error) {
    console.warn("[wqide] highlight failed; falling back to plain text", error);
  }

  const fragments = [];
  let lineStart = 0;
  for (let index = 0; index <= text.length; index += 1) {
    if (index < text.length && text[index] !== "\n") continue;
    const fragment = documentRef.createDocumentFragment();
    appendHighlightedRange(
      fragment,
      documentRef,
      text,
      spans,
      lineStart,
      index,
    );
    fragments.push(fragment);
    lineStart = index + 1;
  }
  return fragments;
}
