export const PLAYGROUND_EXAMPLE_DEFINITIONS = [
  {
    id: "nq",
    title: "N-Queens",
    description: "Count and list small queen placements.",
    sourcePath: "@e/nq.wq",
    stdin: "",
  },
  {
    id: "primes",
    title: "Primes",
    description: "Generate the prime numbers below 100.",
    sourcePath: "@e/primes.wq",
    stdin: "",
  },
  {
    id: "cowsay",
    title: "Cowsay",
    description: "Print a classic terminal speech bubble.",
    sourcePath: "@e/cowsay.wq",
    stdin: "",
  },
  {
    id: "gol",
    title: "Game of Life",
    description: "Step a small cellular automaton grid.",
    sourcePath: "@e/gol.wq",
    stdin: "",
  },
];

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
      stdin: definition.stdin || "",
    };
  });
}

export function findPlaygroundExample(examples, id) {
  return examples.find((example) => example.id === id);
}
