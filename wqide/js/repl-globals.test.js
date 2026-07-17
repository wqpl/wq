import test from "node:test";
import assert from "node:assert/strict";

import {
  formatGlobalBindings,
  formatGlobalsTable,
  formatNameColumns,
} from "./repl-globals.js";

test("formats structured globals for the panel", () => {
  assert.deepEqual(
    formatGlobalBindings([
      { name: "answer", display: "42", category: "int" },
      { name: "lines", display: "a\nb", category: "list" },
    ]),
    [
      { name: "answer", value: "42", category: "int" },
      { name: "lines", value: "a\\nb", category: "list" },
    ],
  );
  assert.deepEqual(formatGlobalBindings(null), []);
});

test("formats empty structured globals", () => {
  assert.equal(formatGlobalsTable([]), "no global bindings");
  assert.equal(formatGlobalsTable(null), "no global bindings");
});

test("formats structured globals as a presentation table", () => {
  const table = formatGlobalsTable([
    { name: "a", display: "1", category: "int" },
    { name: "f", display: "{x}", category: "function" },
  ]);

  assert.equal(
    table,
    [
      "name  value  category",
      "----  -----  --------",
      "a     1      int     ",
      "f     {x}    function",
    ].join("\n"),
  );
});

test("escapes line breaks inside global displays", () => {
  const table = formatGlobalsTable([
    { name: "s", display: "a\nb", category: "list" },
  ]);

  assert.match(table, /a\\nb/);
  assert.equal(table.split("\n").length, 3);
});

test("formats builtin names into UI columns", () => {
  assert.equal(
    formatNameColumns(["a", "long", "z"], 2, 2),
    "a     long\nz",
  );
});
