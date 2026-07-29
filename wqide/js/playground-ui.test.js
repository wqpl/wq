import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const styles = await readFile(
  new URL("../styles.css", import.meta.url),
  "utf8",
);
const appSource = await readFile(new URL("./app.js", import.meta.url), "utf8");
const playgroundSource = await readFile(
  new URL("./playground.js", import.meta.url),
  "utf8",
);
const vizSource = await readFile(new URL("./viz.js", import.meta.url), "utf8");

function styleRules(selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return Array.from(
    styles.matchAll(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`, "g")),
    (match) => match[1],
  );
}

function styleRule(selector) {
  const [rule] = styleRules(selector);
  assert.ok(rule, `missing style rule for ${selector}`);
  return rule;
}

test("interface labels are not forced to uppercase", () => {
  assert.doesNotMatch(styles, /text-transform:\s*uppercase/i);
  assert.match(appSource, /data-viz-status>Ready</);
  for (const label of [
    "Ready",
    "Queued",
    "Running",
    "Done",
    "Interrupted",
    "Error",
  ]) {
    assert.match(vizSource, new RegExp(`setStatus\\(instance, "${label}"`));
  }
});

test("playground output header is compact and its clear action is neutral", () => {
  assert.match(styleRule(".run-output-header"), /padding:\s*6px 12px;/);

  const clearRule = styleRule(".run-output-clear");
  assert.match(clearRule, /border:\s*1px solid var\(--surface-border-soft\);/);
  assert.match(clearRule, /background:\s*var\(--surface-bg\);/);
  assert.match(clearRule, /color:\s*var\(--surface-text-muted\);/);
  assert.match(clearRule, /padding:\s*4px 9px;/);
  assert.match(clearRule, /border-radius:\s*var\(--radius-xs\);/);

  const disabledRule = styleRule(".run-output-clear:disabled");
  assert.match(disabledRule, /cursor:\s*default;/);
  assert.match(disabledRule, /opacity:\s*0\.55;/);
});

test("playground Examples uses a fixed title row and a scrolling card list", () => {
  const sidebarRule = styleRules(".playground-sidebar").find((rule) =>
    rule.includes("overflow: hidden")
  );
  assert.ok(sidebarRule);
  assert.match(sidebarRule, /padding:\s*0;/);
  assert.match(sidebarRule, /overflow:\s*hidden;/);

  const headingRule = styleRule(".playground-sidebar h2");
  assert.match(headingRule, /flex:\s*0 0 auto;/);
  assert.match(
    headingRule,
    /border-bottom:\s*1px solid var\(--surface-border-muted\);/
  );
  assert.match(headingRule, /background:\s*var\(--surface-bg-soft\);/);
  assert.doesNotMatch(headingRule, /position:\s*sticky;/);

  const listRule = styleRule(".playground-template-list");
  assert.match(listRule, /flex:\s*1 1 auto;/);
  assert.match(listRule, /min-height:\s*0;/);
  assert.match(listRule, /overflow-y:\s*auto;/);
});

test("playground editor and output share an accessible vertical split", () => {
  assert.match(
    appSource,
    /class="playground-main"[\s\S]*class="editor"[\s\S]*class="playground-splitter"[\s\S]*role="separator"[\s\S]*aria-orientation="horizontal"[\s\S]*class="run-output-panel"/
  );
  assert.match(
    styleRule(".playground-main"),
    /grid-template-rows:[\s\S]*var\(--playground-splitter-size\)/
  );
  assert.match(
    playgroundSource,
    /splitter\.addEventListener\("pointerdown"[\s\S]*splitter\.addEventListener\("pointermove"[\s\S]*setPlaygroundEditorHeight/
  );
  assert.match(
    playgroundSource,
    /splitter\.addEventListener\("keydown"[\s\S]*ArrowUp[\s\S]*ArrowDown[\s\S]*Home[\s\S]*End/
  );
});

test("playground inspector panels reuse the green REPL inspector treatment", () => {
  const panelRule = styleRule(".symbol-panel,\n.structure-panel");
  assert.match(panelRule, /background:\s*var\(--globals-panel-bg\);/);
  assert.match(
    panelRule,
    /border:\s*1px solid var\(--globals-panel-border\);/
  );

  const headRule = styleRule(".symbol-panel-head,\n.structure-panel-head");
  assert.match(
    headRule,
    /border-bottom:\s*1px solid var\(--globals-row-border\);/
  );

  const countRule = styleRule(".symbol-panel-count");
  assert.match(countRule, /border:\s*0;/);
  assert.match(countRule, /background:\s*transparent;/);
  assert.doesNotMatch(countRule, /border-radius/);

  const symbolEmptyRule = styleRule(
    ".symbol-panel:has(.symbol-panel-list:empty) .symbol-panel-status"
  );
  assert.match(symbolEmptyRule, /place-items:\s*center;/);
  assert.match(symbolEmptyRule, /text-align:\s*center;/);

  const structureEmptyRule = styleRule(".structure-panel-body.empty");
  assert.match(structureEmptyRule, /align-items:\s*center;/);
  assert.match(structureEmptyRule, /justify-content:\s*center;/);

  const structureTabRule = styleRule(".structure-tab");
  assert.match(
    structureTabRule,
    /color:\s*var\(--inspector-control-text\);/
  );
  const activeStructureTabRule = styleRule(".structure-tab.active");
  assert.match(
    activeStructureTabRule,
    /background:\s*var\(--inspector-control-active-bg\);/
  );
  assert.match(
    activeStructureTabRule,
    /color:\s*var\(--inspector-control-active-text\);/
  );
});

test("playground output empty state is centered and subdued", () => {
  const bodyRule = styleRule(".run-output-body");
  assert.match(bodyRule, /padding:\s*16px 18px;/);
  assert.match(bodyRule, /font-size:\s*14px;/);
  assert.match(bodyRule, /line-height:\s*1\.55;/);

  const emptyRule = styleRule(".run-output-body:empty");
  assert.match(emptyRule, /display:\s*flex;/);
  assert.match(emptyRule, /align-items:\s*center;/);
  assert.match(emptyRule, /justify-content:\s*center;/);
  assert.match(emptyRule, /font-size:\s*17px;/);
  assert.match(
    emptyRule,
    /color:\s*color-mix\(\s*in srgb,\s*var\(--surface-text-muted\) 68%,\s*transparent\s*\);/,
  );
});

test("playground clear action stays disabled while output is empty", () => {
  assert.match(
    appSource,
    /<button\s+id="clearOutBtn"\s+class="run-output-clear"\s+type="button"\s+disabled>/,
  );
  assert.match(
    playgroundSource,
    /function syncClearOutputButton\(instance, active = false\)/,
  );
  assert.match(
    playgroundSource,
    /const hasOutput = Boolean\(instance\.output\.textContent\.trim\(\)\);/,
  );
  assert.match(
    playgroundSource,
    /instance\.clearOutBtn\.disabled = active \|\| !hasOutput;/,
  );
  assert.match(
    playgroundSource,
    /finally\s*\{\s*instance\.resetRequested = false;\s*syncClearOutputButton\(instance\);/,
  );
});

test("poster configuration and display use labelled native dialogs", () => {
  assert.match(playgroundSource, /document\.createElement\("dialog"\)/);
  assert.match(
    playgroundSource,
    /dialog\.setAttribute\("aria-labelledby", "posterConfigHeading"\)/
  );
  assert.match(
    playgroundSource,
    /dialog\.setAttribute\("aria-labelledby", "posterDisplayHeading"\)/
  );
  assert.match(playgroundSource, /dialog\.showModal\(\)/);
  assert.match(
    playgroundSource,
    /if \(opener\?\.isConnected\) opener\.focus\(\)/
  );
  assert.doesNotMatch(playgroundSource, /poster-modal-overlay|poster-show-overlay/);
  assert.match(styles, /\.poster-dialog::backdrop\s*\{/);
});

test("Playground editor focus stays quiet while REPL focus stays visible", () => {
  assert.match(
    styles,
    /\.editor \.wq-editor\.editor-text:focus\s*\{[^}]*outline:\s*none/s
  );
  assert.doesNotMatch(styles, /\.editor:focus-within\s*\{/);
  assert.match(styles, /\.repl-live-input-row:focus-within\s*\{/);
});
