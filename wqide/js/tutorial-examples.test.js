import assert from "node:assert/strict";
import test from "node:test";

import {
  exampleHeaderLabel,
  exampleOutcome,
  parseExampleContract,
} from "./tutorial-examples.js";

test("example contracts parse without throwing on third-party content", () => {
  assert.deepEqual(parseExampleContract('{"expect":{"value":"2"}}'), {
    expect: { value: "2" },
  });
  assert.equal(parseExampleContract("{broken"), null);
  assert.equal(parseExampleContract("[]"), null);
});

test("example outcomes distinguish matches from surprises", () => {
  assert.deepEqual(
    exampleOutcome({ expect: { value: "2" } }, { value: "2" }),
    { state: "match", heading: "Result" },
  );
  assert.deepEqual(
    exampleOutcome(
      { expect: { error: "domain" } },
      { errorKind: "length" },
    ),
    {
      state: "mismatch",
      heading: "Expected domain error, got length",
    },
  );
});

test("module files use their filename as the code label", () => {
  assert.equal(
    exampleHeaderLabel("wq", { workspace: "counter", file: "counter.wq" }),
    "counter.wq",
  );
});

test("successful value contracts keep routine labels quiet", () => {
  assert.equal(
    exampleHeaderLabel("wq", { expect: { value: "2" } }),
    "wq",
  );
});
