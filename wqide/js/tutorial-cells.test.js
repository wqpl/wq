import assert from "node:assert/strict";
import test from "node:test";

import {
  cellGroupId,
  cellRunLabel,
  hasFinalResult,
  planCellRuns,
} from "./tutorial-cells.js";

test("cell groups share one run while ungrouped cells stay independent", () => {
  const first = { contract: { cellGroup: "calculator" } };
  const second = { contract: { cellGroup: "calculator" } };
  const independent = { contract: null };
  const later = { contract: { cellGroup: "calculator" } };

  assert.deepEqual(planCellRuns([first, second, independent, later]), [
    { id: "calculator", cells: [first, second] },
    { id: null, cells: [independent] },
    { id: "calculator", cells: [later] },
  ]);
  assert.equal(cellGroupId({ cellGroup: "  calculator  " }), "calculator");
  assert.equal(cellGroupId({ cellGroup: "  " }), null);
  assert.equal(cellRunLabel(2), "Run 2 cells");
});

test("cell groups stop when their code panels are not adjacent", () => {
  const first = { id: 1, contract: { cellGroup: "calculator" } };
  const second = { id: 2, contract: { cellGroup: "calculator" } };

  assert.deepEqual(
    planCellRuns([first, second], () => false),
    [
      { id: "calculator", cells: [first] },
      { id: "calculator", cells: [second] },
    ],
  );
});

test("unit remains a visible final result", () => {
  assert.equal(hasFinalResult({ display: "()" }), true);
  assert.equal(hasFinalResult({ display: "" }), false);
  assert.equal(hasFinalResult(null), false);
});
