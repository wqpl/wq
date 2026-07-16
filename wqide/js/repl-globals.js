const EMPTY_GLOBALS_TEXT = "no global bindings";

function tableCell(value) {
  return String(value ?? "")
    .replaceAll("\r", "\\r")
    .replaceAll("\n", "\\n");
}

export function formatGlobalsTable(globals) {
  const rows = Array.from(globals || [], (binding) => [
    tableCell(binding?.name),
    tableCell(binding?.display),
    tableCell(binding?.type_name),
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
