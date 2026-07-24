import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { isImeCompositionKey } from "./editor.js";

const editorSource = await readFile(
  new URL("editor.js", import.meta.url),
  "utf8",
);
const replSource = await readFile(new URL("repl.js", import.meta.url), "utf8");

test("IME commits wait for the browser to finalize the native caret", () => {
  assert.match(
    editorSource,
    /el\.addEventListener\("compositionend",[\s\S]*composing = false;[\s\S]*requestAnimationFrame\([\s\S]*syncFromDom\(\);/,
  );
  assert.match(
    editorSource,
    /function refreshSymbolOverlays\(\) \{\s*if \(composing \|\| compositionCommitFrame !== null\) return;/,
  );
  assert.match(
    editorSource,
    /el\.addEventListener\("input",[\s\S]*if \(composing \|\| compositionCommitFrame !== null\)/,
  );
});

test("IME confirmation Enter does not trigger editor or REPL Enter actions", () => {
  assert.equal(isImeCompositionKey({ isComposing: true, keyCode: 13 }), true);
  assert.equal(isImeCompositionKey({ isComposing: false, keyCode: 229 }), true);
  assert.equal(
    isImeCompositionKey({ isComposing: false, keyCode: 13 }, true),
    true,
  );
  assert.equal(
    isImeCompositionKey({ isComposing: false, keyCode: 13 }, false),
    false,
  );
  assert.match(
    editorSource,
    /el\.addEventListener\("keydown", \(event\) => \{\s*if \(isImeCompositionActive\(event\)\) return;/,
  );
  assert.match(
    editorSource,
    /get isComposing\(\) \{\s*return composing \|\| compositionCommitFrame !== null;/,
  );
  assert.match(
    replSource,
    /ui\.codeEl\.addEventListener\("keydown", \(e\) => \{\s*if \(isImeCompositionKey\(e, ui\.codeEl\.isComposing\)\) return;/,
  );
});
