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
  const result = await session.eval_wq_async("1+1");
  console.log(result.display);
} finally {
  session.free();
}
```

The package targets browsers and exposes the stable session facade from
`browser.js`. TypeScript declarations are included.

`eval_wq_async` runs the selected interpreter in bounded work slices and
yields to the browser between slices. It accepts an `AbortSignal` and an
optional target slice duration in milliseconds:

```js
const controller = new AbortController();
const result = await session.eval_wq_async(source, {
  signal: controller.signal,
  timeSliceMs: 8,
});
```

Only one evaluation can use a session at a time. Aborting an evaluation keeps
bindings completed before cancellation and leaves the session ready for another
evaluation. `eval_wq` remains available for callers that require synchronous
execution. Higher-order builtins, custom CLI parsers, callable `asciiplot`
sampling, and every interpreter kind use the same yielding VM loop.
Context algorithms and callback forms without resumable VM state still execute
as one work unit and can take longer than the requested slice duration.

## Debugger

Enable wqdb on a session and provide a pause handler to inspect and resume
cooperative evaluations:

```js
session.set_wqdb_mode(true);

const result = await session.eval_wq_async(
  "answer:40+2\n@p answer",
  {
    sourcePath: "scratch.wq",
    onDebuggerPause(stop) {
      console.log(stop.pause.reason, stop.pause.location);
      console.table(stop.stack());
      console.table(stop.globals());
      console.log(stop.instruction());
      return "continue";
    },
  },
);
```

The handler can return `continue`, `step_in`, `step_over`, or `step_out`,
either directly or through a Promise. A debugger stop also exposes frame
locals, source-line breakpoints, step granularity, symbol trackers, and
mutation notifications. Debugger inspection values use the representation
oriented `kind` field.

Call `arm_wqdb_next()` before a later evaluation when persistent wqdb should
pause at entry again. Debugger stop methods are valid only while that stop is
active. Calling one after the handler resumes, aborts, or completes throws a
stale-pause error. An enabled wqdb evaluation without `onDebuggerPause`
continues automatically, so interfaces that do not expose debugger controls do
not surface an internal pause error.

## Language frontend

`WasmFrontend` provides evaluator-free language tooling for editors, REPLs, and
documentation views:

```js
import init, { WasmFrontend } from "wq-wasm";

await init();

const frontend = new WasmFrontend();
try {
  const source = "f:{[x]\n  x+1";
  console.log(frontend.is_complete_input(source)); // false
  console.log(frontend.diagnostics(source));
  console.log(frontend.analyze_symbols(source));
  console.log(frontend.highlight_spans(source));
  console.log(frontend.highlight_wq(source));
  console.log(frontend.cursor_context_at(source, 4));
  console.log(frontend.format_wq("(1;2)|has?@1[2]"));
} finally {
  frontend.free();
}
```

Source spans and cursor offsets use half-open UTF-8 byte offsets. Convert DOM,
textarea, Monaco, or CodeMirror UTF-16 offsets before passing them to an API
that accepts a cursor offset. `highlight_wq` returns escaped HTML for direct
rendering. `highlight_spans` returns editor-neutral structured spans.

During `eval_wq_async`, the stdin callback can return a Promise. Evaluation
suspends until the callback supplies a string or reports end-of-file with
`null` or `undefined`.

## Maintainer release

From the workspace root, run the release CLI without an argument to increment
the current preview number:

```sh
python3 publish-scripts/release.py
```

Pass an explicit version when the next release changes the version line:

```sh
python3 publish-scripts/release.py 0.10.0-preview1
```

The command requires a clean worktree. It updates the workspace, lockfile, and
npm versions, runs the Rust and browser package checks, creates a release commit
and annotated `v<version>` tag, then asks before pushing. The atomic GitHub push
is the publish boundary and triggers the npm publishing workflow. The CLI never
runs `npm publish` locally. Before accepting the push, ensure the package exists
on npm and its trusted publisher is configured for `publish-wq-wasm.yml`.
See [the publishing scripts guide](https://github.com/wqpl/wq/blob/main/publish-scripts/README.md)
for setup, command details, example workflows, and failure recovery.
