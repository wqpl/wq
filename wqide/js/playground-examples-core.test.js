import assert from "node:assert/strict";
import test from "node:test";
import {
  PLAYGROUND_EXAMPLE_DEFINITIONS,
  createPlaygroundExamples,
  findPlaygroundExample,
  inlineImportedExample,
} from "./playground-examples-core.js";

test("playground examples point at curated e scripts", () => {
  assert.deepEqual(
    PLAYGROUND_EXAMPLE_DEFINITIONS.map((example) => example.sourcePath),
    [
      "@e/nq.test.wq",
      "@e/primes.test.wq",
      "@e/cowsay.test.wq",
      "@e/gol.test.wq",
    ],
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
});

test("playground example registry rejects missing source entries", () => {
  assert.throws(
    () => createPlaygroundExamples({}),
    /Missing playground example source for @e\/nq\.test\.wq/,
  );
});

test("imported example tests are inlined for the browser", () => {
  assert.equal(
    inlineImportedExample(
      "double:{2*x}\ndouble\n",
      'double:@i"double.wq"\nassert_eq[double 2;4]',
    ),
    "double:{2*x}\ndouble\n\nassert_eq[double 2;4]",
  );
  assert.throws(
    () => inlineImportedExample("double:{2*x}", "assert_eq[double 2;4]"),
    /start with a wq import/,
  );
});
