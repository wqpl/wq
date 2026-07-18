# wq-wasm

Browser WebAssembly bindings for the [wq programming language](https://wq-pl.com).

## Install

Install the current preview release:

```sh
npm install wq-wasm@preview
```

## Use

```js
import init, { WasmWqSession } from "wq-wasm";

await init();

const session = new WasmWqSession();
try {
  const result = session.eval_wq("1+1");
  console.log(result.display);
} finally {
  session.free();
}
```

The package targets browsers and exposes the stable session facade from
`browser.js`. TypeScript declarations are included.

