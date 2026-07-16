import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const sources = Object.fromEntries(
  await Promise.all(
    [
      "editor.js",
      "playground.js",
      "repl.js",
      "tutorial.js",
      "viz.js",
      "wq-shared.js",
    ].map(async (name) => [
      name,
      await readFile(new URL(name, import.meta.url), "utf8"),
    ]),
  ),
);
const wasmSource = await readFile(
  new URL("../../wq-wasm/src/lib.rs", import.meta.url),
  "utf8",
);
const browserFacade = await readFile(
  new URL("../../wq-wasm/browser.js", import.meta.url),
  "utf8",
);
const viteConfig = await readFile(new URL("../vite.config.js", import.meta.url), "utf8");

test("wqide uses session and frontend handles instead of stale free functions", () => {
  const staleCalls =
    /(?<!\.)\b(?:eval_wq|highlight_wq|builtin_names|analyze_symbols|get_wq_syntax_display|get_symbol_index_json|get_builtins|set_stdout_callback|set_stderr_callback|set_stdin_callback)\s*\(/;
  for (const [name, source] of Object.entries(sources)) {
    assert.doesNotMatch(source, staleCalls, name);
  }

  assert.match(sources["wq-shared.js"], /new WasmFrontend\(\)/);
  assert.doesNotMatch(sources["editor.js"], /from "wq-wasm"/);
  assert.match(sources["editor.js"], /frontend\.highlight_wq\(text\)/);
  for (const name of ["playground.js", "repl.js", "viz.js"]) {
    assert.match(sources[name], /createWqEditor\([\s\S]*?frontend[,\s]/, name);
  }
});

test("stale one-shot and free frontend Rust exports stay removed", () => {
  for (const name of [
    "eval_wq",
    "highlight_wq",
    "builtin_names",
    "analyze_symbols",
    "get_wq_syntax_display",
    "get_symbol_index_json",
    "get_builtins",
    "set_stdout_callback",
    "set_stderr_callback",
    "set_stdin_callback",
  ]) {
    assert.doesNotMatch(wasmSource, new RegExp(`^pub fn ${name}\\s*\\(`, "m"));
  }
});

test("wqide exposes the disposal-safe browser session facade", () => {
  assert.match(wasmSource, /wasm_bindgen\(js_name = WasmWqSessionCore\)/);
  assert.match(wasmSource, /wasm_bindgen\(js_class = WasmWqSessionCore\)/);
  assert.match(browserFacade, /export class WasmWqSession/);
  assert.match(browserFacade, /#activeCalls/);
  assert.match(
    viteConfig,
    /wqWasmEntry = resolve\(rootDir, "\.\.\/wq-wasm\/browser\.js"\)/,
  );
});

test("browser session facade covers every exported Rust session method", () => {
  const rustImpl =
    /#\[wasm_bindgen\(js_class = WasmWqSessionCore\)\]\nimpl WasmWqSession \{([\s\S]*?)\n\}/.exec(
      wasmSource,
    )?.[1];
  const browserClass = /export class WasmWqSession \{([\s\S]*?)\n\}/.exec(
    browserFacade,
  )?.[1];
  assert.ok(rustImpl);
  assert.ok(browserClass);

  const rustMethods = [...rustImpl.matchAll(/^    pub fn ([a-z_][a-z0-9_]*)\(/gm)]
    .map((match) => (match[1] === "new" ? "constructor" : match[1]))
    .concat("free")
    .sort();
  const browserMethods = [
    ...browserClass.matchAll(/^  ([a-z_][a-z0-9_]*)\(/gm),
  ]
    .map((match) => match[1])
    .sort();

  assert.deepEqual(browserMethods, rustMethods);
});

test("REPL reset frees the old session before replacing it", () => {
  const reset = /function resetSession\(\) \{([\s\S]*?)\n\}/.exec(
    sources["repl.js"],
  )?.[1];
  assert.ok(reset);
  assert.match(
    reset,
    /const oldSession = session;\s*session = null;\s*oldSession\?\.free\(\);/,
  );
});
