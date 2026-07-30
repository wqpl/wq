import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const appSource = await readFile(new URL("app.js", import.meta.url), "utf8");
const replSource = await readFile(new URL("repl.js", import.meta.url), "utf8");
const styles = await readFile(new URL("../styles.css", import.meta.url), "utf8");
const sharedSource = await readFile(
  new URL("wq-shared.js", import.meta.url),
  "utf8"
);

test("REPL input is an inline terminal prompt", () => {
  assert.match(
    appSource,
    /id="term"[\s\S]*id="terminalOutput"[\s\S]*id="promptForm" class="repl-live-input"[\s\S]*class="repl-live-prompt"/,
  );
});

test("terminal chrome separates runtime toggles from utility actions", () => {
  assert.match(appSource, /id="terminalStatus"[\s\S]*id="pillBox"[\s\S]*id="pillTime"[\s\S]*id="debugToggle"/);
  assert.match(appSource, /id="inspectorToggleBtn"[\s\S]*id="terminalMenu"/);
  assert.match(appSource, /id="pillBox"[\s\S]*class="pill inactive runtime-segment"/);
  assert.match(appSource, /id="historyToggleBtn"[\s\S]*class="pill inactive repl-utility-button"/);
  assert.match(appSource, /id="historyToggleBtn"[\s\S]*>\s*History\s*<\/button>/);
  assert.match(appSource, /id="inspectorToggleBtn"[\s\S]*<span>Inspector<\/span>/);
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
  assert.match(
    appSource,
    /class="repl-runtime-actions runtime-toggle-cluster"[\s\S]*?class="repl-view-actions"[\s\S]*?class="repl-session-actions"/
  );
  assert.match(
    styles,
    /\.runtime-toggle-cluster\s*\{[^}]*gap:\s*2px;[^}]*border:\s*1px solid var\(--control-cluster-border\);[^}]*border-radius:\s*var\(--radius-surface\);[^}]*background:\s*var\(--control-cluster-bg\);/s
  );
  assert.match(
    styles,
    /\.runtime-segment\.active\s*\{[^}]*border-color:\s*transparent;[^}]*background:\s*var\(--btn-primary-bg\);[^}]*color:\s*var\(--btn-primary-text\);/s
  );
  assert.match(
    appSource,
    /id="pillBox"[\s\S]*class="pill inactive runtime-segment"[\s\S]*aria-pressed="false"/
  );
  assert.match(
    sharedSource,
    /classList\.contains\("runtime-segment"\)[\s\S]*setAttribute\("aria-pressed", String\(!!on\)\)/
  );
  assert.match(
    styles,
    /\.repl-view-actions,\s*\.repl-session-actions\s*\{[^}]*padding:\s*0;[^}]*border:\s*0;[^}]*background:\s*transparent;/s
  );
  assert.match(
    styles,
    /\.repl-terminal-bar \.repl-utility-button\s*\{[^}]*border-color:\s*var\(--code-action-border\);[^}]*border-radius:\s*var\(--radius-md\);[^}]*background:\s*transparent;/s
  );
  assert.match(
    styles,
    /\.repl-terminal-bar \.repl-utility-button\.active,[\s\S]*?\.repl-terminal-menu\[open\] > \.repl-utility-button\s*\{[^}]*border-color:\s*var\(--code-action-border\);[^}]*background:\s*var\(--utility-active-bg\);[^}]*color:\s*var\(--utility-active-text\);/s
  );
  assert.match(
    styles,
    /\.repl-terminal-count\s*\{[^}]*min-width:\s*18px;[^}]*height:\s*18px;[^}]*border-radius:\s*var\(--radius-pill\);[^}]*background:\s*var\(--workbench-rail-bg\);/s
  );
  assert.match(
    styles,
    /:root\[data-theme="midnight"\]\s*\{[^}]*--utility-hover-bg:\s*#38284f;[^}]*--utility-active-bg:\s*#493461;[^}]*--utility-active-hover-bg:\s*#563c71;[^}]*--utility-active-text:\s*#eee8fa;/s
  );
  assert.match(styles, /\.btn\[hidden\]\s*\{[^}]*display:\s*none;/);
});

test("REPL pills and inspector tabs share smooth hover feedback", () => {
  assert.match(
    styles,
    /\.pill\s*\{[^}]*background-color 140ms ease,[^}]*box-shadow 140ms ease;/s
  );
  assert.match(
    styles,
    /\.pill\.inactive:hover\s*\{[^}]*background:\s*var\(--btn-hover-bg\);[^}]*border-color:\s*var\(--pill-hover-border\);/s
  );
  assert.match(
    styles,
    /\.pill\.active:hover\s*\{[^}]*border-color:\s*var\(--pill-active-border\);/s
  );
  assert.match(
    styles,
    /\.pill\.active:focus-visible\s*\{[^}]*outline-color:\s*var\(--pill-active-border\);/s
  );
  assert.match(
    appSource,
    /class="inspector-tabs segmented-control"[\s\S]*class="segmented-control-thumb"/
  );
  assert.match(
    styles,
    /\.inspector-tabs\s*\{[^}]*border-radius:\s*var\(--radius-pill\);/s
  );
  assert.doesNotMatch(styles, /\.inspector-tabs:hover\s*\{/);
  assert.match(
    styles,
    /\.inspector-tab:hover\s*\{[^}]*background:\s*transparent;[^}]*color:\s*var\(--segment-hover-text\);/s
  );
  assert.match(
    styles,
    /\.inspector-tab\.active\s*\{[^}]*background:\s*transparent;[^}]*\}/s
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

test("inspector header stays fixed and debugger status uses a state dot", () => {
  assert.match(
    styles,
    /--repl-titlebar-height:\s*66px;/
  );
  assert.match(
    styles,
    /\.globals-panel-head\s*\{[^}]*flex:\s*0 0 var\(--repl-titlebar-height\);[^}]*height:\s*var\(--repl-titlebar-height\);[^}]*min-height:\s*var\(--repl-titlebar-height\);[^}]*box-sizing:\s*border-box;/
  );
  assert.match(
    styles,
    /\.globals-panel-head\s*\{[^}]*border-bottom:\s*1px solid var\(--workbench-border\);/
  );
  assert.match(
    styles,
    /\.repl-terminal-bar\s*\{[^}]*flex:\s*0 0 var\(--repl-titlebar-height\);[^}]*height:\s*var\(--repl-titlebar-height\);[^}]*min-height:\s*var\(--repl-titlebar-height\);[^}]*box-sizing:\s*border-box;/
  );
  assert.match(
    styles,
    /\.globals-panel \.btn\s*\{[^}]*min-height:\s*34px;/
  );
  assert.match(
    styles,
    /\.wqdb-panel-status\s*\{[^}]*display:\s*inline-flex;[^}]*gap:\s*7px;[^}]*margin-right:\s*var\(--space-1\);/
  );
  assert.match(
    styles,
    /\.wqdb-panel-status::before\s*\{[^}]*width:\s*7px;[^}]*height:\s*7px;[^}]*border-radius:\s*50%;/
  );
  assert.match(
    styles,
    /\.globals-panel\[data-debugger-state="paused"\] \.wqdb-panel-status::before\s*\{[^}]*background:\s*var\(--terminal-warning\);/
  );
});

test("history search uses current workbench surfaces and focus treatment", () => {
  assert.match(
    styles,
    /--history-focus-ring:\s*#4f88a7;[\s\S]*--history-focus-ring:\s*#a0c1d1;/
  );
  assert.match(
    styles,
    /\.history-search\s*\{[^}]*border:\s*1px solid var\(--workbench-border\);[^}]*border-radius:\s*var\(--radius-control\);[^}]*background:\s*var\(--workbench-output-bg\);/
  );
  assert.match(
    styles,
    /\.history-search input\s*\{[^}]*min-height:\s*42px;[^}]*border-radius:\s*var\(--radius-control\);[^}]*background:\s*var\(--workbench-body-bg\);/
  );
  assert.match(
    styles,
    /\.history-search input:focus-visible\s*\{[^}]*border-color:\s*var\(--history-focus-ring\);[^}]*outline:\s*2px solid var\(--history-focus-ring\);[^}]*box-shadow:\s*none;/
  );
  assert.doesNotMatch(
    styles,
    /\.history-search input:focus-visible\s*\{[^}]*(?:terminal-accent|focus-ring-soft)/
  );
  assert.match(
    styles,
    /\.history-search-results button\s*\{[^}]*flex:\s*0 0 auto;[^}]*line-height:\s*1\.5;/
  );
});

test("themed inspector controls use local surface tokens", () => {
  assert.match(
    styles,
    /\.globals-panel \.btn\s*\{[^}]*border-color:\s*var\(--inspector-control-border\);[^}]*background:\s*var\(--inspector-control-bg\);[^}]*color:\s*var\(--inspector-control-text\);/
  );
  assert.match(
    styles,
    /\.inspector-tab\.active\s*\{[^}]*background:\s*transparent;[^}]*\}/
  );
  assert.match(
    styles,
    /\.segmented-control-thumb\s*\{[^}]*border:\s*1px solid var\(--segment-active-border\);[^}]*background:\s*var\(--segment-active-bg\);/
  );
  assert.match(
    styles,
    /\.wqdb-granularity-option\.active\s*\{[^}]*border-color:\s*transparent;[^}]*background:\s*transparent;[^}]*box-shadow:\s*none;/
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
