import assert from "node:assert/strict";
import test from "node:test";

import {
  activeBindingHighlights,
  createSourceMapper,
} from "./symbol-highlights.js";

test("source mapper converts UTF-8 byte offsets and UTF-16 editor offsets", () => {
  const mapper = createSourceMapper("a🦀b");

  assert.equal(mapper.unitAtByte(0), 0);
  assert.equal(mapper.unitAtByte(1), 1);
  assert.equal(mapper.unitAtByte(5), 3);
  assert.equal(mapper.byteAtUnit(0), 0);
  assert.equal(mapper.byteAtUnit(1), 1);
  assert.equal(mapper.byteAtUnit(3), 5);
  assert.deepEqual(mapper.unitRange([1, 5]), [1, 3]);
});

test("active binding highlights group occurrences by definition", () => {
  const analysis = {
    occurrences: [
      { span: [0, 1], def: 4, kind: "write" },
      { span: [8, 9], def: 4, kind: "read" },
      { span: [12, 13], def: 4, kind: "ref-write" },
      { span: [16, 17], def: 9, kind: "read" },
    ],
  };

  assert.deepEqual(activeBindingHighlights(analysis, 8), [
    { span: [0, 1], role: "write", current: false },
    { span: [8, 9], role: "read", current: true },
    { span: [12, 13], role: "write", current: false },
  ]);
});

test("active binding highlights accept a caret at an occurrence end", () => {
  const analysis = {
    occurrences: [{ span: [0, 1], def: 4, kind: "read" }],
  };

  assert.deepEqual(activeBindingHighlights(analysis, 1), [
    { span: [0, 1], role: "read", current: true },
  ]);
  assert.deepEqual(activeBindingHighlights(analysis, 20), []);
});
