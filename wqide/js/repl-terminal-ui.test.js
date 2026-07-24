import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const appSource = await readFile(new URL("app.js", import.meta.url), "utf8");
const replSource = await readFile(new URL("repl.js", import.meta.url), "utf8");
const styles = await readFile(new URL("../styles.css", import.meta.url), "utf8");

test("REPL input is an inline terminal prompt rather than a composer", () => {
  assert.match(
    appSource,
    /id="term"[\s\S]*id="terminalOutput"[\s\S]*id="promptForm" class="repl-live-input"[\s\S]*class="repl-live-prompt"/,
  );
  assert.doesNotMatch(appSource, /class="[^"]*\b(?:repl-composer|composer-frame|composer-actions)\b/);
  assert.doesNotMatch(appSource, /id="evalBtn"|repl-live-input-meta|Enter to run/);
  assert.doesNotMatch(styles, /\.repl-composer\b|\.composer-frame\b|\.composer-actions\b/);
});

test("terminal chrome keeps established pills and utilities outside the prompt", () => {
  assert.match(appSource, /id="terminalStatus"[\s\S]*id="pillBox"[\s\S]*id="pillTime"[\s\S]*id="debugToggle"/);
  assert.match(appSource, /id="inspectorToggleBtn"[\s\S]*id="terminalMenu"/);
  assert.match(appSource, /id="pillBox"[\s\S]*class="pill inactive"/);
  assert.match(appSource, /id="historyToggleBtn"[\s\S]*class="pill inactive"/);
  assert.doesNotMatch(appSource, /class="repl-terminal-action(?:\s|")/);
  assert.match(appSource, /id="scrollLatestBtn"/);
  assert.match(styles, /--terminal-bg:\s*#0b1119;/);
  assert.match(styles, /\.repl-status-dot/);
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
