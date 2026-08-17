export const PLAYGROUND_EXAMPLE_DEFINITIONS = [
  {
    id: "primes",
    title: "Primes",
    description: "Generate the prime numbers.",
    entryPath: "primes.test.wq",
    initialPath: "primes.wq",
    sourcePath: "@e/primes.test.wq"
  },
  {
    id: "nq",
    title: "N-Queens",
    description: "Count and list queen placements.",
    entryPath: "nq.test.wq",
    initialPath: "nq.wq",
    sourcePath: "@e/nq.test.wq"
  },
  {
    id: "sudoku",
    title: "Sudoku",
    description: "Solve a classic 9x9 Sudoku puzzle.",
    entryPath: "sudoku.test.wq",
    initialPath: "sudoku.wq",
    sourcePath: "@e/sudoku.test.wq"
  },
  {
    id: "cowsay",
    title: "Cowsay",
    description: "Print a classic terminal speech bubble.",
    entryPath: "cowsay.test.wq",
    initialPath: "cowsay.wq",
    sourcePath: "@e/cowsay.test.wq"
  },
  {
    id: "gol",
    title: "Game of Life",
    description: "Step a small cellular automaton grid.",
    entryPath: "gol.test.wq",
    initialPath: "gol.wq",
    sourcePath: "@e/gol.test.wq"
  }
];

function requireSource(sources, definition, path) {
  const source = sources.get(path);
  if (typeof source !== "string") {
    throw new Error(
      `Missing playground example source ${path} for ${definition.title}`
    );
  }
  return source;
}

export function createPlaygroundExamples(sources) {
  return PLAYGROUND_EXAMPLE_DEFINITIONS.map((definition) => {
    return {
      ...definition,
      files: new Map([
        [
          definition.initialPath,
          requireSource(sources, definition, definition.initialPath)
        ],
        [
          definition.entryPath,
          requireSource(sources, definition, definition.entryPath)
        ]
      ])
    };
  });
}

export function findPlaygroundExample(examples, id) {
  return examples.find((example) => example.id === id);
}

export function createPlaygroundEvaluation(example, sources) {
  const modules = new Map(sources);
  for (const [path, source] of example.files) {
    modules.set(path, source);
  }
  return {
    modules,
    source: requireSource(example.files, example, example.entryPath),
    sourcePath: example.entryPath
  };
}
