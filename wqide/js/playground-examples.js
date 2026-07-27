import cowsayCode from "../../e/cowsay.wq?raw";
import cowsayTestCode from "../../e/cowsay.test.wq?raw";
import golCode from "../../e/gol.wq?raw";
import golTestCode from "../../e/gol.test.wq?raw";
import nqCode from "../../e/nq.wq?raw";
import nqTestCode from "../../e/nq.test.wq?raw";
import primesCode from "../../e/primes.wq?raw";
import primesTestCode from "../../e/primes.test.wq?raw";
import {
  createPlaygroundExamples,
  findPlaygroundExample,
  inlineImportedExample,
} from "./playground-examples-core.js";

export const PLAYGROUND_EXAMPLES = createPlaygroundExamples({
  cowsay: inlineImportedExample(cowsayCode, cowsayTestCode),
  gol: inlineImportedExample(golCode, golTestCode),
  nq: inlineImportedExample(nqCode, nqTestCode),
  primes: inlineImportedExample(primesCode, primesTestCode),
});

export function getPlaygroundExample(id) {
  return findPlaygroundExample(PLAYGROUND_EXAMPLES, id);
}
