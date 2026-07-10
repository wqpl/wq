import assert from "node:assert/strict";
import test from "node:test";

import {
  WQ_CAT_ART,
  renderWqCatConstellation,
} from "./startup-constellation.js";

test("startup constellation preserves the REPL wq cat silhouette", () => {
  assert.deepEqual(WQ_CAT_ART, [
    "          **********",
    "   ********        ********",
    "      ***            ****",
    "     ***              ***",
    "     ***              ***",
    "     ***              ***",
    "      ***  *  **  *  ***",
    "       ****         ***",
    "          ****  *****",
    "     **   ****          *",
    "              *********",
  ]);
});

test("startup constellation renders every cat point as an animated star", () => {
  const catPointCount = WQ_CAT_ART.join("").match(/\*/g).length;
  const html = renderWqCatConstellation();

  assert.equal(html.match(/class="wq-cat-star wq-cat-star-\d"/g).length, catPointCount);
  assert.equal(html.split("\n").length, WQ_CAT_ART.length);
  assert.match(html, /--star-delay: -\d\.\ds/);
  assert.match(html, /--star-duration: \d\.\ds/);
  assert.match(html, />\+</);
  assert.match(html, />·</);
});
