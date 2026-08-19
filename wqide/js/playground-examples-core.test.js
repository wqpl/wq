import assert from "node:assert/strict";
import test from "node:test";
import {
  PLAYGROUND_EXAMPLE_DEFINITIONS,
  createPlaygroundEvaluation,
  createPlaygroundExamples,
  findPlaygroundExample
} from "./playground-examples-core.js";

test("playground examples point at curated e scripts", () => {
  assert.deepEqual(
    PLAYGROUND_EXAMPLE_DEFINITIONS.map((example) => example.sourcePath),
    [
      "@e/primes.test.wq",
      "@e/nq.test.wq",
      "@e/sudoku.test.wq",
      "@e/fft.test.wq",
      "@e/cowsay.test.wq",
      "@e/gol.test.wq",
    ],
  );
  assert.equal(
    new Set(PLAYGROUND_EXAMPLE_DEFINITIONS.map((example) => example.id)).size,
    PLAYGROUND_EXAMPLE_DEFINITIONS.length
  );
});

function exampleSources() {
  return new Map(
    PLAYGROUND_EXAMPLE_DEFINITIONS.flatMap((definition) => [
      [definition.initialPath, `${definition.id} implementation`],
      [
        definition.entryPath,
        `${definition.id}:@i"${definition.initialPath}"\n${definition.id}[]`
      ]
    ])
  );
}

test("playground examples retain their implementation and entry files", () => {
  const examples = createPlaygroundExamples(exampleSources());
  const primes = findPlaygroundExample(examples, "primes");

  assert.equal(primes.files.get("primes.wq"), "primes implementation");
  assert.equal(
    primes.files.get("primes.test.wq"),
    'primes:@i"primes.wq"\nprimes[]'
  );
});

test("playground example registry rejects missing source entries", () => {
  assert.throws(
    () => createPlaygroundExamples(new Map()),
    /Missing playground example source primes\.wq for Primes/
  );
});

test("playground evaluation preserves the real entry source and modules", () => {
  const sources = exampleSources();
  const example = findPlaygroundExample(
    createPlaygroundExamples(sources),
    "gol"
  );
  example.files.set("gol.wq", "edited implementation");

  const evaluation = createPlaygroundEvaluation(example, sources);

  assert.equal(evaluation.sourcePath, "gol.test.wq");
  assert.equal(
    evaluation.source,
    'gol:@i"gol.wq"\ngol[]'
  );
  assert.equal(evaluation.modules.get("gol.wq"), "edited implementation");
});
