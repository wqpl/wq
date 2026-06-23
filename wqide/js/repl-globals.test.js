import test from "node:test";
import assert from "node:assert/strict";

import { countGlobalsTableRows, normalizeGlobalsTable } from "./repl-globals.js";

test("normalizes empty globals table text", () => {
  assert.equal(normalizeGlobalsTable(""), "no global bindings");
  assert.equal(normalizeGlobalsTable(null), "no global bindings");
});

test("counts rows from the formatted globals table", () => {
  const table = [
    "name  value  type",
    "----  -----  ----",
    "a     1      int",
    "f     {x}    function",
  ].join("\n");

  assert.equal(countGlobalsTableRows(table), 2);
});

test("empty globals table has zero rows", () => {
  assert.equal(countGlobalsTableRows("no global bindings"), 0);
});
