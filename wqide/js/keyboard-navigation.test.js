import assert from "node:assert/strict";
import test from "node:test";

import { nextPopupIndex, nextTabIndex } from "./ui-navigation.js";

test("tab navigation wraps and supports boundary keys", () => {
  assert.equal(nextTabIndex("ArrowRight", 2, 3), 0);
  assert.equal(nextTabIndex("ArrowLeft", 0, 3), 2);
  assert.equal(nextTabIndex("Home", 2, 3), 0);
  assert.equal(nextTabIndex("End", 0, 3), 2);
  assert.equal(nextTabIndex("Enter", 0, 3), null);
  assert.equal(nextTabIndex("ArrowRight", 0, 0), null);
});

test("popup navigation wraps and supports boundary keys", () => {
  assert.equal(nextPopupIndex("ArrowDown", 2, 3), 0);
  assert.equal(nextPopupIndex("ArrowUp", 0, 3), 2);
  assert.equal(nextPopupIndex("Home", 2, 3), 0);
  assert.equal(nextPopupIndex("End", 0, 3), 2);
  assert.equal(nextPopupIndex("Escape", 0, 3), null);
  assert.equal(nextPopupIndex("ArrowDown", 0, 0), null);
});
