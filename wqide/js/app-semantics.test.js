import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("app.js", import.meta.url), "utf8");
const styles = await readFile(
  new URL("../styles.css", import.meta.url),
  "utf8"
);

function templateSource(name) {
  const marker = `const ${name} = html\``;
  const start = source.indexOf(marker);
  assert.notEqual(start, -1, `${name} should exist`);
  const contentStart = start + marker.length;
  const end = source.indexOf("\n`;", contentStart);
  assert.notEqual(end, -1, `${name} should have a closing template delimiter`);
  return source.slice(contentStart, end);
}

test("the application owns one main landmark", () => {
  assert.equal((source.match(/<main\b/g) || []).length, 1);
  assert.match(templateSource("SHELL_HTML"), /<main id="appMain"><\/main>/);

  for (const name of [
    "FEATURED_HTML",
    "PLAYGROUND_HTML",
    "VIZ_HTML",
    "REPL_HTML",
    "MORE_HTML",
    "SUBFOLDER_HTML",
    "ARTICLE_HTML"
  ]) {
    assert.doesNotMatch(templateSource(name), /<main\b/);
  }
});

test("primary workbench routes avoid standalone page-title rows", () => {
  for (const name of [
    "FEATURED_HTML",
    "PLAYGROUND_HTML",
    "VIZ_HTML",
    "REPL_HTML",
    "MORE_HTML"
  ]) {
    const headings = templateSource(name).match(/<h1\b/g) || [];
    assert.equal(headings.length, 0, `${name} should not provide an h1`);
  }

  for (const name of ["SUBFOLDER_HTML", "ARTICLE_HTML"]) {
    const headings = templateSource(name).match(/<h1\b/g) || [];
    assert.equal(headings.length, 1, `${name} should retain its content h1`);
  }
});

test("primary navigation uses links and current-page semantics", () => {
  const shell = templateSource("SHELL_HTML");
  assert.match(shell, /<a class="brand" href="index\.html"/);
  assert.match(shell, /<nav class="tabs" aria-label="Primary">/);
  assert.doesNotMatch(shell, /role="tablist"/);
  assert.match(shell, /data-nav="featured">Home<\/a>/);
  assert.match(source, /setAttribute\("aria-current", "page"\)/);
});

test("button groups are not exposed as incomplete lists", () => {
  assert.doesNotMatch(source, /role="list"/);
});

test("search fields retain names when their placeholders disappear", () => {
  assert.match(
    templateSource("FEATURED_HTML"),
    /id="featuredSearchInput"[\s\S]*aria-labelledby="featuredSearchHeading"/
  );
  assert.match(
    templateSource("REPL_HTML"),
    /<label class="visually-hidden" for="historySearchInput">[\s\S]*Search REPL history/
  );
});

test("interactive chrome does not create accidental text selections", () => {
  assert.match(
    styles,
    /button,\s*summary,[\s\S]*?\.card:has\(\.stretched\)\s*\{[^}]*user-select:\s*none;[^}]*-webkit-user-select:\s*none;/
  );
});

test("code-fence edge actions use rounded rectangles", () => {
  assert.match(
    styles,
    /\.code-action-btn\s*\{[^}]*border-radius:\s*var\(--radius-xs\);/
  );
});
