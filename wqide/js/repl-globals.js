const EMPTY_GLOBALS_TEXT = "no global bindings";

export function normalizeGlobalsTable(table) {
  const text = String(table ?? "").trimEnd();
  return text.trim() ? text : EMPTY_GLOBALS_TEXT;
}

export function countGlobalsTableRows(table) {
  const text = normalizeGlobalsTable(table).trim();
  if (text === EMPTY_GLOBALS_TEXT) return 0;
  const lines = text.split(/\r?\n/).filter((line) => line.trim());
  return Math.max(0, lines.length - 2);
}
