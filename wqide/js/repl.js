import { WasmWqSession } from "wq-wasm";
import { createOutputRenderer } from "./ansi.js";
import {
  ensureWasm,
  createWqFrontend,
  getWqVersion,
  getDocMarkdown,
  DEBUG_FLAGS,
  DEBUG_ALIASES,
  BOX_FLAGS,
  parseDebugFlags,
  formatDebugFlags,
  toggleDebugFlagList,
  parseBoxFlags,
  formatBoxFlags,
  setActive,
  syncDebugButtons,
  syncBoxButtons,
  toggleRuntimePanel,
  closeRuntimePanel,
  positionRuntimePanel,
  alignTurnBody,
  insertTextAtCursor,
  handleTabKey,
  fallbackCopyText,
  queueEval,
  formatWqError,
  wireTabList
} from "./wq-shared.js";
import { createWqEditor, isImeCompositionKey } from "./editor.js";
import { renderHighlightedSource } from "./syntax-highlight.js";
import { appendResultPresentation } from "./result-presentation.js";
import {
  createEvaluationController,
  isAbortError,
} from "./eval-lifecycle.js";
import {
  createDomStdinRenderer,
  createStdinRequester,
} from "./stdin-request.js";
import {
  formatGlobalBindings,
  formatGlobalsTable,
  formatNameColumns,
} from "./repl-globals.js";
import { createWqdbController, renderWqdbPanel } from "./wqdb.js";

let session = null;
let frontend = null;
let history = [];
let histIndex = -1;
let pendingBuffer = "";
let timeMode = false;
let showCategory = false;
let oneshotTime = false;
let oneshotDebug = null;
let oneshotWqdb = false;
let execCounter = 1;
let ui = null;
let autoScroll = true;
let evaluationController = null;
let wqdbController = null;
let stdinRequester = null;
let resetRequested = false;
let inspectorTab = "globals";
let inspectorOpen = false;
let inspectorTabsController = null;
let latestOutputRevealTimer = null;

const HISTORY_KEY = "wqide:repl:history";
const HISTORY_LIMIT = 200;
const LATEST_OUTPUT_DISTANCE_PX = 64;
const LATEST_OUTPUT_REVEAL_DELAY_MS = 220;

function isTouchDevice() {
  return navigator.maxTouchPoints > 0;
}

function loadHistory() {
  try {
    const raw = localStorage.getItem(HISTORY_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) {
        history = parsed.slice(-HISTORY_LIMIT);
      }
    }
  } catch (e) {
    console.debug("[repl] failed to load history", e);
  }
}

function saveHistory() {
  try {
    localStorage.setItem(
      HISTORY_KEY,
      JSON.stringify(history.slice(-HISTORY_LIMIT)),
    );
  } catch (e) {
    console.debug("[repl] failed to save history", e);
  }
}

function getDebugFlags() {
  return parseDebugFlags(ensureSession().get_debug_flags());
}

function formatDebugSpec(flags) {
  return flags.length ? flags.join(",") : "off";
}

function setDebugFlags(flags) {
  const next = formatDebugFlags(flags);
  ensureSession().set_debug_flags(next);
  syncDebugControls();
  console.log(`[repl] debug flags -> ${next === "0" ? "off" : next}\n`);
}

function applyDebugSpec(spec) {
  ensureSession().apply_debug_flags(spec);
  syncDebugControls();
  console.log(`[repl] debug flags -> ${formatDebugSpec(getDebugFlags())}\n`);
}

function toggleDebugFlag(flag) {
  const current = getDebugFlags();
  setDebugFlags(toggleDebugFlagList(current, flag));
}

function syncDebugControls() {
  const flags = getDebugFlags();
  syncDebugButtons(ui?.debugButtons, flags);
  setActive(ui?.debugToggle, flags.length > 0);
}

function syncBoxControl() {
  const flags = parseBoxFlags(ensureSession().get_box_flags());
  syncBoxButtons(ui?.boxButtons, flags);
  setActive(ui?.pillBox, flags.length > 0);
}

function getBoxFlags() {
  return parseBoxFlags(ensureSession().get_box_flags());
}

function setBoxFlags(flags) {
  ensureSession().set_box_flags(formatBoxFlags(flags));
  syncBoxControl();
}

function toggleBoxFlag(flag) {
  const current = getBoxFlags();
  const next = current.includes(flag)
    ? current.filter((item) => item !== flag)
    : [...current, flag];
  setBoxFlags(next);
}

function promptPrefix() {
  return "wq[" + execCounter + "] ";
}

function distanceFromThreadBottom() {
  if (!ui?.term) return 0;
  return ui.term.scrollHeight - ui.term.scrollTop - ui.term.clientHeight;
}

function hideLatestOutput() {
  if (latestOutputRevealTimer !== null) {
    window.clearTimeout(latestOutputRevealTimer);
    latestOutputRevealTimer = null;
  }
  if (ui?.scrollLatestBtn) ui.scrollLatestBtn.hidden = true;
}

function scheduleLatestOutputReveal() {
  if (
    !ui?.scrollLatestBtn?.hidden ||
    latestOutputRevealTimer !== null
  ) {
    return;
  }
  latestOutputRevealTimer = window.setTimeout(() => {
    latestOutputRevealTimer = null;
    const awayFromBottom =
      distanceFromThreadBottom() >= LATEST_OUTPUT_DISTANCE_PX;
    ui.scrollLatestBtn.hidden = !awayFromBottom;
  }, LATEST_OUTPUT_REVEAL_DELAY_MS);
}

function scrollThreadToBottom(mode = "smooth") {
  if (!ui?.term) return;
  if (!autoScroll && !mode.startsWith("force")) return;
  const behavior = mode === "force-instant" ? "auto" : "smooth";
  ui.term.scrollTo({ top: ui.term.scrollHeight, behavior });
  hideLatestOutput();
}

function observeUserScroll() {
  if (!ui?.term) return;
  ui.term.addEventListener("scroll", () => {
    const nearBottom =
      distanceFromThreadBottom() < LATEST_OUTPUT_DISTANCE_PX;
    autoScroll = nearBottom;
    if (nearBottom) {
      hideLatestOutput();
    } else {
      scheduleLatestOutputReveal();
    }
  });
}

function setupViewportHandler() {
  if (!window.visualViewport) return;
  const vv = window.visualViewport;
  const onResize = () => {
    if (!ui?.term || !ui?.promptForm) return;
    const offset =
      vv.height < window.innerHeight ? window.innerHeight - vv.height : 0;
    if (offset > 60) {
      // keyboard likely open
      ui.term.style.paddingBottom = `${Math.min(offset, 280)}px`;
      scrollThreadToBottom("force-keyboard");
    } else {
      ui.term.style.paddingBottom = "";
    }
  };
  vv.addEventListener("resize", onResize);
  vv.addEventListener("scroll", onResize);
}

function createTurn(kind, label, body, msgType = null) {
  const turn = document.createElement("article");
  turn.className = `repl-turn repl-turn-${kind}`;
  const line = document.createElement("div");
  line.className = "repl-line";
  const content = document.createElement("pre");
  content.className = `repl-line-body repl-line-body-${kind}`;

  if (kind === "input") {
    const prompt = document.createElement("span");
    prompt.className = "repl-prompt";
    const m = /^(wq)\[(\d+)\]$/.exec(label);
    if (m) {
      const wqSpan = document.createElement("span");
      wqSpan.className = "repl-prompt-wq";
      wqSpan.textContent = m[1];
      prompt.appendChild(wqSpan);
      prompt.appendChild(document.createTextNode("["));
      const numSpan = document.createElement("span");
      numSpan.className = "repl-prompt-num";
      numSpan.textContent = m[2];
      prompt.appendChild(numSpan);
      prompt.appendChild(document.createTextNode("] "));
    } else {
      prompt.textContent = label + " ";
    }
    content.appendChild(prompt);
    const codeSpan = document.createElement("span");
    codeSpan.className = "repl-input-code";
    renderHighlightedSource(codeSpan, frontend, body || "");
    content.appendChild(codeSpan);
  } else {
    const bar = document.createElement("span");
    bar.className = `repl-bar repl-bar-${msgType || "info"}`;
    bar.textContent = "\u258d ";
    content.appendChild(bar);
    content.__outputRenderer = createOutputRenderer(content, bar);
    if (body) {
      const style = msgType === "error" ? "error" : null;
      content.__outputRenderer.appendOutput(body, style);
    }
  }

  line.appendChild(content);
  turn.appendChild(line);
  ui.output.appendChild(turn);

  // Long-press context menu for output turns
  if (kind === "output" || kind === "system") {
    setupTurnContextMenu(turn, content);
  }

  if (kind !== "input") {
    scrollThreadToBottom();
  }
  return content;
}

function setupTurnContextMenu(turn, contentEl) {
  let pressTimer = null;
  let moved = false;

  const clear = () => {
    if (pressTimer) {
      clearTimeout(pressTimer);
      pressTimer = null;
    }
  };

  const onPointerDown = (e) => {
    moved = false;
    pressTimer = setTimeout(() => {
      if (!moved) {
        showTurnMenu(e.clientX, e.clientY, contentEl);
      }
    }, 600);
  };

  const onPointerMove = () => {
    moved = true;
    clear();
  };

  const onPointerUp = () => {
    clear();
  };

  turn.addEventListener("pointerdown", onPointerDown);
  turn.addEventListener("pointermove", onPointerMove);
  turn.addEventListener("pointerup", onPointerUp);
  turn.addEventListener("pointercancel", clear);
  turn.addEventListener("touchmove", onPointerMove, { passive: true });

  // Double-click to select text
  turn.addEventListener("dblclick", () => {
    const sel = window.getSelection();
    const range = document.createRange();
    range.selectNodeContents(contentEl);
    sel.removeAllRanges();
    sel.addRange(range);
  });
}

let activeMenu = null;

function showTurnMenu(x, y, contentEl) {
  if (activeMenu) {
    activeMenu.remove();
    activeMenu = null;
  }
  const menu = document.createElement("div");
  menu.className = "turn-context-menu";
  menu.style.left = `${x}px`;
  menu.style.top = `${y}px`;

  const addItem = (label, action) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "turn-context-menu-item";
    item.textContent = label;
    item.addEventListener("pointerup", () => {
      action();
      menu.remove();
      activeMenu = null;
    });
    menu.appendChild(item);
  };

  addItem("Copy text", () => {
    fallbackCopyText(contentEl.textContent);
  });

  const inputTurn = contentEl.closest(".repl-turn")?.previousElementSibling;
  if (inputTurn?.classList.contains("repl-turn-input")) {
    addItem("Re-run", () => {
      const text =
        inputTurn.querySelector(".repl-input-code")?.textContent ?? "";
      ui.codeEl.value = text;
      syncInputPresentation();
      doEval();
    });
  }

  document.body.appendChild(menu);
  activeMenu = menu;

  // Adjust if off-screen
  requestAnimationFrame(() => {
    const rect = menu.getBoundingClientRect();
    if (rect.right > window.innerWidth) {
      menu.style.left = `${window.innerWidth - rect.width - 8}px`;
    }
    if (rect.bottom > window.innerHeight) {
      menu.style.top = `${window.innerHeight - rect.height - 8}px`;
    }
  });

  const close = (e) => {
    if (!menu.contains(e.target)) {
      menu.remove();
      activeMenu = null;
      document.removeEventListener("pointerdown", close);
    }
  };
  setTimeout(() => document.addEventListener("pointerdown", close), 10);
}

function append(chunk, msgType = "info", options = {}) {
  console.log(chunk);
  const turn = document.createElement("article");
  turn.className = "repl-turn repl-turn-system";
  const line = document.createElement("div");
  line.className = "repl-line";
  const content = document.createElement("pre");
  content.className = "repl-line-body repl-line-body-system";
  content.__outputRenderer = createOutputRenderer(content);
  const aligned = alignTurnBody(chunk);
  if (options.backend) {
    const style = msgType === "error" ? "error" : null;
    content.__outputRenderer.appendStreamOutput(aligned, style);
  } else if (msgType === "error") {
    content.__outputRenderer.appendStyledText(aligned, "error");
  } else {
    content.__outputRenderer.appendOutput(aligned);
  }
  line.appendChild(content);
  turn.appendChild(line);
  ui.output.appendChild(turn);
  scrollThreadToBottom();
}

function bindRuntimeCallbacks(run = null) {
  const target = ensureSession();
  target.set_stdout_callback((chunk) =>
    append(chunk, "info", { backend: true }),
  );
  target.set_stderr_callback((chunk) =>
    append(chunk, "error", { backend: true }),
  );
  target.set_stdin_callback(
    run
      ? async (prompt) => {
          run.setState("awaiting-input");
          try {
            return await stdinRequester.request(prompt, {
              signal: run.signal,
            });
          } finally {
            if (!run.signal.aborted) run.setState("running");
          }
        }
      : null,
  );
}

function ensureSession() {
  if (!session) {
    session = new WasmWqSession();
    frontend.set_builtins_preset(session.get_builtins_preset());
  }
  return session;
}

function syncLivePrompt() {
  if (!ui?.livePromptNumber) return;
  ui.livePromptNumber.textContent = String(execCounter);
}

function syncInputPresentation() {
  if (!ui?.codeEl || !ui?.promptForm) return;
  ui.codeEl.style.height = "0px";
  const nextHeight = Math.min(Math.max(ui.codeEl.scrollHeight, 22), 176);
  ui.codeEl.style.height = `${nextHeight}px`;

  const source = ui.codeEl.value;
  const incomplete =
    source.trim().length > 0 &&
    !source.trimStart().startsWith("\\") &&
    !frontend.is_complete_input(source);
  ui.promptForm.dataset.inputState = incomplete ? "continuation" : "ready";
}

function setInspectorOpen(open) {
  inspectorOpen = Boolean(open);
  if (!ui?.globalsPanel || !ui?.inspectorToggleBtn || !ui?.replShell) return;
  ui.globalsPanel.hidden = !inspectorOpen;
  ui.inspectorToggleBtn.setAttribute(
    "aria-expanded",
    String(inspectorOpen),
  );
  setActive(ui.inspectorToggleBtn, inspectorOpen);
  ui.replShell.dataset.inspectorOpen = String(inspectorOpen);
  if (inspectorOpen) syncGlobalsPanel();
}

function renderEmptyGlobals() {
  const empty = document.createElement("div");
  empty.className = "globals-panel-empty";

  const title = document.createElement("strong");
  title.textContent = "No global bindings";
  const detail = document.createElement("span");
  detail.textContent = "Assignments from the REPL will appear here.";

  empty.append(title, detail);
  ui.globalsBody.replaceChildren(empty);
}

function renderGlobalBindings(globals) {
  const list = document.createElement("dl");
  list.className = "globals-list";

  for (const binding of formatGlobalBindings(globals)) {
    const item = document.createElement("div");
    item.className = "global-binding";

    const heading = document.createElement("div");
    heading.className = "global-binding-head";

    const name = document.createElement("dt");
    name.className = "global-binding-name";
    const nameCode = document.createElement("code");
    nameCode.textContent = binding.name;
    name.append(nameCode);

    const category = document.createElement("dd");
    category.className = "global-binding-category";
    category.textContent = binding.category;

    const value = document.createElement("dd");
    value.className = "global-binding-value";
    const valueCode = document.createElement("code");
    valueCode.textContent = binding.value;
    value.append(valueCode);

    heading.append(name, category);
    item.append(heading, value);
    list.append(item);
  }

  ui.globalsBody.replaceChildren(list);
}

function syncGlobalsPanel() {
  if (!ui?.globalsBody) return;
  let count = 0;
  let state = "empty";
  try {
    const globals = Array.from(ensureSession().globals());
    count = globals.length;
    state = count === 0 ? "empty" : "ready";
    if (count === 0) {
      renderEmptyGlobals();
    } else {
      renderGlobalBindings(globals);
    }
  } catch (err) {
    state = "error";
    const error = document.createElement("p");
    error.className = "globals-panel-error";
    error.textContent = formatWqError(err);
    ui.globalsBody.replaceChildren(error);
  }
  ui.globalsCount.textContent = String(count);
  ui.globalsCount.setAttribute(
    "aria-label",
    `${count} global binding${count === 1 ? "" : "s"}`,
  );
  ui.globalsPanel.dataset.state = state;
}

function setInspectorTab(tab) {
  if (!ui?.globalsTab || !ui?.debuggerTab) return;
  inspectorTab = tab;
  const showGlobals = tab === "globals";
  ui.globalsTab.classList.toggle("active", showGlobals);
  ui.globalsTab.setAttribute("aria-selected", String(showGlobals));
  ui.debuggerTab.classList.toggle("active", !showGlobals);
  ui.debuggerTab.setAttribute("aria-selected", String(!showGlobals));
  ui.globalsBody.hidden = !showGlobals;
  ui.debuggerBody.hidden = showGlobals;
  ui.globalsPanelActions.hidden = !showGlobals;
  ui.debuggerPanelStatus.hidden = showGlobals;
  inspectorTabsController?.sync(
    showGlobals ? ui.globalsTab : ui.debuggerTab
  );
}

function syncWqdbPanel(state = wqdbController?.state) {
  if (!state || !ui?.debuggerBody) return;
  ui.debuggerPanelStatus.textContent =
    state.status === "paused"
      ? "Paused"
      : state.status === "running"
        ? "Running"
        : "Idle";
  ui.globalsPanel.dataset.debuggerState = state.status;
  renderWqdbPanel(ui.debuggerBody, state, wqdbController, {
    frontend,
  });
  if (state.status === "paused") {
    setInspectorTab("debugger");
    setInspectorOpen(true);
  }
}

function setButtonStatus(btn, label) {
  if (!btn) return;
  const idle = btn.dataset.idleLabel || btn.textContent;
  btn.dataset.idleLabel = idle;
  btn.textContent = label;
}

function resetButtonStatus(btn) {
  if (!btn) return;
  btn.textContent = btn.dataset.idleLabel || btn.textContent;
}

async function copyCurrentOutput() {
  const turns = Array.from(
    ui.term.querySelectorAll(".repl-turn-output .repl-line-body"),
  );
  const text = turns.map((turn) => turn.textContent).join("");
  if (!text.trim()) {
    setButtonStatus(ui.copyOutputBtn, "✕ No Output");
    setTimeout(() => {
      resetButtonStatus(ui.copyOutputBtn);
    }, 1400);
    return;
  }
  try {
    await fallbackCopyText(text);
    setButtonStatus(ui.copyOutputBtn, "✓ Copied");
  } catch (err) {
    console.error(err);
    setButtonStatus(ui.copyOutputBtn, "✕ Error");
  }
  setTimeout(() => {
    resetButtonStatus(ui.copyOutputBtn);
  }, 1400);
}

async function copyCurrentFlow() {
  const turns = Array.from(ui.term.querySelectorAll(".repl-turn"));
  const parts = turns.map((turn) => {
    const body = turn.querySelector(".repl-line-body")?.textContent ?? "";
    return body;
  });
  const promptText = ui.codeEl.value.trim();
  if (promptText) {
    parts.push(`${promptPrefix().trim()} ${promptText}`);
  }
  const text = parts.filter(Boolean).join("\n\n");
  if (!text.trim()) {
    setButtonStatus(ui.copyFlowBtn, "Nothing");
    setTimeout(() => {
      resetButtonStatus(ui.copyFlowBtn);
    }, 1400);
    return;
  }
  try {
    await fallbackCopyText(text);
    setButtonStatus(ui.copyFlowBtn, "✓ Copied");
  } catch (err) {
    console.error(err);
    setButtonStatus(ui.copyFlowBtn, "Error");
  }
  setTimeout(() => {
    resetButtonStatus(ui.copyFlowBtn);
  }, 1400);
}

function resetSession() {
  if (evaluationController?.active) {
    resetRequested = true;
    evaluationController.stop("session reset");
    return;
  }
  const oldSession = session;
  wqdbController?.reset();
  session = null;
  oldSession?.free();
  // Keep history across resets
  pendingBuffer = "";
  timeMode = false;
  showCategory = false;
  oneshotTime = false;
  oneshotDebug = null;
  oneshotWqdb = false;
  execCounter = 1;
  ui.output.replaceChildren();
  bindRuntimeCallbacks();
  ensureSession();
  append(`wq ${getWqVersion()} (c)tttiw (l)MIT\n`);
  syncLivePrompt();
  syncInputPresentation();
  syncBoxControl();
  setActive(ui.pillTime, false);
  syncDebugControls();
  syncGlobalsPanel();
}

function boolWord(on) {
  return on ? "on" : "off";
}

function statusLine(name, value) {
  append(`${name}: ${value}\n`, "info");
}

function commandLine(name, value) {
  append(`${name} -> ${value}\n`, "info");
}

function replHelpText() {
  const commands = [
    "\\exit, \\e, \\\\",
    "\\info",
    "\\dry, \\dry?",
    "\\builtin [preset], \\",
    "\\gb, \\g",
    "\\reset, \\r",
    "\\box, \\b, \\box <spec>, \\box?",
    "\\xray, \\x, \\xray?",
    "\\backtrace, \\bt, \\backtrace?",
    "\\interpreter [name], \\i [name]",
    "\\time, \\t, \\time., \\time?",
    "\\wqdb, \\w, \\wqdb., \\wqdb?",
    "\\debug, \\d, \\d <spec>, \\d.<spec>",
    "\\category, \\category? (aliases: \\type, \\type?)",
    "\\help [topic], \\h [topic]",
  ];
  return commands.join("\n");
}

function debugHelpTable() {
  const rows = [
    ["active", formatDebugSpec(getDebugFlags())],
    ["names", DEBUG_FLAGS.join(",")],
    ...DEBUG_ALIASES,
  ];
  const leftW = rows.reduce((w, row) => Math.max(w, row[0].length), 0);
  const rightW = rows.reduce((w, row) => Math.max(w, row[1].length), 0);
  const rule = `+-${"-".repeat(leftW)}-+-${"-".repeat(rightW)}-+`;
  const lines = [rule, `| ${"spec".padEnd(leftW)} | ${"flags".padEnd(rightW)} |`, rule];
  rows.forEach(([left, right]) => {
    lines.push(`| ${left.padEnd(leftW)} | ${right.padEnd(rightW)} |`);
  });
  lines.push(rule);
  return lines.join("\n");
}

function computeDebugSpec(spec) {
  const session = ensureSession();
  const prev = session.get_debug_flags();
  session.apply_debug_flags(spec);
  const next = session.get_debug_flags();
  session.set_debug_flags(prev);
  syncDebugControls();
  return next;
}

function parseReplCommand(input) {
  const trimmed = input.trim();
  switch (trimmed) {
    case "":
      return { kind: "empty" };
    case "\\exit":
    case "\\e":
    case "\\\\":
    case "\\bye":
      return { kind: "exit" };
    case "\\goodbye":
      return { kind: "goodbye" };
    case "\\highlight":
    case "\\hl":
      return { kind: "highlight" };
    case "\\highlight?":
    case "\\hl?":
      return { kind: "highlight-query" };
    case "\\hint":
    case "\\hint?":
      return { kind: "hint" };
    case "\\info":
      return { kind: "info" };
    case "\\dry":
      return { kind: "dry" };
    case "\\dry?":
      return { kind: "dry-query" };
    case "\\fmt":
    case "\\fmt?":
      return { kind: "fmt" };
    case "\\builtin":
    case "\\":
      return { kind: "builtin" };
    case "\\gb":
    case "\\g":
      return { kind: "gb" };
    case "\\reset":
    case "\\r":
      return { kind: "reset" };
    case "\\box":
    case "\\b":
      return { kind: "box" };
    case "\\box?":
    case "\\b?":
      return { kind: "box-query" };
    case "\\backtrace":
    case "\\bt":
      return { kind: "backtrace" };
    case "\\backtrace?":
    case "\\bt?":
      return { kind: "backtrace-query" };
    case "\\xray":
    case "\\x":
      return { kind: "xray" };
    case "\\xray?":
    case "\\x?":
      return { kind: "xray-query" };
    case "\\interpreter":
    case "\\i":
      return { kind: "interpreter" };
    case "\\time":
    case "\\t":
      return { kind: "time" };
    case "\\time.":
    case "\\t.":
      return { kind: "time-oneshot" };
    case "\\time?":
    case "\\t?":
      return { kind: "time-query" };
    case "\\wqdb":
    case "\\w":
      return { kind: "wqdb" };
    case "\\wqdb.":
    case "\\w.":
      return { kind: "wqdb-oneshot" };
    case "\\wqdb?":
    case "\\w?":
      return { kind: "wqdb-query" };
    case "\\debug":
      return { kind: "debug-show" };
    case "\\d":
      return { kind: "debug-toggle" };
    case "\\category":
    case "\\type":
      return { kind: "category" };
    case "\\category?":
    case "\\type?":
      return { kind: "category-query" };
    case "\\help":
    case "\\h":
      return { kind: "help" };
    default:
      break;
  }

  const prefixed = [
    ["\\fmt ", "fmt"],
    ["\\builtin ", "builtin-set"],
    ["\\box ", "box-set"],
    ["\\b ", "box-set"],
    ["\\interpreter ", "interpreter-set"],
    ["\\i ", "interpreter-set"],
    ["\\help ", "help-topic"],
    ["\\h ", "help-topic"],
    ["\\debug.", "debug-oneshot"],
    ["\\d.", "debug-oneshot"],
    ["\\debug ", "debug-set"],
    ["\\d ", "debug-set"],
  ];
  for (const [prefix, kind] of prefixed) {
    if (trimmed.startsWith(prefix)) {
      return { kind, arg: trimmed.slice(prefix.length).trim() };
    }
  }
  if (trimmed.startsWith("\\d")) {
    return { kind: "debug-set", arg: trimmed.slice(2).trim() };
  }
  if (trimmed.startsWith("\\")) {
    return { kind: "unknown", arg: trimmed };
  }
  return null;
}

async function handleReplCommand(code) {
  const command = parseReplCommand(code);
  if (!command) return false;
  if (command.kind === "empty") return true;

  const session = ensureSession();
  try {
    switch (command.kind) {
      case "exit":
        append("bye..\n", "info");
        return true;
      case "goodbye":
        append("goodbye!\n", "info");
        return true;
      case "highlight":
      case "highlight-query":
        statusLine("highlight", "on");
        return true;
      case "hint":
        statusLine("hint", "off");
        return true;
      case "info":
        append(
          `wq ${getWqVersion()}\ninterpreter: ${session.get_interpreter_name()}\nbuiltin: ${session.get_builtins_preset()}\n`,
          "info",
        );
        return true;
      case "dry": {
        const on = !session.get_dry_mode();
        session.set_dry_mode(on);
        commandLine("dry", boolWord(on));
        return true;
      }
      case "dry-query":
        statusLine("dry", boolWord(session.get_dry_mode()));
        return true;
      case "fmt":
        append("fmt command is not available in wqide yet\n", "info");
        return true;
      case "builtin":
        append(
          `Current: ${session.get_builtins_preset()}\nAvailable: ${Array.from(session.builtin_preset_names()).join(", ")}\n\n${formatNameColumns(frontend.builtin_names())}\n`,
          "info",
        );
        return true;
      case "builtin-set": {
        const selected = session.set_builtins_preset(command.arg);
        frontend.set_builtins_preset(selected);
        commandLine("builtin", selected);
        return true;
      }
      case "gb":
        append(`${formatGlobalsTable(session.globals())}\n`, "info");
        return true;
      case "reset":
        session.reset_workspace();
        append("session reset\n", "info");
        return true;
      case "box":
        setBoxFlags(
          getBoxFlags().length
            ? []
            : BOX_FLAGS.filter((flag) => flag !== "xray"),
        );
        commandLine("box", session.get_box_summary());
        return true;
      case "box-set":
        session.apply_box_flags(command.arg);
        syncBoxControl();
        commandLine("box", session.get_box_summary());
        return true;
      case "box-query":
        statusLine("box", session.get_box_summary());
        return true;
      case "backtrace": {
        const on = !session.backtrace_enabled();
        session.set_backtrace_enabled(on);
        commandLine("backtrace", boolWord(on));
        return true;
      }
      case "backtrace-query":
        statusLine("backtrace", boolWord(session.backtrace_enabled()));
        return true;
      case "xray": {
        const flags = getBoxFlags();
        const on = !flags.includes("xray");
        session.apply_box_flags(on ? "+xray" : "-xray");
        syncBoxControl();
        commandLine("box", session.get_box_summary());
        return true;
      }
      case "xray-query":
        statusLine("xray", boolWord(getBoxFlags().includes("xray")));
        return true;
      case "interpreter":
        append(
          `Current: ${session.get_interpreter_name()}\nAvailable: ${Array.from(session.interpreter_names()).join(", ")}\n`,
          "info",
        );
        return true;
      case "interpreter-set": {
        const selected = session.set_interpreter_by_name(command.arg);
        commandLine("interpreter", selected);
        return true;
      }
      case "time":
        timeMode = !timeMode;
        setActive(ui.pillTime, timeMode);
        commandLine("time", boolWord(timeMode));
        return true;
      case "time-oneshot":
        oneshotTime = true;
        commandLine("time", "on for next eval");
        return true;
      case "time-query": {
        const status = timeMode ? "on" : oneshotTime ? "on for next eval" : "off";
        statusLine("time", status);
        return true;
      }
      case "wqdb": {
        const on = !session.get_wqdb_mode();
        session.set_wqdb_mode(on);
        commandLine("wqdb", boolWord(on));
        return true;
      }
      case "wqdb-oneshot":
        oneshotWqdb = true;
        commandLine("wqdb", "on for next eval");
        return true;
      case "wqdb-query": {
        const status = session.get_wqdb_mode()
          ? "on"
          : oneshotWqdb
            ? "on for next eval"
            : "off";
        statusLine("wqdb", status);
        return true;
      }
      case "debug-show":
        append(`${debugHelpTable()}\n`, "info");
        return true;
      case "debug-toggle":
        if (getDebugFlags().length) {
          setDebugFlags([]);
        } else {
          session.set_debug_flags("1");
          syncDebugControls();
        }
        commandLine("debug flags", formatDebugSpec(getDebugFlags()));
        return true;
      case "debug-oneshot":
        oneshotDebug = computeDebugSpec(command.arg);
        commandLine("debug flags", `${oneshotDebug} for next eval`);
        return true;
      case "debug-set":
        applyDebugSpec(command.arg);
        commandLine("debug flags", formatDebugSpec(getDebugFlags()));
        return true;
      case "category":
        showCategory = !showCategory;
        commandLine("category", boolWord(showCategory));
        return true;
      case "category-query":
        statusLine("category", boolWord(showCategory));
        return true;
      case "help":
        append(`${replHelpText()}\n`, "info");
        return true;
      case "help-topic": {
        const md = await getDocMarkdown(command.arg);
        append(`${md}\n`, "info");
        return true;
      }
      case "unknown":
        append(`unknown repl command '${command.arg}'\n`, "error");
        return true;
      default:
        return false;
    }
  } catch (err) {
    append(`${formatWqError(err)}\n`, "error");
    return true;
  }
}

async function doEval({ recordHistory = true } = {}) {
  const code = ui.codeEl.value;
  if (!code.trim() || evaluationController?.active) return;
  autoScroll = true;
  const evaluationNumber = execCounter;
  createTurn("input", promptPrefix().trim(), code.trim());
  execCounter++;
  syncLivePrompt();
  try {
    if (
      recordHistory &&
      (!history.length || history[history.length - 1] !== code)
    ) {
      history.push(code);
      saveHistory();
    }
    histIndex = -1;
    pendingBuffer = "";
    ui.codeEl.value = "";
    syncInputPresentation();

    if (await handleReplCommand(code)) {
      return;
    }

    const start = performance.now();
    const session = ensureSession();
    const dryMode = session.get_dry_mode();
    const useOneshotTime = oneshotTime;
    const prevDebug = oneshotDebug ? session.get_debug_flags() : null;
    const prevWqdb = oneshotWqdb ? session.get_wqdb_mode() : null;
    const sourcePath = `<repl:${evaluationNumber}>`;
    const result = await evaluationController.run(({ signal, setState }) =>
      queueEval(
        async () => {
          setState("running");
          bindRuntimeCallbacks({ signal, setState });
          if (oneshotDebug) {
            session.set_debug_flags(oneshotDebug);
          }
          if (oneshotWqdb) {
            session.set_wqdb_mode(true);
          }
          if (session.get_wqdb_mode()) {
            session.arm_wqdb_next();
          }
          try {
            return await session.eval_wq_async(code, {
              signal,
              sourcePath,
              onDebuggerPause(stop) {
                setState("paused");
                return wqdbController
                  .pause(stop, { source: code, sourcePath })
                  .finally(() => {
                    if (!signal.aborted) setState("running");
                  });
              },
              onDebuggerNotification(notification) {
                wqdbController.recordNotification(notification);
              },
            });
          } finally {
            wqdbController.finish();
            bindRuntimeCallbacks();
            if (prevDebug !== null) {
              session.set_debug_flags(prevDebug);
            }
            if (prevWqdb !== null) {
              session.set_wqdb_mode(prevWqdb);
            }
          }
        },
        { signal },
      ),
    );
    const end = performance.now();
    if (dryMode) {
      append("dry run: skipped\n", "info");
    } else if (
      result.display !== undefined &&
      result.display !== null &&
      String(result.display).length
    ) {
      const content = createTurn("output", "", "", "success");
      if (
        appendResultPresentation(content, result.presentation, {
          indent: "  ",
        })
      ) {
        content.__outputRenderer.appendText(
          (showCategory && result.category ? `\n${result.category}` : "") +
            "\n",
        );
      } else {
        const valueText =
          alignTurnBody(String(result.display)) +
          (showCategory && result.category ? `\n${result.category}` : "") +
          "\n";
        content.__outputRenderer.appendOutput(valueText);
      }
      if (getBoxFlags().includes("xray") && result.xray) {
        createTurn(
          "system",
          "",
          alignTurnBody(String(result.xray)) + "\n",
          "info",
        );
      }
      if (timeMode || useOneshotTime) {
        append(`time elapsed: ${end - start}ms\n`, "info");
      }
    }
  } catch (err) {
    if (isAbortError(err) && !resetRequested) {
      createTurn("system", "", "Interrupted\n", "info");
    } else if (!isAbortError(err)) {
      console.error("err from wq:" + err);
      createTurn(
        "system",
        "",
        alignTurnBody(formatWqError(err, { rendered: true }) + "\n"),
        "error",
      );
    }
  } finally {
    oneshotTime = false;
    oneshotDebug = null;
    oneshotWqdb = false;
    if (resetRequested) {
      resetRequested = false;
      resetSession();
      return;
    }
    syncDebugControls();
    syncGlobalsPanel();
    syncInputPresentation();
    scrollThreadToBottom("force");
  }
}

function handleReplTabKey(e) {
  handleTabKey(e, ui.codeEl, () => {
    syncInputPresentation();
  });
}

function positionHistorySearch() {
  if (!ui.historySearch) return;
  if (ui.historySearch.hidden) return;
  const replRect = ui.replFlow?.getBoundingClientRect();
  const termRect = ui.term?.getBoundingClientRect();
  if (!replRect || !termRect) return;

  const inset = window.matchMedia("(max-width: 560px)").matches ? 12 : 18;
  const top = Math.max(8, termRect.top + inset);
  const availableHeight = Math.max(96, replRect.bottom - top - inset);
  const panelHeight = Math.min(320, availableHeight);

  ui.historySearch.style.left = `${Math.max(8, replRect.left + inset)}px`;
  ui.historySearch.style.right = `${Math.max(
    8,
    window.innerWidth - replRect.right + inset,
  )}px`;
  ui.historySearch.style.top = `${top}px`;
  ui.historySearch.style.bottom = "auto";
  ui.historySearch.style.setProperty(
    "--history-search-max-h",
    `${panelHeight}px`,
  );
  ui.historySearch.style.setProperty(
    "--history-results-max-h",
    `${Math.max(40, panelHeight - 68)}px`,
  );
}

function closeHistorySearch({ focusComposer = false } = {}) {
  if (!ui.historySearch) return;
  ui.historySearch.hidden = true;
  ui.historyToggleBtn?.setAttribute("aria-expanded", "false");
  setActive(ui.historyToggleBtn, false);
  if (focusComposer) ui.codeEl.focus();
}

function renderHistoryMatches(input, results) {
  const q = input.value.toLowerCase();
  const matches = history
    .slice()
    .reverse()
    .filter((h) => h.toLowerCase().includes(q));
  results.innerHTML = "";
  if (!matches.length) {
    const empty = document.createElement("span");
    empty.className = "history-search-empty";
    empty.textContent = "No matches";
    results.appendChild(empty);
    return;
  }
  matches.forEach((text) => {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.textContent = text;
    btn.addEventListener("click", () => {
      ui.codeEl.value = text;
      syncInputPresentation();
      closeHistorySearch();
      ui.codeEl.focus();
    });
    results.appendChild(btn);
  });
}

function openHistorySearch() {
  if (!ui.historySearch) return;
  ui.historySearch.hidden = false;
  ui.historyToggleBtn?.setAttribute("aria-expanded", "true");
  setActive(ui.historyToggleBtn, true);
  const input = ui.historySearchInput;
  const results = ui.historySearchResults;
  input.value = ui.codeEl.value;
  positionHistorySearch();
  input.focus();

  function update() {
    renderHistoryMatches(input, results);
    positionHistorySearch();
  }

  update();
  input.oninput = update;
  input.onkeydown = (e) => {
    if (e.key === "Escape") {
      closeHistorySearch({ focusComposer: true });
    }
  };
}

function toggleHistorySearch() {
  if (!ui.historySearch) return;
  if (ui.historySearch.hidden) {
    openHistorySearch();
  } else {
    closeHistorySearch({ focusComposer: true });
  }
}

function canNavigateHistory(direction) {
  const start = ui.codeEl.selectionStart;
  const end = ui.codeEl.selectionEnd;
  if (start !== end) return false;
  return direction < 0
    ? !ui.codeEl.value.slice(0, start).includes("\n")
    : !ui.codeEl.value.slice(end).includes("\n");
}

function clearScreen({ focusInput = false } = {}) {
  ui.output.replaceChildren();
  autoScroll = true;
  hideLatestOutput();
  syncInputPresentation();
  if (focusInput) ui.codeEl.focus();
}

export async function mountRepl(root) {
  await ensureWasm();
  frontend = createWqFrontend();
  ui = {
    replShell: root.querySelector(".repl-shell"),
    replFlow: root.querySelector(".repl-flow"),
    codeEl: createWqEditor(root.querySelector("#code"), {
      multilineMode: "shift",
      frontend,
    }),
    term: root.querySelector("#term"),
    output: root.querySelector("#terminalOutput"),
    promptForm: root.querySelector("#promptForm"),
    livePromptNumber: root.querySelector("#livePromptNumber"),
    terminalStatus: root.querySelector("#terminalStatus"),
    terminalMenu: root.querySelector("#terminalMenu"),
    scrollLatestBtn: root.querySelector("#scrollLatestBtn"),
    stopBtn: root.querySelector("#stopBtn"),
    clearBtn: root.querySelector("#clearBtn"),
    resetBtn: root.querySelector("#resetBtn"),
    copyFlowBtn: root.querySelector("#copyFlowBtn"),
    copyOutputBtn: root.querySelector("#copyOutputBtn"),
    pillBox: root.querySelector("#pillBox"),
    boxPanel: root.querySelector("#boxPanel"),
    boxButtons: Object.fromEntries(
      BOX_FLAGS.map((flag) => [
        flag,
        root.querySelector(`[data-box-flag="${flag}"]`),
      ]),
    ),
    pillTime: root.querySelector("#pillTime"),
    debugToggle: root.querySelector("#debugToggle"),
    debugPanel: root.querySelector("#debugPanel"),
    debugButtons: Object.fromEntries(
      DEBUG_FLAGS.map((flag) => [
        flag,
        root.querySelector(`[data-debug-flag="${flag}"]`),
      ]),
    ),
    historyToggleBtn: root.querySelector("#historyToggleBtn"),
    historySearch: root.querySelector("#historySearch"),
    historySearchInput: root.querySelector("#historySearchInput"),
    historySearchResults: root.querySelector("#historySearchResults"),
    clearHistoryBtn: root.querySelector("#clearHistoryBtn"),
    globalsPanel: root.querySelector(".globals-panel"),
    inspectorToggleBtn: root.querySelector("#inspectorToggleBtn"),
    inspectorTabs: root.querySelector(".inspector-tabs"),
    globalsTab: root.querySelector("#globalsTab"),
    debuggerTab: root.querySelector("#debuggerTab"),
    globalsBody: root.querySelector("#globalsBody"),
    globalsCount: root.querySelector("#globalsCount"),
    globalsPanelActions: root.querySelector("#globalsPanelActions"),
    refreshGlobalsBtn: root.querySelector("#refreshGlobalsBtn"),
    debuggerBody: root.querySelector("#debuggerBody"),
    debuggerPanelStatus: root.querySelector("#debuggerPanelStatus"),
  };
  inspectorTabsController = wireTabList(ui.inspectorTabs, {
    onSelect(button) {
      setInspectorTab(button === ui.debuggerTab ? "debugger" : "globals");
    }
  });

  evaluationController = createEvaluationController((state) => {
    const active = state !== "idle";
    ui.clearBtn.disabled = active;
    ui.stopBtn.hidden = !active;
    ui.stopBtn.disabled = state === "stopping";
    ui.promptForm.dataset.evaluationState = state;
    ui.replFlow.dataset.evaluationState = state;
    ui.terminalStatus.textContent =
      state === "idle"
        ? "Ready"
        : state === "awaiting-input"
          ? "Waiting for input"
          : state === "paused"
            ? "Paused"
            : state === "stopping"
              ? "Stopping"
              : "Running";
    syncInputPresentation();
  });
  wqdbController = createWqdbController((state) => {
    syncWqdbPanel(state);
  });
  const renderStdin = createDomStdinRenderer(ui.output);
  stdinRequester = createStdinRequester({
    render(request) {
      const view = renderStdin(request);
      scrollThreadToBottom();
      return view;
    },
  });

  bindRuntimeCallbacks();
  loadHistory();
  observeUserScroll();
  setupViewportHandler();

  ui.resetBtn.addEventListener("click", () => resetSession());
  ui.copyFlowBtn?.addEventListener("click", () => {
    copyCurrentFlow();
  });
  ui.copyOutputBtn?.addEventListener("click", () => {
    copyCurrentOutput();
  });
  ui.historyToggleBtn?.addEventListener("click", () => {
    toggleHistorySearch();
  });
  ui.inspectorToggleBtn?.addEventListener("click", () => {
    setInspectorOpen(!inspectorOpen);
  });
  ui.scrollLatestBtn?.addEventListener("click", () => {
    autoScroll = true;
    scrollThreadToBottom("force-instant");
    ui.codeEl.focus();
  });
  ui.clearHistoryBtn?.addEventListener("click", () => {
    history = [];
    histIndex = -1;
    pendingBuffer = ui.codeEl.value;
    saveHistory();
    if (!ui.historySearch?.hidden) {
      renderHistoryMatches(ui.historySearchInput, ui.historySearchResults);
      positionHistorySearch();
    }
  });
  ui.refreshGlobalsBtn?.addEventListener("click", () => {
    syncGlobalsPanel();
  });
  ui.pillBox?.addEventListener("click", () => {
    toggleRuntimePanel(ui.pillBox, ui.boxPanel);
  });
  BOX_FLAGS.forEach((flag) => {
    ui.boxButtons[flag]?.addEventListener("click", () => {
      toggleBoxFlag(flag);
      console.log(`[repl] box -> ${ensureSession().get_box_summary()}\n`);
    });
  });
  ui.pillTime?.addEventListener("click", () => {
    timeMode = !timeMode;
    setActive(ui.pillTime, timeMode);
    console.log(`[repl] time mode -> ${timeMode ? "on" : "off"}\n`);
  });
  DEBUG_FLAGS.forEach((flag) => {
    ui.debugButtons[flag]?.addEventListener("click", () => {
      toggleDebugFlag(flag);
    });
  });

  ui.debugToggle?.addEventListener("click", () => {
    toggleRuntimePanel(ui.debugToggle, ui.debugPanel);
  });
  ui.terminalMenu?.addEventListener("click", (event) => {
    if (!event.target.closest(".repl-terminal-menu-panel button")) return;
    setTimeout(() => {
      ui.terminalMenu.open = false;
    }, 900);
  });
  // Close debug panel and history search on outside click
  document.addEventListener("click", (e) => {
    if (
      ui.boxPanel?.classList.contains("open") &&
      !ui.boxPanel.contains(e.target) &&
      !ui.pillBox?.contains(e.target)
    ) {
      closeRuntimePanel(ui.pillBox, ui.boxPanel);
    }
    if (
      ui.debugPanel?.classList.contains("open") &&
      !ui.debugPanel.contains(e.target) &&
      !ui.debugToggle?.contains(e.target)
    ) {
      closeRuntimePanel(ui.debugToggle, ui.debugPanel);
    }
    if (
      ui.historySearch &&
      !ui.historySearch.hidden &&
      !ui.historySearch.contains(e.target) &&
      !ui.historyToggleBtn?.contains(e.target)
    ) {
      closeHistorySearch();
    }
    if (ui.terminalMenu?.open && !ui.terminalMenu.contains(e.target)) {
      ui.terminalMenu.open = false;
    }
  });
  window.addEventListener("resize", () => {
    positionHistorySearch();
    positionRuntimePanel(ui.pillBox, ui.boxPanel);
    positionRuntimePanel(ui.debugToggle, ui.debugPanel);
  });
  window.visualViewport?.addEventListener("resize", () => {
    positionHistorySearch();
    positionRuntimePanel(ui.pillBox, ui.boxPanel);
    positionRuntimePanel(ui.debugToggle, ui.debugPanel);
  });
  window.visualViewport?.addEventListener("scroll", () => {
    positionHistorySearch();
    positionRuntimePanel(ui.pillBox, ui.boxPanel);
    positionRuntimePanel(ui.debugToggle, ui.debugPanel);
  });

  ui.stopBtn.addEventListener("click", () => {
    evaluationController.stop("stop requested");
  });
  ui.promptForm.addEventListener("submit", (e) => {
    e.preventDefault();
    doEval();
  });
  ui.clearBtn.addEventListener("click", () => {
    clearScreen({ focusInput: true });
  });
  ui.term.addEventListener("click", (event) => {
    if (event.target === ui.term && !window.getSelection()?.toString()) {
      ui.codeEl.focus();
    }
  });
  ui.codeEl.addEventListener("input", () => syncInputPresentation());
  ui.codeEl.addEventListener("keydown", (e) => {
    if (isImeCompositionKey(e, ui.codeEl.isComposing)) return;
    if (e.key === "Escape" && evaluationController.active) {
      e.preventDefault();
      evaluationController.stop("stop requested");
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key === "r") {
      e.preventDefault();
      openHistorySearch();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key === "l") {
      e.preventDefault();
      clearScreen({ focusInput: true });
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      doEval();
    } else if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      const source = ui.codeEl.value;
      if (
        source.trimStart().startsWith("\\") ||
        frontend.is_complete_input(source)
      ) {
        doEval();
      } else {
        insertTextAtCursor(ui.codeEl, "\n");
        syncInputPresentation();
      }
    } else if (
      !e.shiftKey &&
      !e.ctrlKey &&
      !e.metaKey &&
      e.key === "ArrowUp" &&
      canNavigateHistory(-1)
    ) {
      if (history.length) {
        e.preventDefault();
        if (histIndex === -1) {
          pendingBuffer = ui.codeEl.value;
          histIndex = history.length - 1;
        } else if (histIndex > 0) {
          histIndex--;
        }
        ui.codeEl.value = history[histIndex];
        ui.codeEl.selectionStart = ui.codeEl.selectionEnd =
          ui.codeEl.value.length;
        syncInputPresentation();
      }
    } else if (
      !e.shiftKey &&
      !e.ctrlKey &&
      !e.metaKey &&
      e.key === "ArrowDown" &&
      canNavigateHistory(1)
    ) {
      if (history.length && histIndex !== -1) {
        e.preventDefault();
        if (histIndex < history.length - 1) {
          histIndex++;
          ui.codeEl.value = history[histIndex];
        } else {
          histIndex = -1;
          ui.codeEl.value = pendingBuffer;
        }
        ui.codeEl.selectionStart = ui.codeEl.selectionEnd =
          ui.codeEl.value.length;
        syncInputPresentation();
      }
    } else if (e.key === "Tab") {
      handleReplTabKey(e);
    }
  });

  root.addEventListener("wqide:deactivate", () => {
    evaluationController.stop("view closed");
  });

  syncInputPresentation();
  setInspectorTab(inspectorTab);
  setInspectorOpen(inspectorOpen);
  syncWqdbPanel();
  resetSession();
}

export function activateRepl() {
  if (!ui) return;
  bindRuntimeCallbacks();
  syncBoxControl();
  syncDebugControls();
  syncGlobalsPanel();
  syncWqdbPanel();
}

export function applyReplRoute(root, params) {
  if (!ui) return;
  const input = params.get("input");
  if (!input) return;
  ui.codeEl.value = input;
  syncInputPresentation();
  doEval({ recordHistory: false }).then(() => {
    ui.codeEl.focus();
  });
  return true;
}
