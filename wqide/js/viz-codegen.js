export function wqString(value) {
  return `"${String(value).replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

export function named(name, value) {
  return `  \`${name}:${value}`;
}

export function plotSeriesArg(series, state) {
  const expr = series.expr.trim();
  if (!state.seriesOptions) return `  ${expr}`;
  const parts = [`\`data:${expr}`];
  const symbol = series.symbol.trim();
  const mode = series.mode.trim() || state.mode;
  const label = series.label.trim() || expr.replace(/^@s\s+/, "");
  if (symbol) parts.push(`\`symbol:${wqString(symbol)}`);
  if (mode) parts.push(`\`mode:${wqString(mode)}`);
  if (state.labels && label) parts.push(`\`label:${wqString(label)}`);
  return `  (${parts.join(";")})`;
}
