import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const appSource = await readFile(new URL("app.js", import.meta.url), "utf8");
const styles = await readFile(new URL("../styles.css", import.meta.url), "utf8");
const segmentedSource = await readFile(
  new URL("ui-segmented.js", import.meta.url),
  "utf8"
);

function cssRule(selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return styles.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`))?.[1] ?? "";
}

test("theme control exposes two plain segmented choices", () => {
  assert.match(
    appSource,
    /class="theme-toggle"[\s\S]*role="radiogroup"[\s\S]*class="theme-toggle-thumb"[\s\S]*class="theme-toggle-label-window"/,
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

test("other segmented controls share a continuously draggable thumb", () => {
  assert.match(
    cssRule(".segmented-control-thumb"),
    /translateX\(calc\(var\(--segment-position\) \* 100%\)\)/
  );
  assert.match(
    segmentedSource,
    /function preview\(clientX\)[\s\S]*--segment-position[\s\S]*addEventListener\("pointermove"[\s\S]*preview\(event\.clientX\)/
  );
  for (const className of [
    "inspector-tabs",
    "structure-tabs",
    "viz-layout-toggle"
  ]) {
    assert.match(appSource, new RegExp(`class="${className} segmented-control"`));
  }
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

test("workbench segmented controls keep hover inside the tray", () => {
  for (const selector of [
    ".inspector-tabs:hover",
    ".structure-tabs:hover",
    ".viz-layout-toggle:hover",
    ".wqdb-granularity-options:hover"
  ]) {
    assert.equal(cssRule(selector), "");
  }
  assert.match(
    cssRule(".segmented-control-thumb"),
    /border:\s*1px solid var\(--segment-active-border\);[\s\S]*background:\s*var\(--segment-active-bg\);/
  );
  assert.match(styles, /--segment-active-bg:\s*var\(--btn-primary-bg\);/);
  assert.match(styles, /--segment-active-text:\s*var\(--btn-primary-text\);/);
  assert.match(styles, /--segment-active-bg:\s*#563c71;/);
  assert.match(styles, /--segment-active-text:\s*#f7f3ff;/);
});

test("sliding labels follow the thumb instead of selection state", () => {
  assert.equal(cssRule('.theme-toggle-option[aria-checked="true"]'), "");
  assert.match(
    cssRule(".theme-toggle-label-window"),
    /color:\s*var\(--theme-toggle-selected-text\);[\s\S]*translateX\(calc\(var\(--theme-position\) \* 100%\)\)/
  );
  assert.match(
    cssRule(".theme-toggle-label"),
    /var\(--theme-label-index\) \* 100% - var\(--theme-position\) \* 100%/
  );
  assert.match(
    cssRule(".segmented-control-label-window"),
    /color:\s*var\(--segment-active-text\);[\s\S]*translateX\(calc\(var\(--segment-position\) \* 100%\)\)/
  );
  assert.match(
    cssRule(".segmented-control-label"),
    /var\(--segment-index\) \* 100% - var\(--segment-position\) \* 100%/
  );
  assert.match(
    segmentedSource,
    /className = "segmented-control-label-window"[\s\S]*setAttribute\("aria-hidden", "true"\)[\s\S]*--segment-index/
  );
});

test("Midnight restores purple controls on dark blue surfaces", () => {
  assert.match(styles, /--btn-bg:\s*transparent;/);
  assert.match(styles, /--btn-hover-bg:\s*#f3fcf5;/);
  assert.match(styles, /--btn-primary-bg:\s*#216b55;/);
  assert.match(styles, /--btn-hover-bg:\s*#241b38;/);
  assert.match(styles, /--btn-primary-bg:\s*#b19cd9;/);
  assert.match(
    cssRule(':root[data-theme="midnight"] .theme-toggle-thumb'),
    /background:\s*#c2b8e0;/
  );
});

test("the header theme switch keeps its light floating thumb treatment", () => {
  assert.match(cssRule(".theme-toggle-thumb"), /background:\s*#f7fcff;/);
  assert.match(styles, /--theme-toggle-selected-text:\s*#153f59;/);
  assert.match(styles, /--theme-toggle-selected-text:\s*#0f1e3d;/);
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
