import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const appSource = await readFile(new URL("app.js", import.meta.url), "utf8");
const styles = await readFile(new URL("../styles.css", import.meta.url), "utf8");

function cssRule(selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return styles.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`))?.[1] ?? "";
}

test("theme control exposes two plain segmented choices", () => {
  assert.match(
    appSource,
    /class="theme-toggle"[\s\S]*role="radiogroup"[\s\S]*class="theme-toggle-thumb"/,
  );
  assert.match(
    appSource,
    /data-theme-option="light"[\s\S]*>\s*Light\s*<\/button>/,
  );
  assert.match(
    appSource,
    /data-theme-option="midnight"[\s\S]*>\s*Midnight\s*<\/button>/,
  );
});

test("theme thumb follows a continuous drag position before settling", () => {
  assert.match(
    cssRule(".theme-toggle-thumb"),
    /translateX\(calc\(var\(--theme-position\) \* 100%\)\)/,
  );
  assert.match(
    appSource,
    /const previewPointerTheme[\s\S]*setThemeTogglePosition\(control, position\)[\s\S]*control\.addEventListener\("pointermove",[\s\S]*previewPointerTheme\(event\)/,
  );
  assert.match(
    appSource,
    /function themeForTogglePosition\(position\)[\s\S]*position >= 0\.5/,
  );
  assert.match(
    appSource,
    /control\.addEventListener\("pointerup",[\s\S]*applyTheme\(themeForTogglePosition\(position\)\)/,
  );
});

test("pointer theme changes preserve an active code editor caret", () => {
  assert.match(
    appSource,
    /control\.addEventListener\("pointerdown",[\s\S]*document\.activeElement[\s\S]*matches\("\.wq-editor"\)[\s\S]*event\.preventDefault\(\);/,
  );
});

test("theme choices support radio keyboard navigation", () => {
  assert.match(
    appSource,
    /control\.addEventListener\("keydown",[\s\S]*ArrowLeft[\s\S]*ArrowRight[\s\S]*Home[\s\S]*End/,
  );
  assert.match(
    appSource,
    /option\.setAttribute\("aria-checked", String\(selected\)\)/,
  );
});

test("theme hover feedback stays on the hovered choice", () => {
  assert.equal(cssRule(".theme-toggle:hover"), "");
  assert.equal(
    cssRule(':root[data-theme="midnight"] .theme-toggle:hover'),
    ""
  );
  assert.match(cssRule(".theme-toggle-option:hover"), /color:/);
});

test("Midnight restores purple controls on dark blue surfaces", () => {
  assert.match(styles, /--btn-bg:\s*#f7fcff;/);
  assert.match(styles, /--btn-primary-bg:\s*#216b55;/);
  assert.match(styles, /--btn-bg:\s*#2a1a41;/);
  assert.match(styles, /--btn-primary-bg:\s*#b19cd9;/);
  assert.match(
    cssRule(':root[data-theme="midnight"] .theme-toggle-thumb'),
    /background:\s*#c2b8e0;/
  );
});

test("Home shows its root path before search and contains no welcome remnants", () => {
  const featuredStart = appSource.indexOf("const FEATURED_HTML");
  const featuredEnd = appSource.indexOf("const PLAYGROUND_HTML");
  const featuredSource = appSource.slice(featuredStart, featuredEnd);
  const pathIndex = featuredSource.indexOf(
    'class="breadcrumbs featured-breadcrumbs"'
  );
  const searchIndex = featuredSource.indexOf(
    '<section class="featured-search"'
  );

  assert.ok(pathIndex >= 0);
  assert.ok(pathIndex < searchIndex);
  assert.doesNotMatch(featuredSource, /class="divider"/);
  assert.match(
    featuredSource,
    /class="crumb-current" aria-current="page">~<\/span>/
  );
  assert.match(
    featuredSource,
    /<section class="featured-search"[\s\S]*<h2 id="featuredSearchHeading">Search<\/h2>/
  );
  for (const source of [appSource, styles]) {
    assert.doesNotMatch(
      source,
      /route-heading|welcome-card|welcome-copy|welcome-links|welcome-constellation|wq-cat-constellation|ambient-star|constellation-shooting-star/,
    );
  }
});
