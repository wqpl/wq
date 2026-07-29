import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const appSource = await readFile(new URL("app.js", import.meta.url), "utf8");
const replSource = await readFile(new URL("repl.js", import.meta.url), "utf8");
const styles = await readFile(new URL("../styles.css", import.meta.url), "utf8");

test("REPL input is an inline terminal prompt", () => {
  assert.match(
    appSource,
    /id="term"[\s\S]*id="terminalOutput"[\s\S]*id="promptForm" class="repl-live-input"[\s\S]*class="repl-live-prompt"/,
  );
});

test("terminal chrome keeps established pills and utilities outside the prompt", () => {
  assert.match(appSource, /id="terminalStatus"[\s\S]*id="pillBox"[\s\S]*id="pillTime"[\s\S]*id="debugToggle"/);
  assert.match(appSource, /id="inspectorToggleBtn"[\s\S]*id="terminalMenu"/);
  assert.match(appSource, /id="pillBox"[\s\S]*class="pill inactive"/);
  assert.match(appSource, /id="historyToggleBtn"[\s\S]*class="pill inactive"/);
  assert.match(appSource, /id="scrollLatestBtn"/);
  assert.match(
    appSource,
    /class="repl-terminal-menu-ellipsis" aria-hidden="true"\s*>…<\/span/,
  );
  assert.match(
    styles,
    /:root\s*\{[\s\S]*--terminal-bg:\s*#fbfdff;[\s\S]*--terminal-text:\s*#183747;[\s\S]*\}/,
  );
  assert.match(
    styles,
    /:root\[data-theme="midnight"\]\s*\{[\s\S]*--terminal-bg:\s*#080d16;[\s\S]*--terminal-text:\s*#e7eef6;[\s\S]*\}/,
  );
  assert.match(
    styles,
    /\.repl-terminal-menu-ellipsis\s*\{[^}]*font-weight:\s*500;/,
  );
  assert.match(styles, /\.repl-status-dot/);
  assert.match(
    appSource,
    /class="repl-status-dot"[\s\S]*id="terminalStatus" class="repl-terminal-status">Ready<\/span>/
  );
  assert.doesNotMatch(appSource, /<h2>wq repl<\/h2>/);
  assert.match(
    styles,
    /\.repl-terminal-status\s*\{[^}]*font-family:\s*var\(--font-display\);/
  );
  for (const label of [
    "Ready",
    "Waiting for input",
    "Paused",
    "Stopping",
    "Running"
  ]) {
    assert.match(replSource, new RegExp(`\\? "${label}"|: "${label}"`));
  }
  assert.match(
    styles,
    /\.repl-terminal-bar \.runtime-panel-head \.mini\s*\{[^}]*color:\s*var\(--terminal-text\);/,
  );
});

test("submitted input advances immediately and terminal shortcuts remain available", () => {
  assert.match(
    replSource,
    /createTurn\("input"[\s\S]*execCounter\+\+;[\s\S]*syncLivePrompt\(\);[\s\S]*ui\.codeEl\.value = "";/,
  );
  assert.match(replSource, /function canNavigateHistory\(direction\)/);
  assert.match(replSource, /e\.key === "l"[\s\S]*clearScreen\(\{ focusInput: true \}\)/);
  assert.match(replSource, /e\.key === "Escape" && evaluationController\.active/);
  assert.match(
    replSource,
    /ui\.terminalMenu\?\.open && !ui\.terminalMenu\.contains\(e\.target\)/,
  );
});

test("clear screen keeps the first input aligned with its submitted row", () => {
  assert.match(
    replSource,
    /function clearScreen\([\s\S]*ui\.output\.replaceChildren\(\);/,
  );
  assert.match(
    styles,
    /\.repl-output-log:empty \+ \.repl-live-input\s*\{[^}]*margin-top:\s*0;/,
  );
});

test("live and submitted input use the same one-character prompt gap", () => {
  assert.match(
    styles,
    /\.repl-live-input-row\s*\{[^}]*gap:\s*0;/,
  );
  assert.match(
    styles,
    /\.repl-live-prompt\s*\{[^}]*margin-right:\s*1ch;/,
  );
});

test("desktop inspector keeps the same minimum height as the REPL", () => {
  assert.match(
    styles,
    /\.repl-flow\s*\{[^}]*min-height:\s*440px;/,
  );
  assert.match(
    styles,
    /\.globals-panel\s*\{[^}]*min-height:\s*440px;/,
  );
});

test("green inspector controls use local surface tokens", () => {
  assert.match(
    styles,
    /\.globals-panel \.btn\s*\{[^}]*border-color:\s*var\(--inspector-control-border\);[^}]*background:\s*var\(--inspector-control-bg\);[^}]*color:\s*var\(--inspector-control-text\);/
  );
  assert.match(
    styles,
    /\.inspector-tab\.active\s*\{[^}]*background:\s*var\(--inspector-control-active-bg\);[^}]*color:\s*var\(--inspector-control-active-text\);/
  );
  assert.match(
    styles,
    /\.wqdb-granularity-option\.active\s*\{[^}]*border-color:\s*var\(--inspector-control-active-border\);[^}]*background:\s*var\(--inspector-control-active-bg\);/
  );
  assert.match(
    styles,
    /\.wqdb-panel-status\s*\{[^}]*font-family:\s*var\(--font-body\);/
  );
});

test("latest output waits for sustained distance from the transcript end", () => {
  assert.match(replSource, /const LATEST_OUTPUT_REVEAL_DELAY_MS = \d+;/);
  assert.match(replSource, /function scheduleLatestOutputReveal\(\)/);
  assert.match(
    replSource,
    /window\.setTimeout\([\s\S]*LATEST_OUTPUT_REVEAL_DELAY_MS/,
  );
  assert.match(
    replSource,
    /if \(nearBottom\) \{[\s\S]*hideLatestOutput\(\);[\s\S]*\} else \{[\s\S]*scheduleLatestOutputReveal\(\);/,
  );
});
