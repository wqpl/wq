import test from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_STATE, PRESETS } from "./viz-presets.js";

test("viz presets cover distinct asciiplot input and rendering styles", () => {
  const presets = Object.values(PRESETS);
  const signatures = presets.map((preset) =>
    JSON.stringify({
      mode: preset.mode,
      complex: preset.complex,
      axes: preset.axes,
      grid: preset.grid,
      seriesOptions: preset.seriesOptions,
      series: preset.series,
      tableXText: preset.tableXText,
      tableYText: preset.tableYText,
    }),
  );

  assert.equal(new Set(signatures).size, presets.length);

  const expressions = presets
    .flatMap((preset) => preset.series || [])
    .map((series) => series.expr);
  assert(expressions.some((expr) => expr === "sin"));
  assert(expressions.some((expr) => expr.startsWith("(")));
  assert(expressions.some((expr) => expr.includes("`x:")));
  assert(expressions.some((expr) => expr.startsWith("@s ")));
  assert(presets.some((preset) => preset.mode === "area"));
  assert(presets.some((preset) => preset.mode === "bar"));
  assert(presets.some((preset) => preset.complex === "plane"));
  assert(
    presets.some(
      (preset) => new Set(preset.series.map((series) => series.mode)).size > 1,
    ),
  );
});

test("viz presets only describe asciiplot state", () => {
  assert(!("sourceKind" in DEFAULT_STATE));
  assert(!("tableShape" in DEFAULT_STATE));
  assert(!("tableStyle" in DEFAULT_STATE));
  assert.deepEqual(Object.keys(PRESETS), [
    "trig",
    "data",
    "tablePlot",
    "cas",
    "modes",
    "bars",
    "complex",
  ]);
});
