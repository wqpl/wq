import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const appSource = await readFile(new URL("app.js", import.meta.url), "utf8");
const styles = await readFile(new URL("../styles.css", import.meta.url), "utf8");

function cssRule(selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return styles.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`))?.[1] ?? "";
}

test("theme artwork is clipped inside the pill border", () => {
  assert.match(
    appSource,
    /class="theme-toggle-scene"[\s\S]*class="theme-cloud theme-cloud-front"[\s\S]*class="theme-toggle-icon theme-toggle-midnight"[\s\S]*<span class="theme-toggle-label">/,
  );

  const scene = cssRule(".theme-toggle-scene");
  assert.match(scene, /inset:\s*1px;/);
  assert.match(scene, /overflow:\s*hidden;/);
  assert.match(scene, /border-radius:\s*inherit;/);
});

test("theme change crossfades scenes without replaying travel keyframes", () => {
  const toggle = cssRule(".theme-toggle");
  assert.doesNotMatch(toggle, /background\s+\d+ms/);
  assert.match(cssRule(".theme-night-sky"), /opacity\s+\d+ms/);
  assert.doesNotMatch(styles, /@keyframes theme-(?:sun|moon)-(?:rise|set)/);
});

test("pointer theme changes preserve an active code editor caret", () => {
  assert.match(
    appSource,
    /button\.addEventListener\("pointerdown",[\s\S]*document\.activeElement[\s\S]*matches\("\.wq-editor"\)[\s\S]*event\.preventDefault\(\);/,
  );
});

test("midnight stars use quiet layers without flashing sparkles", () => {
  assert.match(appSource, /theme-stars-far/);
  assert.match(appSource, /theme-stars-mid/);
  assert.match(appSource, /theme-stars-near/);
  assert.doesNotMatch(appSource, /theme-star-sparkles/);
  assert.doesNotMatch(styles, /steps\(/);
});

test("welcome links use a quiet theme-aware hover border", () => {
  assert.equal(
    styles.match(/--welcome-link-border-hover:/g)?.length,
    2,
  );
  assert.equal(styles.match(/--welcome-link-bg-hover:/g)?.length, 2);
  assert.match(
    cssRule(".article-link:hover"),
    /border-color:\s*var\(--welcome-link-border-hover\);/,
  );
  assert.match(
    cssRule(".article-link:hover"),
    /background:\s*var\(--welcome-link-bg-hover\);/,
  );
  assert.match(
    cssRule(':root[data-theme="midnight"]'),
    /--welcome-link-bg:\s*rgba\(7, 9, 17, 0\.76\);/,
  );
  assert.doesNotMatch(styles, /--welcome-link-hover:\s*#54eaf5;/);
});
