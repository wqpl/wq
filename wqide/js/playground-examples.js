import cowsayCode from "../../e/cowsay.wq?raw";
import golCode from "../../e/gol.wq?raw";
import nqCode from "../../e/nq.wq?raw";
import primesCode from "../../e/primes.wq?raw";
import {
  createPlaygroundExamples,
  findPlaygroundExample,
} from "./playground-examples-core.js";

export const PLAYGROUND_EXAMPLES = createPlaygroundExamples({
  cowsay: cowsayCode,
  gol: golCode,
  nq: nqCode,
  primes: primesCode,
});

export function getPlaygroundExample(id) {
  return findPlaygroundExample(PLAYGROUND_EXAMPLES, id);
}
