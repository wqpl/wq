import { createPlaygroundExamples } from "./playground-examples-core.js";

const SOURCE_ROOT = "../../e/";
const sourceModules = import.meta.glob("../../e/**/*.wq", {
  eager: true,
  import: "default",
  query: "?raw"
});

function createSourceRegistry() {
  return new Map(
    Object.entries(sourceModules).map(([path, source]) => [
      path.slice(SOURCE_ROOT.length),
      source
    ])
  );
}

export function loadPlaygroundExamples() {
  const sources = createSourceRegistry();
  return {
    examples: createPlaygroundExamples(sources),
    sources
  };
}
