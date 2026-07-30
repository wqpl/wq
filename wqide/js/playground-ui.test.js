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

test("playground output is a spacious attached tray", () => {
  const headerRule = styleRule(".run-output-header");
  assert.match(headerRule, /min-height:\s*48px;/);
  assert.match(headerRule, /padding:\s*9px 18px;/);
  assert.match(headerRule, /background:\s*var\(--workbench-output-bg\);/);
  assert.match(
    headerRule,
    /border-bottom:\s*1px solid var\(--workbench-rule\);/
  );

  const clearRule = styleRule(".run-output-clear");
  assert.match(clearRule, /border:\s*0;/);
  assert.match(clearRule, /background:\s*transparent;/);
  assert.match(clearRule, /color:\s*var\(--code-secondary-text\);/);
  assert.match(clearRule, /padding:\s*7px 10px;/);
  assert.match(clearRule, /border-radius:\s*var\(--radius-sm\);/);

  const disabledRule = styleRule(".run-output-clear:disabled");
  assert.match(disabledRule, /cursor:\s*default;/);
  assert.match(disabledRule, /opacity:\s*0\.55;/);
});

test("playground Examples is one connected scrolling rail", () => {
  const sidebarRule = styleRules(".playground-sidebar").find((rule) =>
    rule.includes("overflow: hidden")
  );
  assert.ok(sidebarRule);
  assert.match(sidebarRule, /padding:\s*0;/);
  assert.match(sidebarRule, /overflow:\s*hidden;/);
  assert.match(sidebarRule, /border-right:\s*1px solid var\(--workbench-border\);/);
  assert.match(sidebarRule, /background:\s*var\(--workbench-rail-bg\);/);

  const headingRule = styleRule(".playground-sidebar h2");
  assert.match(headingRule, /flex:\s*0 0 auto;/);
  assert.match(
    headingRule,
    /border-bottom:\s*1px solid var\(--workbench-rule\);/
  );
  assert.match(headingRule, /background:\s*var\(--workbench-rail-bg\);/);
  assert.doesNotMatch(headingRule, /position:\s*sticky;/);

  const listRule = styleRule(".playground-template-list");
  assert.match(listRule, /flex:\s*1 1 auto;/);
  assert.match(listRule, /min-height:\s*0;/);
  assert.match(listRule, /overflow-y:\s*auto;/);

  const cardRule = styleRule(".playground-template-card");
  assert.match(cardRule, /border-bottom:\s*1px solid var\(--workbench-rule\);/);
  assert.match(cardRule, /border-radius:\s*0;/);
  assert.match(cardRule, /background:\s*transparent;/);
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

test("playground inspector is an attached workbench rail", () => {
  const inspectorRule = styleRule(".playground-inspector");
  assert.match(
    inspectorRule,
    /border-left:\s*1px solid var\(--workbench-border\);/
  );
  assert.match(
    inspectorRule,
    /background:\s*var\(--workbench-rail-bg\);/
  );

  const panelRule = styleRule(".symbol-panel,\n.structure-panel");
  assert.match(panelRule, /background:\s*var\(--workbench-rail-bg\);/);
  assert.match(panelRule, /border:\s*0;/);
  assert.match(panelRule, /border-radius:\s*0;/);

  const headRule = styleRule(".symbol-panel-head,\n.structure-panel-head");
  assert.match(
    headRule,
    /border-bottom:\s*1px solid var\(--workbench-rule\);/
  );
  const structureRule = styleRules(".structure-panel").find((rule) =>
    rule.includes("border-top")
  );
  assert.match(
    structureRule,
    /border-top:\s*1px solid var\(--workbench-rule\);/
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
    /background:\s*transparent;/
  );
  assert.doesNotMatch(activeStructureTabRule, /color:/);
  assert.match(
    appSource,
    /class="structure-tabs segmented-control"[\s\S]*class="segmented-control-thumb"/
  );
});

test("playground output empty state is centered and subdued", () => {
  const bodyRule = styleRule(".run-output-body");
  assert.match(bodyRule, /padding:\s*20px 18px 24px;/);
  assert.match(bodyRule, /font-size:\s*14px;/);
  assert.match(bodyRule, /line-height:\s*1\.55;/);
  assert.match(bodyRule, /background:\s*var\(--workbench-output-bg\);/);

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

test("playground runtime toggles use flat segments with a clear active fill", () => {
  assert.match(
    appSource,
    /class="pills runtime-toggle-cluster"[\s\S]*id="playgroundBoxBtn"[\s\S]*class="pill inactive runtime-segment"[\s\S]*aria-pressed="false"[\s\S]*id="playgroundTimeBtn"[\s\S]*class="pill inactive runtime-segment"[\s\S]*aria-pressed="false"/
  );
  const clusterRule = styleRule(".runtime-toggle-cluster");
  assert.match(clusterRule, /padding:\s*4px;/);
  assert.match(
    clusterRule,
    /border:\s*1px solid var\(--control-cluster-border\);/
  );
  assert.match(clusterRule, /border-radius:\s*var\(--radius-surface\);/);
  assert.match(clusterRule, /background:\s*var\(--control-cluster-bg\);/);

  const segmentRule = styleRule(".runtime-segment");
  assert.match(segmentRule, /border-color:\s*transparent;/);
  assert.match(segmentRule, /border-radius:\s*var\(--radius-md\);/);

  const activeRule = styleRule(".runtime-segment.active");
  assert.match(activeRule, /background:\s*var\(--btn-primary-bg\);/);
  assert.match(activeRule, /color:\s*var\(--btn-primary-text\);/);
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
  assert.doesNotMatch(styles, /poster-slide-up/);
});

test("Playground editor focus stays quiet while REPL focus stays visible", () => {
  assert.match(
    styles,
    /\.editor \.wq-editor\.editor-text:focus\s*\{[^}]*outline:\s*none/s
  );
  assert.doesNotMatch(styles, /\.editor:focus-within\s*\{/);
  assert.match(styles, /\.repl-live-input-row:focus-within\s*\{/);
});
