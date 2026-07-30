import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("app.js", import.meta.url), "utf8");
const tutorialSource = await readFile(
  new URL("tutorial.js", import.meta.url),
  "utf8"
);
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

test("code-fence edge actions use rounded rectangles and contain animated labels", () => {
  assert.match(
    styles,
    /\.code-action-btn\s*\{[^}]*min-height:\s*40px;[^}]*border-radius:\s*var\(--radius-control\);/
  );
  assert.match(styles, /\.code-action-btn\s*\{[^}]*overflow:\s*hidden;/);
});

test("action buttons use the prototype hierarchy", () => {
  assert.match(
    styles,
    /\.btn\s*\{[^}]*min-height:\s*42px;[^}]*padding:\s*9px 14px;[^}]*border-radius:\s*var\(--radius-control\);[^}]*background:\s*var\(--btn-bg\);/
  );
  assert.match(styles, /--btn-bg:\s*transparent;/);
  assert.match(
    styles,
    /\.btn\.primary\s*\{[^}]*background:\s*var\(--btn-primary-bg\);[^}]*box-shadow:\s*var\(--btn-primary-shadow\);/
  );
});

test("block code does not inherit inline-code margins or Midnight fills", () => {
  assert.match(
    styles,
    /\.article pre code,\s*\.run-result pre code\s*\{[^}]*margin:\s*0;[^}]*background:\s*transparent;/
  );
  assert.match(
    styles,
    /:root\[data-theme="midnight"\] \.article pre code\s*\{[^}]*background:\s*transparent;[^}]*border-color:\s*transparent;/
  );
});

test("standalone runnable fences share the grouped action hierarchy", () => {
  assert.match(tutorialSource, /className = "tutorial-single-cell"/);
  assert.match(
    styles,
    /\.code-wrapper\s*>\s*\.code-header\s*\.code-action-btn\[data-action="copy"\],[\s\S]*?background:\s*transparent;/
  );
  assert.match(
    styles,
    /\.code-wrapper[\s\S]*?\.code-action-btn\[data-action="run"\]:not\(\.code-action-danger\),[\s\S]*?background:\s*var\(--code-primary-bg\);/
  );
});

test("grouped tutorial cells use one toolbar and a numbered reading rail", () => {
  assert.match(tutorialSource, /className = "tutorial-cell-group-header"/);
  assert.match(tutorialSource, /className = "tutorial-cell-index"/);
  assert.match(tutorialSource, /classList\.add\("tutorial-cell-copy"\)/);
  assert.match(
    tutorialSource,
    /querySelector\("\.code-header"\)\?\.remove\(\)/
  );
  assert.match(tutorialSource, /dataset\.action = "copy-all"/);
  assert.doesNotMatch(tutorialSource, /cellHeaderLabel/);
  assert.match(tutorialSource, /view\.head\.hidden = heading === "Result"/);
  assert.doesNotMatch(
    tutorialSource,
    /total > 1[\s\S]{0,120}createOutputBar\("info"\)/
  );
  assert.match(
    styles,
    /\.tutorial-cell\s*\{[^}]*grid-template-columns:\s*52px minmax\(0, 1fr\);/
  );
  assert.match(
    styles,
    /\.tutorial-cell-group \.code-wrapper \+ \.run-result\s*\{[^}]*border-top:\s*1px solid var\(--code-group-rule\);/
  );
});

test("the web book comes from the shared catalog and keeps chapter navigation", () => {
  assert.match(source, /fetch\("book\/catalog\.json"\)/);
  assert.match(
    source,
    /file: `book\/\$\{chapter\.file\}`[\s\S]*bookOrder: index/
  );
  const article = templateSource("ARTICLE_HTML");
  assert.match(article, /data-role="article-sequence"/);
  assert.match(article, /data-role="previous-chapter"/);
  assert.match(article, /data-role="next-chapter"/);
});
