import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import { createWqdbController } from "./wqdb.js";

function fakeStop() {
  let granularity = "expr";
  let breakpointLines = [];
  let trackers = [];
  return {
    pause: {
      id: "7",
      reason: "entry",
      location: {
        chunk: 1,
        pc: 0,
        function: "<main>",
        path: "<repl:1>",
        line: 1,
        column: 1,
        span: [0, 1],
      },
    },
    stack() {
      return [
        {
          frame: 0,
          function: "<main>",
          path: "<repl:1>",
          line: 1,
          column: 1,
          byte: 0,
          chunk: 1,
          pc: 0,
        },
        {
          frame: 1,
          function: "caller",
          path: "<repl:1>",
          line: 2,
          column: 1,
          byte: 2,
          chunk: 2,
          pc: 3,
        },
      ];
    },
    globals() {
      return [{ name: "answer", slot: null, display: "42", kind: "int" }];
    },
    locals(frame) {
      return [
        {
          name: `local${frame}`,
          slot: frame,
          display: String(frame),
          kind: "int",
        },
      ];
    },
    instruction() {
      return {
        pc: 0,
        opcode: "Push",
        operands: "1",
        annotations: [],
        class: "stack",
        is_special: false,
      };
    },
    granularity() {
      return granularity;
    },
    setGranularity(next) {
      granularity = next;
      return next;
    },
    setSourceBreakpoints(lines) {
      breakpointLines = [...lines];
      return breakpointLines.map((line, index) => ({
        id: index + 1,
        source_path: "<repl:1>",
        requested_line: line,
        chunk: null,
        pc: null,
      }));
    },
    trackSymbol(name) {
      const tracker = {
        id: trackers.length + 1,
        enabled: true,
        target: { scope: "global", name, chunk: null, slot: null },
      };
      trackers.push(tracker);
      return { added: true, tracker };
    },
    trackers() {
      return trackers;
    },
    removeTracker(id) {
      const previousLength = trackers.length;
      trackers = trackers.filter((tracker) => tracker.id !== id);
      return trackers.length !== previousLength;
    },
    clearTrackers() {
      trackers = [];
    },
  };
}

test("wqdb controller inspects, configures, and resumes a pause once", async () => {
  const states = [];
  const controller = createWqdbController((state) =>
    states.push({
      status: state.status,
      selectedFrame: state.selectedFrame,
      breakpointLines: [...state.breakpointLines],
    }),
  );
  const resume = controller.pause(fakeStop(), {
    source: "answer:42",
    sourcePath: "<repl:1>",
  });

  assert.equal(controller.state.status, "paused");
  assert.equal(controller.state.globals[0].kind, "int");
  assert.equal(controller.state.locals[0].name, "local0");

  controller.selectFrame(1);
  assert.equal(controller.state.locals[0].name, "local1");
  controller.setGranularity("line");
  assert.equal(controller.state.granularity, "line");
  controller.toggleBreakpoint(2);
  assert.deepEqual(controller.state.breakpointLines, [2]);
  assert.equal(controller.state.breakpoints[0].chunk, null);
  controller.trackSymbol("answer");
  assert.equal(controller.state.trackers[0].target.name, "answer");
  controller.removeTracker(1);
  assert.equal(controller.state.trackers.length, 0);

  assert.equal(controller.resume("step_over"), true);
  assert.equal(controller.resume("continue"), false);
  assert.equal(await resume, "step_over");
  assert.equal(controller.state.status, "running");
  const laterResume = controller.pause(fakeStop(), {
    source: "answer:42",
    sourcePath: "<repl:1>",
  });
  assert.deepEqual(controller.state.breakpointLines, [2]);
  controller.resume("continue");
  assert.equal(await laterResume, "continue");
  controller.finish();
  assert.equal(controller.state.status, "idle");
  assert.ok(states.length >= 7);
});

test("wqdb controller clears pending pauses and records mutations", async () => {
  const controller = createWqdbController();
  const resume = controller.pause(fakeStop(), {
    source: "answer:42",
    sourcePath: "<repl:1>",
  });
  controller.recordNotification({
    kind: "symbol_changed",
    tracker_id: 1,
    target: { scope: "global", name: "answer", chunk: null, slot: null },
    operation: "set",
    old_value: null,
    new_value: { name: null, slot: null, display: "42", kind: "int" },
  });
  assert.equal(controller.state.notifications.length, 1);

  controller.reset();
  assert.equal(await resume, "continue");
  assert.equal(controller.state.status, "idle");
  assert.equal(controller.state.notifications.length, 0);
});

test("wqdb controller keeps top-level pauses usable without captured locals", async () => {
  const stop = fakeStop();
  stop.locals = () => {
    throw new Error("debugger frame does not have captured locals");
  };
  const controller = createWqdbController();
  const resume = controller.pause(stop, {
    source: "answer:42",
    sourcePath: "<repl:1>",
  });

  assert.deepEqual(controller.state.locals, []);
  controller.resume("continue");
  assert.equal(await resume, "continue");
});

test("wqdb granularity uses custom buttons instead of a native select", async () => {
  const source = await readFile(new URL("./wqdb.js", import.meta.url), "utf8");
  assert.equal(source.includes('element("select"'), false);
  assert.match(source, /wqdb-granularity-option/);
  assert.match(source, /aria-pressed/);
});

test("wqdb granularity uses nested pills with distinct active hover styling", async () => {
  const styles = await readFile(
    new URL("../styles.css", import.meta.url),
    "utf8",
  );
  assert.match(
    styles,
    /\.wqdb-granularity-options\s*\{[^}]*padding:\s*3px;[^}]*border-radius:\s*var\(--radius-pill\)/s,
  );
  assert.match(
    styles,
    /\.wqdb-granularity-option:hover:not\(:disabled\):not\(\.active\)/,
  );
  assert.match(
    styles,
    /\.wqdb-granularity-option\.active:hover:not\(:disabled\)/,
  );
});

test("wqdb controls share a clear disabled treatment", async () => {
  const styles = await readFile(
    new URL("../styles.css", import.meta.url),
    "utf8",
  );
  assert.match(
    styles,
    /\.wqdb-panel-body :is\(button, input\):disabled\s*\{[^}]*cursor:\s*not-allowed;[^}]*opacity:/s,
  );
  assert.match(
    styles,
    /\.wqdb-panel-body \.btn:disabled,[\s\S]*?\.wqdb-track-input:disabled\s*\{[^}]*background:\s*var\(--surface-bg-muted\);[^}]*box-shadow:\s*none/s,
  );
});

test("wqdb centers its ready placeholder", async () => {
  const [source, styles] = await Promise.all([
    readFile(new URL("./wqdb.js", import.meta.url), "utf8"),
    readFile(new URL("../styles.css", import.meta.url), "utf8"),
  ]);
  assert.match(
    source,
    /body\.classList\.toggle\("is-empty", !state\.pause\);/,
  );
  assert.match(
    styles,
    /\.wqdb-panel-body\.is-empty\s*\{[^}]*align-items:\s*center;[^}]*justify-content:\s*center;/s,
  );
});

test("wqdb source uses highlighting with a background-only current line", async () => {
  const [source, styles] = await Promise.all([
    readFile(new URL("./wqdb.js", import.meta.url), "utf8"),
    readFile(new URL("../styles.css", import.meta.url), "utf8"),
  ]);
  assert.match(source, /highlightWq\(source\)/);
  assert.match(source, /highlightedLines/);
  assert.match(
    styles,
    /\.wqdb-source-code\s*\{[^}]*background:\s*var\(--surface-bg-field\);[^}]*color:\s*var\(--surface-text\)/s,
  );
  assert.match(
    styles,
    /\.wqdb-source-line\.active \.wqdb-source-code\s*\{[^}]*background:\s*color-mix\(/s,
  );
  assert.doesNotMatch(
    styles,
    /\.wqdb-source-line\.active(?:\s+\.wqdb-source-code)?\s*\{[^}]*box-shadow:/s,
  );
});

test("wqdb renders pretty-printer instruction parts with class styling", async () => {
  const source = await readFile(new URL("./wqdb.js", import.meta.url), "utf8");
  assert.match(source, /wqdb-instruction-opcode/);
  assert.match(source, /instruction\.class/);
  assert.match(source, /instruction\.is_special/);
  assert.match(source, /instruction\.operands/);
  assert.match(source, /instruction\.annotations/);
});

test("potentially long wqdb sections use foldable details", async () => {
  const source = await readFile(new URL("./wqdb.js", import.meta.url), "utf8");
  assert.match(source, /element\("details", "wqdb-section wqdb-foldable"\)/);
  assert.match(source, /element\("summary", "wqdb-section-title", title\)/);
  for (const title of ["Source", "Stack", "Locals", "Globals"]) {
    assert.match(source, new RegExp(`sectionOpen\\(foldState, "${title}"\\)`));
  }
});
