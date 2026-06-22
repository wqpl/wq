import test from "node:test";
import assert from "node:assert/strict";
import {
  buildFeaturedSearchIndex,
  searchFeaturedItems,
} from "./featured-search.js";

const tutorials = [
  {
    slug: "control-flow",
    title: "Control Flow",
    description: "Choose branches, loop with counters, and return early.",
    section: "wqpl",
    code: 'n:1;$[n>0;"+";"-"]',
  },
  {
    slug: "cas",
    title: "CAS",
    description: "Treat symbolic math as data with `@s` and CAS bfns.",
    section: "wqpl",
    code: "factor[@s x^2-1]",
  },
  {
    slug: "primes",
    title: "A Prime Sieve",
    description: "Walk through e/primes.wq one runnable composition at a time.",
    section: "wqpl",
    code: "x:30;p:x+1|iota|>1;where p",
  },
];

const docs = [
  {
    id: "map",
    kind: "builtin",
    title: "map builtin",
    group: "List",
    summary: "Apply a function to each item.",
    usage: "map[xs;f;d?]",
    aliases: ["map", "M"],
  },
  {
    id: "$",
    kind: "syntax",
    title: "$ conditional",
    group: "Syntax",
    summary: "Branch forms for ternary, guard, and condition chains.",
  },
];

test("searches symbolic tutorial code without losing punctuation", () => {
  const index = buildFeaturedSearchIndex({ tutorials, docs });
  const results = searchFeaturedItems(index, "@s");
  assert.equal(results[0].title, "CAS");
});

test("matches punctuation-insensitive multi-word tutorial queries", () => {
  const index = buildFeaturedSearchIndex({ tutorials, docs });
  const results = searchFeaturedItems(index, "control-flow");
  assert.equal(results[0].title, "Control Flow");
});

test("ranks exact tutorial titles for multi-word queries", () => {
  const index = buildFeaturedSearchIndex({ tutorials, docs });
  const results = searchFeaturedItems(index, "prime sieve");
  assert.equal(results[0].title, "A Prime Sieve");
});

test("indexes reference aliases and help commands", () => {
  const index = buildFeaturedSearchIndex({ tutorials, docs });
  const results = searchFeaturedItems(index, "help map");
  assert.equal(results[0].title, "map builtin");
  assert.equal(results[0].code, "map[xs;f;d?]");
  assert.equal(results[0].href, "article.html?slug=ref:map");
});

test("returns no results for an empty query", () => {
  const index = buildFeaturedSearchIndex({ tutorials, docs });
  assert.deepEqual(searchFeaturedItems(index, "   "), []);
});
