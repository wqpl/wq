import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const integrationFiles = [
  "app.js",
  "playground.js",
  "playground-examples-core.js",
  "repl.js",
  "tutorial.js",
  "viz.js",
];

async function integrationSource() {
  return (
    await Promise.all(
      integrationFiles.map((name) =>
        readFile(new URL(name, import.meta.url), "utf8"),
      ),
    )
  ).join("\n");
}

test("every wqide async evaluation receives its run signal", async () => {
  const source = await integrationSource();
  const calls = [...source.matchAll(/eval_wq_async\(/g)];
  assert.ok(calls.length >= 5);
  for (const call of calls) {
    const callSource = source.slice(call.index, call.index + 300);
    assert.match(callSource, /\bsignal\b/);
  }
});
