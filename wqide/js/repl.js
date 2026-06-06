import {
  WasmWqSession,
  set_stdout_callback,
  set_stderr_callback,
  set_stdin_callback,
  highlight_wq,
} from "wq-wasm";
import { createAnsiRenderer } from "./ansi.js";
import { createCmOverlay } from "./cm-overlay.js";
import {
  ensureWasm,
  getWqVersion,
  DEBUG_FLAGS,
  parseDebugFlags,
  formatDebugFlags,
  setActive,
  syncDebugButtons,
  alignTurnBody,
  insertTextAtCursor,
  handleTabKey,
  fallbackCopyText,
  queueEval,
} from "./wq-shared.js";

let session = null;
let stdinQueue = [];
let replOverlay = null;
let history = [];
let histIndex = -1;
let pendingBuffer = "";
let timeMode = false;
let currentTurn = null;
let execCounter = 1;
let ui = null;
let autoScroll = true;
let userScrolledUp = false;
let scrollTimeout = null;

const HISTORY_KEY = "wqide:repl:history";
const HISTORY_LIMIT = 200;

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

function setDebugFlags(flags) {
  const next = formatDebugFlags(flags);
  ensureSession().set_debug_flags(next);
  syncDebugControls();
  console.log(`[repl] debug flags -> ${next === "0" ? "off" : next}\n`);
}

function toggleDebugFlag(flag) {
  const current = getDebugFlags();
  const next = current.includes(flag)
    ? current.filter((item) => item !== flag)
    : [...current, flag];
  setDebugFlags(next);
}

function syncDebugControls() {
  syncDebugButtons(ui?.debugButtons, getDebugFlags());
}

function syncBoxControl() {
  setActive(ui?.pillBox, ensureSession().get_box_mode());
}

function promptPrefix() {
  return "wq[" + execCounter + "] ";
}

function scrollThreadToBottom(mode = "composer") {
  if (!ui?.term) return;
  if (!autoScroll && !mode.startsWith("force")) return;
  if (mode === "current-turn" && currentTurn) {
    const turn = currentTurn.closest(".repl-turn");
    if (turn) {
      turn.scrollIntoView({ block: "end", behavior: "smooth" });
      return;
    }
  }
  ui.term.scrollTo({ top: ui.term.scrollHeight, behavior: "smooth" });
}

function observeUserScroll() {
  if (!ui?.term) return;
  ui.term.addEventListener("scroll", () => {
    if (scrollTimeout) clearTimeout(scrollTimeout);
    const nearBottom =
      ui.term.scrollHeight - ui.term.scrollTop - ui.term.clientHeight < 40;
    if (!nearBottom) {
      userScrolledUp = true;
      autoScroll = false;
    } else {
      userScrolledUp = false;
      autoScroll = true;
    }
    scrollTimeout = setTimeout(() => {
      if (nearBottom) autoScroll = true;
    }, 150);
  });
}

function setupViewportHandler() {
  if (!window.visualViewport) return;
  const vv = window.visualViewport;
  const onResize = () => {
    if (!ui?.term || !ui?.composerForm) return;
    const offset =
      vv.height < window.innerHeight ? window.innerHeight - vv.height : 0;
    if (offset > 60) {
      // keyboard likely open
      ui.term.style.paddingBottom = `${Math.min(offset, 280)}px`;
      scrollThreadToBottom("force-keyboard");
    } else {
      ui.term.style.paddingBottom = "18px";
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
    prompt.textContent = label + " ";
    content.appendChild(prompt);
    const codeSpan = document.createElement("span");
    codeSpan.className = "repl-input-code";
    codeSpan.innerHTML = highlight_wq(body || "");
    content.appendChild(codeSpan);

    turn.style.cursor = "pointer";
    turn.title = "Click to reuse this command";
    turn.addEventListener("click", () => {
      const text = body ?? "";
      ui.codeEl.value = text;
      replOverlay?.update();
      autoSizeComposer();
      ui.codeEl.focus();
    });
  } else {
    const bar = document.createElement("span");
    bar.className = `repl-bar repl-bar-${msgType || "info"}`;
    bar.textContent = "\u258d ";
    content.appendChild(bar);
    content.__ansiRenderer = createAnsiRenderer(content, bar);
    if (body) {
      content.__ansiRenderer.append(body);
    }
  }

  line.appendChild(content);
  turn.appendChild(line);
  ui.term.appendChild(turn);

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
  const startX = 0;
  const startY = 0;

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
  menu.style.cssText = `
    position: fixed;
    left: ${x}px;
    top: ${y}px;
    background: #ffffff;
    border: 1px solid #cfe6f6;
    border-radius: 10px;
    box-shadow: 0 8px 24px rgba(19,78,110,0.16);
    z-index: 1000;
    padding: 6px 0;
    min-width: 140px;
    font-size: 14px;
  `;

  const addItem = (label, action) => {
    const item = document.createElement("button");
    item.type = "button";
    item.textContent = label;
    item.style.cssText = `
      display: block; width: 100%; text-align: left;
      padding: 8px 14px; border: 0; background: transparent;
      cursor: pointer; font: inherit; color: #0b4060;
    `;
    item.addEventListener("pointerup", () => {
      action();
      menu.remove();
      activeMenu = null;
    });
    item.addEventListener("mouseenter", () => {
      item.style.background = "#f0f8ff";
    });
    item.addEventListener("mouseleave", () => {
      item.style.background = "transparent";
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
      replOverlay?.update();
      autoSizeComposer();
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

function append(chunk, msgType = "info") {
  console.log(chunk);
  const turn = document.createElement("article");
  turn.className = "repl-turn repl-turn-system";
  const line = document.createElement("div");
  line.className = "repl-line";
  const content = document.createElement("pre");
  content.className = "repl-line-body repl-line-body-system";
  content.__ansiRenderer = createAnsiRenderer(content);
  const aligned = alignTurnBody(chunk);
  if (msgType === "error") {
    content.__ansiRenderer.append("\x1b[31m" + aligned + "\x1b[0m");
  } else {
    content.__ansiRenderer.append(aligned);
  }
  line.appendChild(content);
  turn.appendChild(line);
  ui.term.appendChild(turn);
  scrollThreadToBottom();
}

function bindRuntimeCallbacks() {
  set_stdout_callback((chunk) => append(chunk, "info"));
  set_stderr_callback((chunk) => append(chunk, "error"));
  set_stdin_callback((p) => {
    if (stdinQueue.length > 0) return String(stdinQueue.shift());
    const msg = typeof p === "string" ? p : "stdin:";
    const ans = window.prompt(msg || "stdin:");
    if (ans === null) return null;
    return ans;
  });
}

function ensureSession() {
  if (!session) {
    session = new WasmWqSession();
  }
  return session;
}

function autoSizeComposer() {
  if (replOverlay) {
    replOverlay.resize();
    return;
  }
  ui.codeEl.style.height = "0px";
  const nextHeight = Math.min(Math.max(ui.codeEl.scrollHeight, 44), 160);
  ui.codeEl.style.height = `${nextHeight}px`;
}

function setButtonStatus(btn, label) {
  if (!btn) return;
  const idle = btn.dataset.idleLabel || btn.textContent;
  btn.dataset.idleLabel = idle;
  btn.textContent = label;
  const idleWidth = Number(btn.dataset.idleWidth || 0);
  btn.style.width = "auto";
  const nextWidth = Math.ceil(btn.getBoundingClientRect().width);
  btn.style.width = `${Math.max(idleWidth, nextWidth)}px`;
}

function resetButtonStatus(btn) {
  if (!btn) return;
  btn.textContent = btn.dataset.idleLabel || btn.textContent;
  const idleWidth = Number(btn.dataset.idleWidth || 0);
  if (idleWidth > 0) {
    btn.style.width = `${idleWidth}px`;
  }
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
  const composerText = ui.codeEl.value.trim();
  if (composerText) {
    parts.push(`${promptPrefix().trim()} ${composerText}`);
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
  session = null;
  stdinQueue = [];
  // Keep history across resets
  pendingBuffer = "";
  execCounter = 1;
  currentTurn = null;
  ui.term.innerHTML = "";
  bindRuntimeCallbacks();
  ensureSession();
  append(`wq ${getWqVersion()} (c)tttiw (l)MIT\n`);
  syncBoxControl();
  setActive(ui.pillTime, false);
  syncDebugControls();
}

async function doEval() {
  const code = ui.codeEl.value;
  if (!code.trim()) return;
  autoScroll = true;
  createTurn("input", promptPrefix().trim(), code.trim());
  execCounter++;
  ui.evalBtn.disabled = true;
  try {
    const start = performance.now();
    const result = await queueEval(() => {
      bindRuntimeCallbacks();
      return ensureSession().eval_wq_result(code);
    });
    const end = performance.now();
    if (!history.length || history[history.length - 1] !== code) {
      history.push(code);
      saveHistory();
    }
    histIndex = -1;
    pendingBuffer = "";
    if (
      result.value !== undefined &&
      result.value !== null &&
      String(result.value).length
    ) {
      if (result.is_cas) {
        const content = createTurn("output", "", "", "success");
        const casSpan = document.createElement("span");
        casSpan.innerHTML = highlight_wq(alignTurnBody(result.value));
        content.appendChild(casSpan);
      } else {
        createTurn(
          "output",
          "",
          alignTurnBody(String(result.value)) + "\n",
          "success",
        );
      }
      if (timeMode === true) {
        append(`time elapsed: ${end - start}ms\n`, "info");
      }
    }
    ui.codeEl.value = "";
    replOverlay?.update();
    autoSizeComposer();
  } catch (err) {
    console.error("err from wq:" + err);
    createTurn(
      "system",
      "",
      alignTurnBody((err?.message ?? String(err)) + "\n"),
      "error",
    );
  } finally {
    ui.evalBtn.disabled = false;
    currentTurn = null;
  }
}

function handleReplTabKey(e) {
  handleTabKey(e, ui.codeEl, () => {
    autoSizeComposer();
    replOverlay?.update();
  });
}

function openHistorySearch() {
  if (!ui.historySearch) return;
  ui.historySearch.hidden = false;
  const input = ui.historySearchInput;
  const results = ui.historySearchResults;
  input.value = ui.codeEl.value;
  input.focus();

  function update() {
    const q = input.value.toLowerCase();
    const matches = history.slice().reverse().filter((h) => h.toLowerCase().includes(q));
    results.innerHTML = "";
    if (!matches.length) {
      results.innerHTML = "<span style='padding:6px 10px;color:#6a8da8;font-size:13px;'>No matches</span>";
      return;
    }
    matches.forEach((text) => {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.textContent = text;
      btn.addEventListener("click", () => {
        ui.codeEl.value = text;
        replOverlay?.update();
        autoSizeComposer();
        ui.historySearch.hidden = true;
        ui.codeEl.focus();
      });
      results.appendChild(btn);
    });
  }

  update();
  input.oninput = update;
  input.onkeydown = (e) => {
    if (e.key === "Escape") {
      ui.historySearch.hidden = true;
      ui.codeEl.focus();
    }
  };
}

export async function mountRepl(root) {
  await ensureWasm();
  ui = {
    codeEl: root.querySelector("#code"),
    term: root.querySelector("#term"),
    composerForm: root.querySelector("#composerForm"),
    evalBtn: root.querySelector("#evalBtn"),
    clearBtn: root.querySelector("#clearBtn"),
    resetBtn: root.querySelector("#resetBtn"),
    copyFlowBtn: root.querySelector("#copyFlowBtn"),
    copyOutputBtn: root.querySelector("#copyOutputBtn"),
    stdinLine: root.querySelector("#stdinLine"),
    pushStdinBtn: root.querySelector("#pushStdinBtn"),
    pillBox: root.querySelector("#pillBox"),
    pillTime: root.querySelector("#pillTime"),
    newlineBtn: root.querySelector("#newlineBtn"),
    debugToggle: root.querySelector("#debugToggle"),
    debugPanel: root.querySelector("#debugPanel"),
    debugButtons: Object.fromEntries(
      DEBUG_FLAGS.map((flag) => [
        flag,
        root.querySelector(`[data-debug-flag="${flag}"]`),
      ]),
    ),
    openInPlaygroundBtn: root.querySelector("#openInPlaygroundBtn"),
    historySearch: root.querySelector("#historySearch"),
    historySearchInput: root.querySelector("#historySearchInput"),
    historySearchResults: root.querySelector("#historySearchResults"),
  };
  const cmRoot = root.querySelector(".repl-cm-editor");
  if (cmRoot) {
    replOverlay = createCmOverlay(cmRoot, {
      highlight: highlight_wq,
      onExec: () => doEval(),
    });
  }

  bindRuntimeCallbacks();
  loadHistory();
  observeUserScroll();
  setupViewportHandler();

  [ui.copyFlowBtn, ui.copyOutputBtn].forEach((btn) => {
    if (!btn) return;
    btn.dataset.idleLabel = btn.textContent;
    requestAnimationFrame(() => {
      const rect = btn.getBoundingClientRect();
      const idleWidth = Math.ceil(rect.width);
      const idleHeight = Math.ceil(rect.height);
      btn.dataset.idleWidth = String(idleWidth);
      btn.style.width = `${idleWidth}px`;
      btn.style.height = `${idleHeight}px`;
    });
  });

  ui.resetBtn.addEventListener("click", () => resetSession());
  ui.copyFlowBtn?.addEventListener("click", () => {
    copyCurrentFlow();
  });
  ui.copyOutputBtn?.addEventListener("click", () => {
    copyCurrentOutput();
  });
  ui.openInPlaygroundBtn?.addEventListener("click", () => {
    let code = ui.codeEl.value.trim();
    if (!code && history.length) {
      code = history[history.length - 1];
    }
    if (!code) return;
    window.navigate(`playground.html?code=${encodeURIComponent(code)}`);
  });
  ui.pillBox?.addEventListener("click", () => {
    const on = ensureSession().toggle_box_mode();
    setActive(ui.pillBox, on);
    console.log(`[repl] box mode -> ${on ? "on" : "off"}\n`);
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

  // Mobile debug toggle
  ui.debugToggle?.addEventListener("click", () => {
    const isOpen = ui.debugPanel.classList.toggle("open");
    ui.debugToggle.setAttribute("aria-expanded", String(isOpen));
  });
  // Close debug panel and history search on outside click
  document.addEventListener("click", (e) => {
    if (
      ui.debugPanel?.classList.contains("open") &&
      !ui.debugPanel.contains(e.target) &&
      !ui.debugToggle?.contains(e.target)
    ) {
      ui.debugPanel.classList.remove("open");
      ui.debugToggle?.setAttribute("aria-expanded", "false");
    }
    if (
      ui.historySearch &&
      !ui.historySearch.hidden &&
      !ui.historySearch.contains(e.target)
    ) {
      ui.historySearch.hidden = true;
    }
  });

  ui.pushStdinBtn.addEventListener("click", () => {
    const text = ui.stdinLine.value;
    if (!text) return;
    const normalized = text.replace(/\\n/g, "\n");
    const lines = normalized.includes("\n")
      ? normalized.split(/\r?\n/)
      : [normalized];
    try {
      ensureSession();
      stdinQueue.push(...lines);
      append(`pushed ${lines.length} line(s) to stdin\n`);
      ui.stdinLine.value = "";
    } catch (e) {
      console.error(e);
      append((e?.message ?? String(e)) + "\n");
    }
  });
  ui.composerForm.addEventListener("submit", (e) => {
    e.preventDefault();
    doEval();
  });
  ui.clearBtn.addEventListener("click", () => {
    ui.term.innerHTML = "";
    currentTurn = null;
    autoSizeComposer();
  });
  ui.codeEl.addEventListener("input", () => autoSizeComposer());
  ui.codeEl.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === "r") {
      e.preventDefault();
      openHistorySearch();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      doEval();
    } else if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      doEval();
    } else if (!e.shiftKey && !e.ctrlKey && !e.metaKey && e.key === "ArrowUp") {
      if (history.length) {
        e.preventDefault();
        if (histIndex === -1) {
          pendingBuffer = ui.codeEl.value;
          histIndex = history.length - 1;
        } else if (histIndex > 0) {
          histIndex--;
        }
        ui.codeEl.value = history[histIndex];
        replOverlay?.update();
        ui.codeEl.selectionStart = ui.codeEl.selectionEnd =
          ui.codeEl.value.length;
        autoSizeComposer();
      }
    } else if (
      !e.shiftKey &&
      !e.ctrlKey &&
      !e.metaKey &&
      e.key === "ArrowDown"
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
        replOverlay?.update();
        ui.codeEl.selectionStart = ui.codeEl.selectionEnd =
          ui.codeEl.value.length;
        autoSizeComposer();
      }
    } else if (e.key === "Tab" && !replOverlay) {
      handleReplTabKey(e);
    }
  });

  ui.newlineBtn?.addEventListener("click", () => {
    insertTextAtCursor(ui.codeEl, "\n");
    replOverlay?.update();
    autoSizeComposer();
  });

  autoSizeComposer();
  resetSession();
}

export function activateRepl() {
  if (!ui) return;
  bindRuntimeCallbacks();
  syncBoxControl();
  syncDebugControls();
}

export function applyReplRoute(root, params) {
  if (!ui) return;
  const input = params.get("input");
  if (!input) return;
  ui.codeEl.value = decodeURIComponent(input);
  replOverlay?.update();
  autoSizeComposer();
  doEval().then(() => {
    ui.codeEl.focus();
  });
}
