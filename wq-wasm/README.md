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

## Maintainer release

From `wq-wasm`, run the release CLI without an argument to increment the current
preview number:

```sh
npm run release
```

Pass an explicit version when the next release changes the version line:

```sh
npm run release -- 0.10.0-preview1
```

The command requires a clean worktree. It updates the workspace, lockfile, and
npm versions, runs the Rust and browser package checks, creates a release commit
and annotated `v<version>` tag, then asks before pushing. The atomic GitHub push
is the publish boundary and triggers the npm publishing workflow. The CLI never
runs `npm publish` locally. Before accepting the push, ensure the package exists
on npm and its trusted publisher is configured for `publish-wq-wasm.yml`.
