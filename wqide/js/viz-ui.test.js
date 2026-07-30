import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const appSource = await readFile(new URL("./app.js", import.meta.url), "utf8");
const styles = await readFile(
  new URL("../styles.css", import.meta.url),
  "utf8"
);

function styleRules(selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = new RegExp(`^${escaped}\\s*\\{([^}]*)\\}`, "gm");
  return [...styles.matchAll(pattern)].map((match) => match[1]);
}

function styleRule(selector) {
  const [rule] = styleRules(selector);
  assert.ok(rule, `missing style rule for ${selector}`);
  return rule;
}

test("viz is measurable before its first auto-width render", () => {
  const mountStart = appSource.indexOf("async function mountViz(route)");
  const mountEnd = appSource.indexOf("async function mountRepl(route)");
  const mountSource = appSource.slice(mountStart, mountEnd);

  const showIndex = mountSource.indexOf("showView(root);");
  const firstRenderIndex = mountSource.indexOf("await mod.mountViz(root);");

  assert.notEqual(showIndex, -1);
  assert.notEqual(firstRenderIndex, -1);
  assert.ok(showIndex < firstRenderIndex);
});

test("palette options show four trailing color dots", () => {
  for (const palette of ["classic", "bright", "ink"]) {
    const optionPattern = new RegExp(
      `data-viz-option="${palette}"[\\s\\S]*?` +
        `<span>${palette}</span>[\\s\\S]*?` +
        `class="viz-palette-preview viz-palette-${palette}"[\\s\\S]*?` +
        `(?:<i></i>\\s*){4}`,
    );
    assert.match(appSource, optionPattern);
  }

  const optionRule = styleRule(".viz-palette-option");
  assert.match(optionRule, /display:\s*flex;/);
  assert.match(optionRule, /justify-content:\s*space-between;/);

  const previewRule = styleRule(".viz-palette-preview");
  assert.match(previewRule, /margin-left:\s*auto;/);

  const dotRule = styleRule(".viz-palette-preview i");
  assert.match(dotRule, /border-radius:\s*50%;/);
  assert.match(dotRule, /background:\s*var\(--viz-palette-color\);/);
});

test("viz combines its title and actions in one workbench header", () => {
  assert.match(
    appSource,
    /class="viz-topbar"[\s\S]*class="viz-stage-title"[\s\S]*data-viz-title[\s\S]*data-viz-status>Ready<\/span>[\s\S]*class="viz-stage-actions"[\s\S]*data-viz-preset-menu/
  );

  const topbarRule = styleRule(".viz-topbar");
  assert.match(
    topbarRule,
    /grid-template-columns:\s*minmax\(0, 1fr\) auto;/
  );
  assert.match(topbarRule, /gap:\s*0;/);
  assert.match(topbarRule, /background:\s*var\(--workbench-header-bg\);/);
  assert.match(
    topbarRule,
    /border-bottom:\s*1px solid var\(--workbench-border\);/
  );

  const titleRule = styleRule(".viz-stage-title");
  assert.match(titleRule, /padding:\s*17px 20px;/);
  assert.match(titleRule, /border-bottom:\s*0;/);
});

test("viz code disclosure uses a quiet conventional affordance", () => {
  assert.match(
    appSource,
    /<summary>[\s\S]*class="viz-code-chevron"[\s\S]*<span>Code<\/span>\s*<\/summary>/
  );
  assert.match(
    styles,
    /\.viz-code-chevron\s*\{[^}]*width:\s*12px;[^}]*height:\s*12px;[^}]*transform-origin:\s*50% 50%;/
  );
  assert.match(
    styleRule(".viz-code-panel summary"),
    /border-bottom:\s*1px solid transparent;[\s\S]*background:\s*var\(--workbench-output-bg\);[\s\S]*background-color 260ms cubic-bezier/
  );
  assert.match(
    styleRule(".viz-code-chevron"),
    /color 240ms ease,[\s\S]*transform 240ms cubic-bezier/
  );
  assert.match(
    styleRule(".viz-code-panel summary:hover"),
    /background:\s*var\(--workbench-rail-bg\);/
  );
  assert.match(
    styleRule(".viz-code-panel[open] summary"),
    /border-bottom-color:\s*var\(--workbench-rule\);/
  );
  assert.doesNotMatch(styles, /viz-code-summary-hint|Generated code|Collapse/);
});

test("expanded viz code overlays the stage without stretching Data", () => {
  assert.match(
    appSource,
    /class="viz-code-panel"[\s\S]*<summary>[\s\S]*data-viz-copy-code[\s\S]*data-viz-code/
  );
  const openRule = styleRule(".viz-code-panel[open]");
  assert.match(openRule, /position:\s*absolute;/);
  assert.match(openRule, /top:\s*0;/);
  assert.match(openRule, /bottom:\s*auto;/);
  assert.match(openRule, /max-height:\s*min\(70vh, 640px\);/);
  assert.match(openRule, /box-shadow:\s*var\(--terminal-menu-shadow\);/);
  assert.match(styleRule(".viz-control-group.viz-data-panel"), /height:\s*auto;/);
});

test("viz controls use the shared blue and green control palette", () => {
  assert.match(
    styleRule(".viz-code-copy"),
    /border:\s*1px solid var\(--btn-border\);[\s\S]*background:\s*var\(--btn-bg\);[\s\S]*color:\s*var\(--btn-text\);/
  );
  assert.match(
    styleRule('.viz-preset-trigger[aria-expanded="true"]'),
    /background:\s*var\(--pill-active-bg\);[\s\S]*color:\s*var\(--pill-active-text\);/
  );
  assert.match(
    styles,
    /\.viz-live-switch input\[type="checkbox"\],[\s\S]*?\.poster-field-inline input\[type="checkbox"\]\s*\{[^}]*background:\s*var\(--surface-bg-field\);[^}]*appearance:\s*none;/s
  );
  assert.match(
    styleRule(".viz-code-copy"),
    /min-height:\s*40px;[\s\S]*border-radius:\s*var\(--radius-control\);/
  );
});

test("viz output follows the active surface theme", () => {
  assert.match(
    styleRule(".viz-output"),
    /background:\s*var\(--workbench-output-bg\);[\s\S]*color:\s*var\(--surface-text\);/
  );
  const midnightOutputRule = styleRules(
    ':root[data-theme="midnight"] .viz-output'
  ).find((rule) => rule.includes("background: var(--workbench-output-bg)"));
  assert.match(
    midnightOutputRule,
    /background:\s*var\(--workbench-output-bg\);[\s\S]*color:\s*#f1f1f3;/
  );
});

test("viz view toggles sit inside one control capsule", () => {
  assert.match(
    appSource,
    /class="viz-view-controls"[\s\S]*class="viz-live-switch"[\s\S]*class="viz-layout-toggle segmented-control"/
  );
  const clusterRule = styleRule(".viz-view-controls");
  assert.match(clusterRule, /padding:\s*4px;/);
  assert.match(
    clusterRule,
    /border:\s*1px solid var\(--control-cluster-border\);/
  );
  assert.match(clusterRule, /background:\s*var\(--control-cluster-bg\);/);
  assert.match(
    appSource,
    /class="viz-layout-toggle segmented-control"[\s\S]*class="segmented-control-thumb"/
  );
  assert.match(
    styles,
    /\.viz-layout-toggle button\.active\s*\{[^}]*background:\s*transparent;/
  );
});

test("viz status uses the same minimal state dot as the REPL", () => {
  assert.match(
    styleRule(".viz-status::before"),
    /width:\s*7px;[\s\S]*height:\s*7px;[\s\S]*border-radius:\s*50%;[\s\S]*background:\s*var\(--terminal-success\);/
  );
  assert.match(
    styles,
    /\.viz-status\[data-tone="running"\]::before\s*\{[^}]*background:\s*var\(--terminal-blue\);/
  );
  assert.match(
    styles,
    /\.viz-status\[data-tone="error"\]::before\s*\{[^}]*background:\s*var\(--terminal-error\);/
  );
});

test("viz dropdowns use stroked chevrons and center the Presets popover", () => {
  for (const selector of [
    ".viz-preset-trigger::after",
    ".viz-select-button::after"
  ]) {
    const chevronRule = styleRule(selector);
    assert.match(chevronRule, /width:\s*14px;/);
    assert.match(chevronRule, /height:\s*14px;/);
    assert.match(chevronRule, /mask:[\s\S]*m6 9 6 6 6-6/);
    assert.doesNotMatch(chevronRule, /border-(?:left|right|top):/);
  }

  const popoverRule = styleRule(".viz-preset-popover");
  assert.match(popoverRule, /left:\s*50%;/);
  assert.match(popoverRule, /transform:\s*translateX\(-50%\);/);
});
