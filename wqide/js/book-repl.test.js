import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  bookReplOptions,
  bookReplRoute,
  bookReplStatusLabel,
} from "./book-repl-core.js";

test("book REPL metadata keeps portable wq fences configurable", () => {
  assert.deepEqual(bookReplOptions({ repl: true }), { debugger: false });
  assert.deepEqual(bookReplOptions({ repl: { wqdb: true } }), {
    debugger: true,
  });
  assert.equal(bookReplOptions({}), null);
  assert.equal(bookReplOptions({ repl: [] }), null);
});

test("book REPL links preserve the current multiline source", () => {
  assert.equal(
    bookReplRoute("answer:40+2\nanswer"),
    "repl.html?input=answer%3A40%2B2%0Aanswer",
  );
});

test("book REPL status copy covers evaluator and debugger states", () => {
  assert.equal(bookReplStatusLabel("idle"), "Ready");
  assert.equal(bookReplStatusLabel("queued"), "Queued");
  assert.equal(bookReplStatusLabel("awaiting-input"), "Waiting for input");
  assert.equal(bookReplStatusLabel("paused"), "Paused");
  assert.equal(bookReplStatusLabel("stopping"), "Stopping");
});

test("book REPL keeps a transcript and attaches the existing debugger", async () => {
  const source = await readFile(
    new URL("./book-repl.js", import.meta.url),
    "utf8",
  );
  assert.match(source, /role", "log"/);
  assert.match(source, /new WasmWqSession\(\)/);
  assert.match(source, /createWqEditor/);
  assert.match(source, /createWqdbController/);
  assert.match(source, /renderWqdbPanel/);
  assert.match(source, /activeSession\.arm_wqdb_next\(\)/);
  assert.match(source, /run\.dataset\.action = "run"/);
  assert.doesNotMatch(source, /runtime-segment/);
  assert.doesNotMatch(source, /editor\.focus\(\)/);
  assert.match(source, /Open full REPL/);
});

test("book REPL becomes one stacked workbench when its container narrows", async () => {
  const styles = await readFile(
    new URL("../styles.css", import.meta.url),
    "utf8",
  );
  assert.match(styles, /container-name:\s*book-repl;/);
  assert.match(
    styles,
    /\.book-repl-layout\[data-debugger-open="true"\]\s*\{[^}]*grid-template-columns:\s*minmax\(0, 1\.2fr\) minmax\(0, 0\.8fr\);/,
  );
  assert.match(
    styles,
    /\.book-repl-terminal\s*\{[^}]*min-height:\s*420px;/,
  );
  assert.doesNotMatch(
    styles,
    /\.book-repl-layout\[data-debugger-open="true"\] \.book-repl-terminal/,
  );
  assert.match(
    styles,
    /@container book-repl \(max-width: 700px\)[\s\S]*?\.book-repl-layout\[data-debugger-open="true"\]\s*\{[^}]*grid-template-columns:\s*minmax\(0, 1fr\);/,
  );
  assert.match(
    styles,
    /\.book-repl-input:focus-visible\s*\{[^}]*outline:\s*2px solid var\(--history-focus-ring\);/s,
  );
  assert.doesNotMatch(styles, /\.book-repl-input-row:focus-within/);
  assert.match(
    styles,
    /\.code-action-btn\.code-action-danger:hover:not\(:disabled\)\s*\{[^}]*background:\s*var\(--globals-error-bg\);/s,
  );
  assert.match(
    styles,
    /\.book-repl[\s\S]*?\.code-action-btn\[data-action="run"\]:not\(\.code-action-danger\):hover\s*\{[^}]*color:\s*var\(--code-primary-text\);/s,
  );
});
