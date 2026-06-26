import test from "node:test";
import assert from "node:assert/strict";
import { plotSeriesArg } from "./viz-codegen.js";

const state = {
  labels: true,
  mode: "line",
  seriesOptions: true,
};

test("per-series function codegen uses unified data source key", () => {
  assert.equal(
    plotSeriesArg({ expr: "sin", label: "sin", symbol: "s", mode: "line" }, state),
    '  (`data:sin;`symbol:"s";`mode:"line";`label:"sin")',
  );
});

test("per-series raw list codegen uses unified data source key", () => {
  assert.equal(
    plotSeriesArg({ expr: "(1;3;2)", label: "raw", symbol: "#", mode: "bar" }, state),
    '  (`data:(1;3;2);`symbol:"#";`mode:"bar";`label:"raw")',
  );
});

test("per-series CAS codegen uses unified data source key", () => {
  assert.equal(
    plotSeriesArg({ expr: "@s x^2", label: "square", symbol: "q", mode: "area" }, state),
    '  (`data:@s x^2;`symbol:"q";`mode:"area";`label:"square")',
  );
});

test("per-series table-shaped codegen uses unified data source key", () => {
  assert.equal(
    plotSeriesArg({
      expr: "(`x:(0;1);`y:(2;3))",
      label: "",
      symbol: "",
      mode: "scatter",
    }, state),
    '  (`data:(`x:(0;1);`y:(2;3));`mode:"scatter";`label:"(`x:(0;1);`y:(2;3))")',
  );
});
