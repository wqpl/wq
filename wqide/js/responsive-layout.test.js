import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const appSource = await readFile(new URL("./app.js", import.meta.url), "utf8");
const styles = await readFile(
  new URL("../styles.css", import.meta.url),
  "utf8"
);

test("Playground mobile reading order follows the primary task", () => {
  const playgroundStart = appSource.indexOf("const PLAYGROUND_HTML");
  const playgroundEnd = appSource.indexOf("const VIZ_HTML");
  const playground = appSource.slice(playgroundStart, playgroundEnd);
  const editor = playground.indexOf('class="editor"');
  const output = playground.indexOf('class="run-output-panel"');
  const examples = playground.indexOf('class="playground-sidebar"');
  const inspector = playground.indexOf('class="playground-inspector"');

  assert.ok(editor >= 0);
  assert.ok(editor < output);
  assert.ok(output < examples);
  assert.ok(examples < inspector);
});

test("desktop grid places examples and inspector beside the split workbench", () => {
  assert.match(
    styles,
    /grid-template-areas:\s*"header header header"\s*"examples editor inspector";/
  );
  assert.match(
    styles,
    /grid-template-areas:\s*"header"\s*"editor"\s*"examples"\s*"inspector";/
  );
});

test("narrow examples collapse into compact rows", () => {
  assert.match(
    styles,
    /\.playground-template-list\s*\{[^}]*grid-template-columns:\s*repeat\(2, minmax\(0, 1fr\)\);/
  );
  assert.match(
    styles,
    /\.playground-template-code\s*\{[^}]*display:\s*none;/
  );
  assert.match(
    styles,
    /\.playground-template-list\s*\{[^}]*grid-template-columns:\s*1fr;/
  );
});

test("coarse pointers receive durable touch targets", () => {
  assert.match(styles, /@media \(pointer: coarse\)/);
  assert.match(
    styles,
    /button,[\s\S]*?\.code-action-btn\s*\{[^}]*min-height:\s*44px;/
  );
  assert.match(
    styles,
    /input\[type="range"\]\s*\{[^}]*min-height:\s*44px;/
  );
  assert.match(
    styles,
    /\.crumb-back,\s*\.crumb-path\s*\{[^}]*height:\s*44px;/
  );
});

test("grouped tutorial cells keep a proportional numbered rail on phones", () => {
  assert.match(
    styles,
    /@media \(max-width: 560px\)[\s\S]*?\.tutorial-cell\s*\{[^}]*grid-template-columns:\s*42px minmax\(0, 1fr\);/
  );
});
