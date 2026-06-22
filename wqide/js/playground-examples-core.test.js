import assert from "node:assert/strict";
import test from "node:test";
import {
  PLAYGROUND_EXAMPLE_DEFINITIONS,
  createPlaygroundExamples,
  findPlaygroundExample,
} from "./playground-examples-core.js";

test("playground examples point at curated e scripts", () => {
  assert.deepEqual(
    PLAYGROUND_EXAMPLE_DEFINITIONS.map((example) => example.sourcePath),
    ["@e/nq.wq", "@e/primes.wq", "@e/cowsay.wq", "@e/gol.wq"],
  );
  assert.equal(
    new Set(PLAYGROUND_EXAMPLE_DEFINITIONS.map((example) => example.id)).size,
    PLAYGROUND_EXAMPLE_DEFINITIONS.length,
  );
});

test("playground examples are built from source registry entries", () => {
  const examples = createPlaygroundExamples({
    nq: "nq source",
    primes: "primes source",
    cowsay: "cowsay source",
    gol: "gol source",
  });

  assert.equal(findPlaygroundExample(examples, "primes").code, "primes source");
  assert.equal(findPlaygroundExample(examples, "nq").code, "nq source");
  assert.equal(findPlaygroundExample(examples, "cowsay").stdin, "");
});

test("playground example registry rejects missing source entries", () => {
  assert.throws(
    () => createPlaygroundExamples({}),
    /Missing playground example source for @e\/nq\.wq/,
  );
});
