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

  const disabledRule = styleRule(".run-output-clear:disabled");
  assert.match(disabledRule, /cursor:\s*default;/);
  assert.match(disabledRule, /opacity:\s*0\.55;/);
});

test("playground Examples heading stays above the scrolling cards", () => {
  const sidebarRule = styleRules(".playground-sidebar").find((rule) =>
    rule.includes("overflow-y: auto"),
  );
  assert.ok(sidebarRule);
  assert.match(sidebarRule, /padding:\s*0;/);
  assert.match(sidebarRule, /overflow-y:\s*auto;/);
  assert.match(sidebarRule, /isolation:\s*isolate;/);

  const headingRule = styleRule(".playground-sidebar h2");
  assert.match(headingRule, /position:\s*sticky;/);
  assert.match(headingRule, /top:\s*0;/);
  assert.match(headingRule, /z-index:\s*3;/);
  assert.match(headingRule, /background:\s*var\(--surface-bg-soft\);/);

  const listRule = styleRule(".playground-template-list");
  assert.match(listRule, /padding:\s*0 var\(--space-8\) var\(--space-8\);/);
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
