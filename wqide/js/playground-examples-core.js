export const PLAYGROUND_EXAMPLE_DEFINITIONS = [
  {
    id: "nq",
    title: "N-Queens",
    description: "Count and list small queen placements.",
    sourcePath: "@e/nq.test.wq",
  },
  {
    id: "primes",
    title: "Primes",
    description: "Generate the prime numbers below 100.",
    sourcePath: "@e/primes.test.wq",
  },
  {
    id: "cowsay",
    title: "Cowsay",
    description: "Print a classic terminal speech bubble.",
    sourcePath: "@e/cowsay.test.wq",
  },
  {
    id: "gol",
    title: "Game of Life",
    description: "Step a small cellular automaton grid.",
    sourcePath: "@e/gol.test.wq",
  },
];

export function inlineImportedExample(implementation, test) {
  const newline = test.indexOf("\n");
  const firstLine = newline === -1 ? test : test.slice(0, newline);
  if (!/^[^;]+:@i(?:@l)?".+\.wq"(?:;.*)?$/.test(firstLine)) {
    throw new Error("Expected example test to start with a wq import");
  }
  const testBody = newline === -1 ? "" : test.slice(newline + 1);
  return `${implementation.trimEnd()}\n\n${testBody.trimStart()}`;
}

export function createPlaygroundExamples(sources) {
  return PLAYGROUND_EXAMPLE_DEFINITIONS.map((definition) => {
    const code = sources[definition.id];
    if (typeof code !== "string") {
      throw new Error(
        `Missing playground example source for ${definition.sourcePath}`,
      );
    }
    return {
      ...definition,
      code,
    };
  });
}

export function findPlaygroundExample(examples, id) {
  return examples.find((example) => example.id === id);
}
