import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import { relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { PLAYGROUND_EXAMPLE_DEFINITIONS } from "./playground-examples-core.js";

const wasmModuleUrl = new URL("../../wq-wasm/browser.js", import.meta.url);
const wasmBinaryUrl = new URL(
  "../../wq-wasm/pkg/wq_wasm_bg.wasm",
  import.meta.url
);
const examplesDirectory = fileURLToPath(new URL("../../e/", import.meta.url));

async function readExampleSources() {
  const paths = await readdir(examplesDirectory, { recursive: true });
  const sources = new Map();
  for (const path of paths.filter((candidate) => candidate.endsWith(".wq"))) {
    const absolutePath = resolve(examplesDirectory, path);
    const specifier = relative(examplesDirectory, absolutePath)
      .split(sep)
      .join("/");
    sources.set(specifier, await readFile(absolutePath, "utf8"));
  }
  return sources;
}

test("curated Playground projects execute their real entry files", async (t) => {
  let bytes;
  try {
    bytes = await readFile(wasmBinaryUrl);
  } catch (error) {
    if (error?.code === "ENOENT") {
      t.skip("build wq-wasm first to run the Playground project test");
      return;
    }
    throw error;
  }

  const { initSync, WasmWqSession } = await import(wasmModuleUrl);
  initSync({ module: bytes });
  const sources = await readExampleSources();

  for (const definition of PLAYGROUND_EXAMPLE_DEFINITIONS) {
    const session = new WasmWqSession();
    try {
      session.set_stdout_callback(() => {});
      session.set_stderr_callback(() => {});
      for (const [specifier, source] of sources) {
        session.register_module(specifier, source);
      }
      const entry = sources.get(definition.entryPath);
      assert.equal(typeof entry, "string");
      await session.eval_wq_async(entry, {
        sourcePath: definition.entryPath
      });
    } finally {
      session.free();
    }
  }
});
