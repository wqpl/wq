import { WasmWqSession } from "wq-wasm";
import { createOutputRenderer } from "./ansi.js";
import { createWqEditor, isImeCompositionKey } from "./editor.js";
import {
  createEvaluationController,
  isAbortError,
} from "./eval-lifecycle.js";
import { appendResultPresentation } from "./result-presentation.js";
import {
  createDomStdinRenderer,
  createStdinRequester,
} from "./stdin-request.js";
import { renderHighlightedSource } from "./syntax-highlight.js";
import {
  createOutputBar,
  ensureWasm,
  formatWqError,
  getWqFrontend,
  handleTabKey,
  insertTextAtCursor,
  queueEval,
} from "./wq-shared.js";
import { createWqdbController, renderWqdbPanel } from "./wqdb.js";
import {
  bookReplRoute,
  bookReplStatusLabel,
} from "./book-repl-core.js";

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function prompt(label, number) {
  const node = element(label, "book-repl-prompt");
  node.append(
    element("span", "repl-prompt-wq", "wq"),
    "[",
    element("span", "repl-prompt-num", String(number)),
    "]",
  );
  return node;
}

function appendInputTurn(log, frontend, source, number) {
  const turn = element("article", "repl-turn book-repl-turn");
  const line = element("div", "repl-line");
  const body = element(
    "pre",
    "repl-line-body repl-line-body-input book-repl-line-body",
  );
  const prefix = prompt("span", number);
  prefix.classList.add("repl-prompt");
  body.append(prefix, " ");
  const code = element("span", "repl-input-code");
  renderHighlightedSource(code, frontend, source);
  body.append(code);
  line.append(body);
  turn.append(line);
  log.append(turn);
}

function appendOutputTurn(log, kind) {
  const turn = element(
    "article",
    `repl-turn repl-turn-output book-repl-turn book-repl-turn-${kind}`,
  );
  const line = element("div", "repl-line");
  const body = element(
    "pre",
    "repl-line-body repl-line-body-output book-repl-line-body",
  );
  const bar = createOutputBar(kind);
  body.append(bar);
  body.__outputRenderer = createOutputRenderer(body, bar);
  line.append(body);
  turn.append(line);
  log.append(turn);
  return body;
}

function createShell(initialSource, options, contract) {
  const shell = element("section", "book-repl");
  shell.dataset.bookRepl = contract?.id || "repl";
  shell.dataset.evaluationState = "idle";
  shell.setAttribute("aria-label", "Interactive wq book REPL");

  const bar = element("header", "book-repl-bar");
  const identity = element("div", "book-repl-identity");
  identity.append(
    element("span", "repl-status-dot"),
    element("strong", "book-repl-title", "Book REPL"),
    element("span", "book-repl-status", "Loading"),
  );

  const actions = element("div", "book-repl-actions");
  const debug = element(
    "button",
    "code-action-btn book-repl-debug inactive",
    "Debug",
  );
  debug.type = "button";
  debug.dataset.bookReplDebug = "";
  debug.setAttribute("aria-pressed", String(options.debugger));
  const reset = element("button", "code-action-btn", "Reset");
  reset.type = "button";
  reset.dataset.bookReplReset = "";
  const open = element("a", "book-repl-open", "Open full REPL");
  open.href = bookReplRoute(initialSource);
  actions.append(debug, reset, open);
  bar.append(identity, actions);

  const layout = element("div", "book-repl-layout");
  layout.dataset.debuggerOpen = String(options.debugger);
  const terminal = element("div", "book-repl-terminal");
  const log = element("div", "book-repl-log");
  log.setAttribute("role", "log");
  log.setAttribute("aria-live", "polite");
  log.setAttribute("aria-relevant", "additions");

  const form = element("form", "book-repl-composer");
  const composerHead = element("div", "book-repl-composer-head");
  const livePrompt = prompt("label", 1);
  livePrompt.htmlFor = `book-repl-input-${contract?.id || "repl"}`;
  const inputHint = element(
    "span",
    "book-repl-input-hint",
    "Enter runs · Shift+Enter adds a line",
  );
  composerHead.append(livePrompt, inputHint);
  const inputRow = element("div", "book-repl-input-row");
  const textarea = element("textarea", "editor-text book-repl-input");
  textarea.id = livePrompt.htmlFor;
  textarea.value = initialSource;
  textarea.rows = 2;
  textarea.spellcheck = false;
  textarea.placeholder = "Enter a wq expression";
  textarea.setAttribute("aria-label", "Book REPL code");
  textarea.setAttribute("enterkeyhint", "send");
  const run = element("button", "code-action-btn book-repl-run", "Run");
  run.type = "submit";
  run.dataset.action = "run";
  run.dataset.bookReplRun = "";
  inputRow.append(textarea, run);
  form.append(composerHead, inputRow);
  terminal.append(log, form);

  const inspector = element(
    "aside",
    "globals-panel book-repl-debugger",
  );
  inspector.hidden = !options.debugger;
  inspector.dataset.debuggerState = "idle";
  inspector.setAttribute("aria-label", "Book REPL debugger");
  const inspectorHead = element(
    "header",
    "globals-panel-head book-repl-debugger-head",
  );
  inspectorHead.append(
    element("strong", "book-repl-debugger-title", "Debugger"),
    element("span", "wqdb-panel-status", "Idle"),
  );
  const inspectorBody = element(
    "div",
    "globals-panel-body wqdb-panel-body book-repl-debugger-body",
  );
  inspectorBody.setAttribute("aria-live", "polite");
  inspector.append(inspectorHead, inspectorBody);
  layout.append(terminal, inspector);
  shell.append(bar, layout);

  return {
    shell,
    layout,
    log,
    form,
    inputRow,
    textarea,
    livePrompt,
    run,
    reset,
    debug,
    open,
    status: identity.querySelector(".book-repl-status"),
    inspector,
    inspectorBody,
    inspectorStatus: inspectorHead.querySelector(".wqdb-panel-status"),
  };
}

export async function mountBookRepl(pre, { source, contract, options }) {
  const initialSource = String(source).trimEnd();
  const view = createShell(initialSource, options, contract);
  pre.replaceWith(view.shell);

  try {
    await ensureWasm();
  } catch (error) {
    view.status.textContent = "Unavailable";
    const output = appendOutputTurn(view.log, "error");
    output.__outputRenderer.appendOutput(formatWqError(error), "error");
    return;
  }
  if (!view.shell.isConnected) return;

  const frontend = getWqFrontend();
  const editor = createWqEditor(view.textarea, {
    multilineMode: "shift",
    frontend,
  });
  let session = null;
  let turnNumber = 1;
  let lastSource = initialSource;
  let debuggerEnabled = options.debugger;
  let streamOutput = null;
  let streamKind = null;
  let resetRequested = false;

  const stdinRequester = createStdinRequester({
    render: createDomStdinRenderer(view.log),
  });

  function ensureSession() {
    if (!session) {
      session = new WasmWqSession();
      session.set_box_flags("0");
      session.set_wqdb_mode(debuggerEnabled);
    }
    return session;
  }

  function syncOpenLink() {
    const current = editor.value.trim() || lastSource;
    view.open.href = bookReplRoute(current);
  }

  function syncPrompt() {
    const number = view.livePrompt.querySelector(".repl-prompt-num");
    if (number) number.textContent = String(turnNumber);
  }

  function setDebuggerOpen(open) {
    view.layout.dataset.debuggerOpen = String(open);
    view.inspector.hidden = !open;
  }

  function syncDebugger(state) {
    view.inspector.dataset.debuggerState = state.status;
    view.inspectorStatus.textContent =
      state.status === "paused"
        ? "Paused"
        : state.status === "running"
          ? "Running"
          : "Idle";
    renderWqdbPanel(view.inspectorBody, state, wqdbController, {
      frontend,
      emptyMessage: "Run the expression to pause before its first instruction.",
    });
    if (state.status === "paused") setDebuggerOpen(true);
  }

  const wqdbController = createWqdbController(syncDebugger);

  function setDebuggerEnabled(on) {
    debuggerEnabled = on;
    view.debug.classList.toggle("active", on);
    view.debug.classList.toggle("inactive", !on);
    view.debug.setAttribute("aria-pressed", String(on));
    session?.set_wqdb_mode(on);
    if (!on) wqdbController.reset();
    setDebuggerOpen(on);
  }

  function appendStream(chunk, kind) {
    if (!streamOutput || streamKind !== kind) {
      streamOutput = appendOutputTurn(view.log, kind);
      streamKind = kind;
    }
    streamOutput.__outputRenderer.appendStreamOutput(
      chunk,
      kind === "error" ? "error" : null,
    );
    view.log.scrollTop = view.log.scrollHeight;
  }

  function bindSessionCallbacks(activeSession, { signal, setState }) {
    activeSession.set_stdout_callback((chunk) => appendStream(chunk, "info"));
    activeSession.set_stderr_callback((chunk) => appendStream(chunk, "error"));
    activeSession.set_stdin_callback(async (requestPrompt) => {
      setState("awaiting-input");
      try {
        return await stdinRequester.request(requestPrompt, { signal });
      } finally {
        if (!signal.aborted) setState("running");
      }
    });
  }

  function clearSessionCallbacks(activeSession) {
    activeSession.set_stdout_callback(null);
    activeSession.set_stderr_callback(null);
    activeSession.set_stdin_callback(null);
  }

  const evaluationController = createEvaluationController((state) => {
    const active = state !== "idle";
    view.shell.dataset.evaluationState = state;
    view.status.textContent = bookReplStatusLabel(state);
    view.run.textContent = active ? "Stop" : "Run";
    view.run.classList.toggle("code-action-danger", active);
    view.run.disabled = state === "stopping";
    view.reset.disabled = active;
    view.debug.disabled = active;
    editor.element.setAttribute("contenteditable", String(!active));
    editor.element.setAttribute("aria-disabled", String(active));
  });

  async function evaluate() {
    const sourceText = editor.value.trim();
    if (!sourceText || evaluationController.active) return;
    const activeTurn = turnNumber;
    lastSource = sourceText;
    appendInputTurn(view.log, frontend, sourceText, activeTurn);
    turnNumber += 1;
    syncPrompt();
    editor.value = "";
    syncOpenLink();
    streamOutput = null;
    streamKind = null;

    try {
      const result = await evaluationController.run(({ signal, setState }) =>
        queueEval(
          async () => {
            setState("running");
            const activeSession = ensureSession();
            activeSession.set_wqdb_mode(debuggerEnabled);
            if (debuggerEnabled) activeSession.arm_wqdb_next();
            bindSessionCallbacks(activeSession, { signal, setState });
            try {
              return await activeSession.eval_wq_async(sourceText, {
                signal,
                sourcePath: `<book-repl:${contract?.id || "repl"}:${activeTurn}>`,
                onDebuggerPause(stop) {
                  setState("paused");
                  return wqdbController
                    .pause(stop, {
                      source: sourceText,
                      sourcePath: stop.sourcePath,
                    })
                    .finally(() => {
                      if (!signal.aborted) setState("running");
                    });
                },
                onDebuggerNotification(notification) {
                  wqdbController.recordNotification(notification);
                },
              });
            } finally {
              wqdbController.reset();
              clearSessionCallbacks(activeSession);
            }
          },
          { signal },
        ),
      );

      if (
        result.display !== undefined &&
        result.display !== null &&
        String(result.display).length
      ) {
        const output = appendOutputTurn(view.log, "success");
        if (!appendResultPresentation(output, result.presentation)) {
          output.__outputRenderer.appendOutput(String(result.display));
        }
      }
    } catch (error) {
      if (isAbortError(error) && !resetRequested) {
        const output = appendOutputTurn(view.log, "info");
        output.__outputRenderer.appendOutput("Interrupted");
      } else if (!isAbortError(error)) {
        const output = appendOutputTurn(view.log, "error");
        output.__outputRenderer.appendOutput(
          formatWqError(error, { rendered: true }),
          "error",
        );
      }
    } finally {
      if (resetRequested) {
        resetRequested = false;
        resetSession();
      }
      view.log.scrollTop = view.log.scrollHeight;
      syncOpenLink();
    }
  }

  function resetSession() {
    if (evaluationController.active) {
      resetRequested = true;
      evaluationController.stop("session reset");
      return;
    }
    stdinRequester.cancel("session reset");
    wqdbController.reset();
    const previous = session;
    session = null;
    previous?.free();
    turnNumber = 1;
    lastSource = initialSource;
    view.log.replaceChildren();
    editor.value = initialSource;
    syncPrompt();
    syncOpenLink();
    setDebuggerEnabled(options.debugger);
    view.status.textContent = "Ready";
  }

  view.form.addEventListener("submit", (event) => {
    event.preventDefault();
    if (evaluationController.active) {
      evaluationController.stop("stop requested");
    } else {
      evaluate();
    }
  });
  view.reset.addEventListener("click", resetSession);
  view.debug.addEventListener("click", () => {
    setDebuggerEnabled(!debuggerEnabled);
  });
  editor.addEventListener("input", syncOpenLink);
  editor.addEventListener("keydown", (event) => {
    if (isImeCompositionKey(event, editor.isComposing)) return;
    if (event.key === "Escape" && evaluationController.active) {
      event.preventDefault();
      evaluationController.stop("stop requested");
    } else if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
      event.preventDefault();
      evaluate();
    } else if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      if (frontend.is_complete_input(editor.value)) {
        evaluate();
      } else {
        insertTextAtCursor(editor, "\n");
      }
    } else if (event.key === "Tab") {
      handleTabKey(event, editor);
    }
  });

  const ownerView = view.shell.closest("[data-view]");
  ownerView?.addEventListener(
    "wqide:deactivate",
    () => {
      evaluationController.stop("view closed");
      stdinRequester.cancel("view closed");
      wqdbController.reset();
      session?.free();
      session = null;
    },
    { once: true },
  );

  setDebuggerEnabled(debuggerEnabled);
  syncDebugger(wqdbController.state);
  syncOpenLink();
  view.status.textContent = "Ready";
}
