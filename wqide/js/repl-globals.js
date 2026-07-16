const EMPTY_GLOBALS_TEXT = "no global bindings";

function tableCell(value) {
  return String(value ?? "")
    .replaceAll("\r", "\\r")
    .replaceAll("\n", "\\n");
}

export function formatGlobalBindings(globals) {
  return Array.from(globals || [], (binding) => ({
    name: tableCell(binding?.name),
    value: tableCell(binding?.display),
    type: tableCell(binding?.type_name),
  }));
}

export function formatGlobalsTable(globals) {
  const rows = formatGlobalBindings(globals).map(({ name, value, type }) => [
    name,
    value,
    type,
  ]);
  if (!rows.length) return EMPTY_GLOBALS_TEXT;

  const headers = ["name", "value", "type"];
  const widths = headers.map((header, column) =>
    rows.reduce(
      (width, row) => Math.max(width, row[column].length),
      header.length,
    ),
  );
  const formatRow = (row) =>
    row.map((cell, column) => cell.padEnd(widths[column])).join("  ");
  return [
    formatRow(headers),
    formatRow(widths.map((width) => "-".repeat(width))),
    ...rows.map(formatRow),
  ].join("\n");
}

export function formatNameColumns(items, columns = 6, gutter = 2) {
  const names = Array.from(items || [], String);
  if (!names.length) return "";
  const width = Math.max(...names.map((name) => name.length)) + gutter;
  const lines = [];
  for (let index = 0; index < names.length; index += columns) {
    lines.push(
      names
        .slice(index, index + columns)
        .map((name) => name.padEnd(width))
        .join("")
        .trimEnd(),
    );
  }
  return lines.join("\n");
}
