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

test("viz separates its title from the control toolbar", () => {
  assert.match(
    appSource,
    /class="viz-topbar"[\s\S]*class="viz-stage-title"[\s\S]*data-viz-title[\s\S]*data-viz-status>Ready<\/span>[\s\S]*class="viz-stage-actions"[\s\S]*data-viz-preset-menu/
  );

  const topbarRule = styleRule(".viz-topbar");
  assert.match(topbarRule, /flex-direction:\s*column;/);
  assert.match(topbarRule, /gap:\s*0;/);

  const titleRule = styleRule(".viz-stage-title");
  assert.match(
    titleRule,
    /border-bottom:\s*1px solid var\(--surface-border-muted\);/
  );
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
    /background:\s*var\(--surface-bg-soft\);/
  );
  assert.doesNotMatch(styles, /viz-code-summary-hint|Generated code|Collapse/);
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
    styleRule(".viz-live-switch input"),
    /accent-color:\s*var\(--terminal-success\);/
  );
  assert.match(
    styleRule(".viz-code-copy"),
    /border-radius:\s*var\(--radius-xs\);/
  );
});

test("viz output follows the active surface theme", () => {
  assert.match(
    styleRule(".viz-output"),
    /background:\s*var\(--surface-bg-output\);[\s\S]*color:\s*var\(--surface-text\);/
  );
  const midnightOutputRule = styleRules(
    ':root[data-theme="midnight"] .viz-output'
  ).find((rule) => rule.includes("background: #060b16"));
  assert.match(
    midnightOutputRule,
    /background:\s*#060b16;[\s\S]*color:\s*#f1f1f3;/
  );
});
