import test from "node:test";
import assert from "node:assert/strict";

import { formatGlobalsTable, formatNameColumns } from "./repl-globals.js";

test("formats empty structured globals", () => {
  assert.equal(formatGlobalsTable([]), "no global bindings");
  assert.equal(formatGlobalsTable(null), "no global bindings");
});

test("formats structured globals as a presentation table", () => {
  const table = formatGlobalsTable([
    { name: "a", display: "1", type_name: "int" },
    { name: "f", display: "{x}", type_name: "function" },
  ]);

  assert.equal(
    table,
    [
      "name  value  type    ",
      "----  -----  --------",
      "a     1      int     ",
      "f     {x}    function",
    ].join("\n"),
  );
});

test("escapes line breaks inside global displays", () => {
  const table = formatGlobalsTable([
    { name: "s", display: "a\nb", type_name: "string" },
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
