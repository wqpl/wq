import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const appSource = await readFile(new URL("./app.js", import.meta.url), "utf8");
const styles = await readFile(
  new URL("../styles.css", import.meta.url),
  "utf8",
);

function styleRule(selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = styles.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`));
  assert.ok(match, `missing style rule for ${selector}`);
  return match[1];
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
