import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { resolveRoute } from "./route.js";

const entryRoutes = [
  ["playground.html", "playground"],
  ["viz.html", "viz"],
  ["repl.html", "repl"],
  ["more.html", "more"]
];

test("every primary direct entry resolves before and after its redirect", async () => {
  assert.equal(resolveRoute("/index.html").key, "featured");

  for (const [entry, key] of entryRoutes) {
    const html = await readFile(new URL(`../${entry}`, import.meta.url), "utf8");
    assert.match(html, new RegExp(`route=${entry.replace(".", "\\.")}`));
    assert.equal(resolveRoute(`/${entry}`).key, key);
    assert.equal(resolveRoute("/index.html", `?route=${entry}`).key, key);
  }
});

test("article and folder direct entries preserve their identifiers", () => {
  const article = resolveRoute("/article.html", "?slug=arithmetic");
  assert.equal(article.key, "article:arithmetic");
  assert.equal(article.area, "featured");

  const folder = resolveRoute("/subfolder.html", "?section=Reference");
  assert.equal(folder.key, "subfolder:Reference");
  assert.equal(folder.area, "featured");
});

test("redirect-only route parameters do not leak into mounted route state", () => {
  const route = resolveRoute(
    "/index.html",
    "?route=playground.html&template=gol"
  );
  assert.equal(route.key, "playground");
  assert.equal(route.params.has("route"), false);
  assert.equal(route.params.get("template"), "gol");
});
