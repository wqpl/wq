// Build outline from headings, scrollspy, copy-to-clipboard buttons after article content is injected.

import { WasmWqSession } from "wq-wasm";
import { createOutputRenderer } from "./ansi.js";
import { renderHighlightedSource } from "./syntax-highlight.js";
import { appendResultPresentation } from "./result-presentation.js";
import {
  ensureWasm,
  fallbackCopyText,
  getWqFrontend,
  queueEval,
  createOutputBar,
  formatWqError,
} from "./wq-shared.js";
import {
  abortError,
  createEvaluationController,
  isAbortError,
} from "./eval-lifecycle.js";
import {
  createDomStdinRenderer,
  createStdinRequester,
} from "./stdin-request.js";
import {
  exampleHeaderLabel,
  exampleOutcome,
  parseExampleContract,
} from "./tutorial-examples.js";
import {
  cellRunLabel,
  hasFinalResult,
  planCellRuns,
} from "./tutorial-cells.js";
import { mountBookRepl } from "./book-repl.js";
import { bookReplOptions } from "./book-repl-core.js";

let __outlineObserver = null;
let __outlineLockUntil = 0;
// Defer WASM init until the first Run click.

function canonicalResultText(result) {
  if (result?.presentation?.text !== undefined) {
    return String(result.presentation.text);
  }
  return String(result?.display ?? "").replace(/\x1b\[[0-9;]*m/g, "");
}

function registerWorkspaceModules(session, article, workspace) {
  if (!workspace) return;
  for (const moduleCode of article.querySelectorAll(
    "pre code[data-wq-example]",
  )) {
    const moduleContract = parseExampleContract(
      moduleCode.dataset.wqExample || "",
    );
    if (
      moduleContract?.workspace === workspace &&
      typeof moduleContract.file === "string"
    ) {
      session.register_module(moduleContract.file, moduleCode.textContent);
    }
  }
}

function ensureCellPanel(cell) {
  let panel = cell.wrapper.nextElementSibling;
  if (
    !panel ||
    !panel.classList?.contains("run-result") ||
    panel.dataset.cellResult !== cell.id
  ) {
    panel = document.createElement("div");
    panel.className = "run-result";
    panel.dataset.cellResult = cell.id;
    const head = document.createElement("div");
    head.className = "run-head";
    head.textContent = "Result";
    head.hidden = true;
    const preOut = document.createElement("pre");
    const codeOut = document.createElement("code");
    const inputHost = document.createElement("div");
    inputHost.className = "stdin-request-host";
    inputHost.dataset.stdinHost = "";
    preOut.appendChild(codeOut);
    panel.append(head, preOut, inputHost);
    cell.wrapper.parentNode.insertBefore(panel, cell.wrapper.nextSibling);
    cell.wrapper.classList.add("attached");
  }

  const codeOut = panel.querySelector("code");
  const inputHost = panel.querySelector("[data-stdin-host]");
  const outputRenderer =
    codeOut.__outputRenderer ||
    (codeOut.__outputRenderer = createOutputRenderer(codeOut));
  cell.stdinRequester ||= createStdinRequester({
    render: createDomStdinRenderer(inputHost),
  });
  return {
    panel,
    head: panel.querySelector(".run-head"),
    codeOut,
    inputHost,
    outputRenderer,
    stdinRequester: cell.stdinRequester,
  };
}

function setCellPanelHeading(view, heading) {
  view.head.textContent = heading;
  view.head.hidden = heading === "Result";
}

function resetCellPanel(cell, heading) {
  const view = ensureCellPanel(cell);
  setCellPanelHeading(view, heading);
  delete view.panel.dataset.outcome;
  view.inputHost.innerHTML = "";
  view.outputRenderer.clear();
  return view;
}

function packCellRun(run) {
  if (!run.id || run.cells.length < 2) return null;
  const wrappers = run.cells.map((cell) => cell.wrapper);
  const contiguous = wrappers.every(
    (wrapper, index) =>
      index === wrappers.length - 1 ||
      wrapper.nextElementSibling === wrappers[index + 1],
  );
  if (!contiguous) return null;

  const group = document.createElement("section");
  group.className = "tutorial-cell-group";
  group.dataset.cellGroup = run.id;
  group.setAttribute("aria-label", "Runnable wq cell group");

  const groupHeader = document.createElement("header");
  groupHeader.className = "tutorial-cell-group-header";
  const language = document.createElement("span");
  language.className = "tutorial-cell-group-language";
  language.textContent = run.cells[0].label.textContent || "wq";
  const groupActions = document.createElement("div");
  groupActions.className = "code-actions";
  const copyAllButton = document.createElement("button");
  copyAllButton.type = "button";
  copyAllButton.className = "code-action-btn code-action-quiet";
  copyAllButton.dataset.action = "copy-all";
  copyAllButton.textContent = "Copy all";
  bindCopyButton(copyAllButton, () =>
    run.cells.map((cell) => cell.source).join("\n"),
  );
  groupActions.appendChild(copyAllButton);
  groupHeader.append(language, groupActions);
  group.appendChild(groupHeader);

  wrappers[0].parentNode.insertBefore(group, wrappers[0]);
  run.cells.forEach((cell, index) => {
    const cellRow = document.createElement("div");
    cellRow.className = "tutorial-cell";
    cellRow.setAttribute("role", "group");
    cellRow.setAttribute("aria-label", `Cell ${index + 1}`);
    const cellGutter = document.createElement("div");
    cellGutter.className = "tutorial-cell-gutter";
    const cellIndex = document.createElement("span");
    cellIndex.className = "tutorial-cell-index";
    cellIndex.setAttribute("aria-hidden", "true");
    cellIndex.textContent = String(index + 1);
    cellGutter.appendChild(cellIndex);
    const copyButton = cell.actions.querySelector('[data-action="copy"]');
    if (copyButton) {
      copyButton.classList.add("tutorial-cell-copy");
      copyButton.dataset.copyLabel = `Copy cell ${index + 1}`;
      copyButton.setAttribute("aria-label", copyButton.dataset.copyLabel);
    }
    const cellContent = document.createElement("div");
    cellContent.className = "tutorial-cell-content";
    cell.wrapper.querySelector(".code-header")?.remove();
    cellContent.appendChild(cell.wrapper);
    if (copyButton) cellContent.appendChild(copyButton);
    cellRow.append(cellGutter, cellContent);
    group.appendChild(cellRow);
  });

  return { actions: groupActions };
}

function packSingleCell(cell) {
  const shell = document.createElement("section");
  shell.className = "tutorial-single-cell";
  shell.setAttribute("aria-label", "Runnable wq example");
  cell.wrapper.parentNode.insertBefore(shell, cell.wrapper);
  shell.appendChild(cell.wrapper);
}

function animateButtonWidth(btn, newText) {
  // Clear any stale inline width from a previous animation
  btn.style.width = "";
  const currentWidth = btn.getBoundingClientRect().width;

  // Lock to current width
  btn.style.width = currentWidth + "px";

  // Measure target width after text change
  btn.textContent = newText;
  btn.style.width = "auto";
  const targetWidth = btn.getBoundingClientRect().width;

  // Reset to current width and animate to target
  btn.style.width = currentWidth + "px";
  btn.offsetWidth; // force reflow
  btn.style.width = targetWidth + "px";

  // Clean up inline width after transition, letting the button be auto-sized again
  setTimeout(() => {
    btn.style.width = "";
  }, 300);
}

function bindCopyButton(btn, getText) {
  btn.addEventListener("click", async () => {
    const cellCopy = btn.classList.contains("tutorial-cell-copy");
    try {
      await fallbackCopyText(getText());
      if (cellCopy) {
        btn.setAttribute(
          "aria-label",
          btn.dataset.copyLabel.replace("Copy", "Copied"),
        );
      }
      animateButtonWidth(btn, "Copied");
    } catch {
      if (cellCopy) btn.setAttribute("aria-label", "Copy failed");
      animateButtonWidth(btn, "Error");
    }
    setTimeout(() => {
      if (cellCopy) btn.setAttribute("aria-label", btn.dataset.copyLabel);
      animateButtonWidth(
        btn,
        btn.dataset.action === "copy-all" ? "Copy all" : "Copy",
      );
    }, 1400);
  });
}

window.initTutorialUI = function initTutorialUI() {
  const article =
    document.querySelector(".article[data-active-article='true']") ||
    document.querySelector(".article");
  const outlineList = document.querySelector("#outlineList");
  const mobileOutline = document.querySelector("#mobileOutline");

  if (article && outlineList) {
    if (__outlineObserver) {
      __outlineObserver.disconnect();
      __outlineObserver = null;
    }

    // Reset any existing outline
    outlineList.innerHTML = "";
    if (mobileOutline) mobileOutline.innerHTML = "";

    const articleKey =
      article.getAttribute("data-article-slug") ||
      article.getAttribute("data-view") ||
      "article";
    const headings = Array.from(article.querySelectorAll("h2, h3"));
    headings.forEach((h, idx) => {
      h.id = `${articleKey}-sec-${idx + 1}`;
      const a = document.createElement("a");
      a.href = "#" + h.id;
      a.textContent = h.textContent;
      if (h.tagName === "H3") a.classList.add("sub");
      outlineList.appendChild(a);
      if (mobileOutline) {
        const ma = a.cloneNode(true);
        mobileOutline.appendChild(ma);
      }
    });

    const links = Array.from(outlineList.querySelectorAll("a"));
    const mlinks = mobileOutline
      ? Array.from(mobileOutline.querySelectorAll("a"))
      : [];

    function activate(id) {
      links.forEach((l) =>
        l.classList.toggle("active", l.getAttribute("href") === "#" + id),
      );
      mlinks.forEach((l) =>
        l.classList.toggle("active", l.getAttribute("href") === "#" + id),
      );
    }
    __outlineObserver = new IntersectionObserver(
      (entries) => {
        if (Date.now() < __outlineLockUntil) return;

        const visible = entries
          .filter((e) => e.isIntersecting)
          .sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top);
        if (visible[0]) {
          activate(visible[0].target.id);
        }
      },
      { rootMargin: "-20% 0px -65% 0px", threshold: [0, 0.1, 0.25, 0.5, 1] },
    );
    headings.forEach((h) => __outlineObserver.observe(h));

    // smooth scroll
    function hookup(list) {
      list.forEach((a) => {
        a.addEventListener("click", (e) => {
          e.preventDefault();
          const id = a.getAttribute("href").slice(1);
          const target = article.querySelector(`#${CSS.escape(id)}`);
          if (target) {
            __outlineLockUntil = Date.now() + 700;
            const top =
              window.scrollY +
              target.getBoundingClientRect().top -
              (parseInt(
                getComputedStyle(document.documentElement).getPropertyValue(
                  "--header-h",
                ),
              ) +
                16);
            window.scrollTo({ top, behavior: "smooth" });
            activate(id);
          }
        });
      });
    }
    hookup(links);
    hookup(mlinks);
  }

  if (!article) return;

  const evaluationControllers =
    article.__evaluationControllers || new Set();
  article.__evaluationControllers = evaluationControllers;
  if (!article.__evaluationDeactivationBound) {
    article
      .closest("[data-view]")
      ?.addEventListener("wqide:deactivate", () => {
        for (const controller of evaluationControllers) {
          controller.stop("view closed");
        }
      });
    article.__evaluationDeactivationBound = true;
  }

  // Async highlight for wq code blocks
  (async () => {
    await ensureWasm();
    const frontend = getWqFrontend();
    article.querySelectorAll("pre code.language-wq").forEach((codeEl) => {
      const raw = codeEl.textContent;
      renderHighlightedSource(codeEl, frontend, raw);
    });
  })();

  // Enhance code blocks: wrap pre in .code-wrapper with header and copy button.
  const runnableCells = [];
  article.querySelectorAll("pre").forEach((pre, cellIndex) => {
    if (pre.closest(".run-result")) return;
    if (
      pre.parentElement &&
      pre.parentElement.classList.contains("code-wrapper")
    )
      return;
    const wrapper = document.createElement("div");
    wrapper.className = "code-wrapper";
    const header = document.createElement("div");
    header.className = "code-header";

    // Detect language from code class e.g. language-js
    const codeEl = pre.querySelector("code");
    let lang = "";
    let codeMeta = [];
    let contract = null;
    if (codeEl) {
      const m = Array.from(codeEl.classList).find((c) =>
        c.startsWith("language-"),
      );
      if (m) lang = m.replace("language-", "").trim();
      codeMeta = (codeEl.dataset.codeMeta || "").split(/\s+/).filter(Boolean);
      contract = parseExampleContract(codeEl.dataset.wqExample || "");
    }
    const replOptions = bookReplOptions(contract);
    if (lang === "wq" && replOptions) {
      mountBookRepl(pre, {
        source: codeEl?.textContent || "",
        contract,
        options: replOptions,
      }).catch((error) => console.error("[book repl] mount failed", error));
      return;
    }
    const expectedError =
      contract?.expect?.error ||
      (codeMeta.includes("error") ? "error" : undefined);
    const noRun =
      codeMeta.includes("no-run") ||
      contract?.role === "syntax" ||
      typeof contract?.file === "string";

    // Left: language label (only if provided)
    if (lang) {
      const langSpan = document.createElement("span");
      langSpan.className = "lang";
      if (contract) {
        langSpan.textContent = exampleHeaderLabel(lang.toLowerCase(), contract);
      } else {
        const labels = [lang.toLowerCase()];
        if (expectedError) labels.push("expected error");
        codeMeta
          .filter((item) => item !== "error" && item !== "no-run")
          .forEach((item) => labels.push(item));
        langSpan.textContent = labels.join(" | ");
      }
      header.appendChild(langSpan);
    } else {
      // add an empty spacer to keep layout consistent
      const spacer = document.createElement("span");
      spacer.className = "lang";
      spacer.textContent = "";
      header.appendChild(spacer);
    }

    // Right: actions (Run for wq + Copy)
    const actions = document.createElement("div");
    actions.className = "code-actions";

    const btn = document.createElement("button");
    btn.className = "code-action-btn";
    btn.dataset.action = "copy";
    btn.textContent = "Copy";
    actions.appendChild(btn);
    header.appendChild(actions);

    // move pre inside wrapper
    pre.parentNode.insertBefore(wrapper, pre);
    wrapper.appendChild(header);
    wrapper.appendChild(pre);

    if (lang === "wq" && !noRun) {
      runnableCells.push({
        id: `tutorial-cell-${cellIndex + 1}`,
        wrapper,
        actions,
        label: header.querySelector(".lang"),
        source: codeEl?.textContent || "",
        contract,
        expectedError,
        stdinRequester: null,
      });
    }

    bindCopyButton(btn, () => pre.innerText);
  });

  for (const runPlan of planCellRuns(
    runnableCells,
    (previous, next) =>
      previous.wrapper.nextElementSibling === next.wrapper,
  )) {
    const total = runPlan.cells.length;
    const groupView = packCellRun(runPlan);
    if (total === 1) packSingleCell(runPlan.cells[0]);

    const runButton = document.createElement("button");
    runButton.type = "button";
    runButton.className = "code-action-btn";
    runButton.dataset.action = "run";
    const idleLabel = cellRunLabel(total);
    runButton.textContent = idleLabel;
    (groupView?.actions || runPlan.cells[0].actions).appendChild(runButton);

    let activeCell = null;
    const evaluationController = createEvaluationController((state) => {
      const active = state !== "idle";
      runButton.textContent = active ? "Stop" : idleLabel;
      runButton.classList.toggle("code-action-danger", active);
      runButton.disabled = state === "stopping";
      runButton.dataset.evaluationState = state;
    });
    evaluationControllers.add(evaluationController);

    runButton.addEventListener("click", async () => {
      if (evaluationController.active) {
        evaluationController.stop("stop requested");
        return;
      }

      for (const cell of runPlan.cells) resetCellPanel(cell, "Waiting");

      try {
        await evaluationController.run(({ signal, setState }) =>
          queueEval(
            async () => {
              await ensureWasm();
              if (signal.aborted) throw abortError(signal.reason);
              setState("running");
              const session = new WasmWqSession();
              try {
                session.set_box_flags("0");
                for (const workspace of new Set(
                  runPlan.cells
                    .map((cell) => cell.contract?.workspace)
                    .filter(Boolean),
                )) {
                  registerWorkspaceModules(session, article, workspace);
                }

                for (let index = 0; index < runPlan.cells.length; index++) {
                  const cell = runPlan.cells[index];
                  activeCell = cell;
                  const view = resetCellPanel(cell, "Running...");
                  session.set_stdout_callback((chunk) => {
                    view.outputRenderer.appendStreamOutput(chunk);
                  });
                  session.set_stderr_callback((chunk) => {
                    view.outputRenderer.appendStreamOutput(chunk, "error");
                  });
                  session.set_stdin_callback(async (prompt) => {
                    setState("awaiting-input");
                    try {
                      return await view.stdinRequester.request(prompt, {
                        signal,
                      });
                    } finally {
                      if (!signal.aborted) setState("running");
                    }
                  });

                  try {
                    const result = await session.eval_wq_async(
                      cell.source,
                      total > 1
                        ? {
                            signal,
                            sourcePath: `<tutorial:${cell.id}>`,
                          }
                        : { signal },
                    );
                    if (hasFinalResult(result)) {
                      const needsNL =
                        view.codeOut.textContent &&
                        !view.codeOut.textContent.endsWith("\n");
                      if (needsNL) view.outputRenderer.appendText("\n");
                      if (total < 2) {
                        view.outputRenderer.appendText("\u{258D} ");
                      }
                      if (
                        !appendResultPresentation(
                          view.codeOut,
                          result.presentation,
                        )
                      ) {
                        view.outputRenderer.appendOutput(String(result.display));
                      }
                    }
                    const outcome = exampleOutcome(cell.contract, {
                      value: canonicalResultText(result),
                    });
                    view.panel.dataset.outcome = outcome.state;
                    setCellPanelHeading(view, outcome.heading);
                    if (outcome.state === "mismatch") {
                      for (const remaining of runPlan.cells.slice(index + 1)) {
                        resetCellPanel(
                          remaining,
                          "Not run because an earlier result differed",
                        );
                      }
                      return;
                    }
                  } catch (error) {
                    if (isAbortError(error)) throw error;
                    if (total < 2) view.outputRenderer.clear();
                    const outcome = exampleOutcome(cell.contract, {
                      errorKind: String(error?.kind || "error"),
                    });
                    view.panel.dataset.outcome = outcome.state;
                    setCellPanelHeading(view, outcome.heading);
                    if (!cell.expectedError) console.error(error);
                    const needsNL =
                      view.codeOut.textContent &&
                      !view.codeOut.textContent.endsWith("\n");
                    if (needsNL) view.outputRenderer.appendText("\n");
                    const bar = createOutputBar("error");
                    view.codeOut.appendChild(bar);
                    const errorRenderer = createOutputRenderer(
                      view.codeOut,
                      bar,
                    );
                    errorRenderer.appendOutput(
                      formatWqError(error, { rendered: true }) + "\n",
                      "error",
                    );
                    if (outcome.state !== "match") {
                      for (const remaining of runPlan.cells.slice(index + 1)) {
                        resetCellPanel(
                          remaining,
                          "Not run because an earlier cell failed",
                        );
                      }
                      return;
                    }
                  }
                }
              } finally {
                session.free();
              }
            },
            { signal },
          ),
        );
      } catch (error) {
        const failedCell = activeCell || runPlan.cells[0];
        if (isAbortError(error)) {
          const view = ensureCellPanel(failedCell);
          setCellPanelHeading(view, "Interrupted");
          if (total < 2) {
            view.outputRenderer.clear();
            view.outputRenderer.appendText("Interrupted\n");
          } else {
            view.outputRenderer.appendText(
              view.codeOut.textContent &&
                !view.codeOut.textContent.endsWith("\n")
                ? "\nInterrupted\n"
                : "Interrupted\n",
            );
          }
          const activeIndex = runPlan.cells.indexOf(failedCell);
          for (const remaining of runPlan.cells.slice(activeIndex + 1)) {
            resetCellPanel(remaining, "Not run");
          }
        } else {
          console.error(error);
          const view = resetCellPanel(failedCell, "Error");
          view.panel.dataset.outcome = "error";
          const bar = createOutputBar("error");
          view.codeOut.appendChild(bar);
          createOutputRenderer(view.codeOut, bar).appendOutput(
            formatWqError(error, { rendered: true }) + "\n",
            "error",
          );
          const failedIndex = runPlan.cells.indexOf(failedCell);
          for (const remaining of runPlan.cells.slice(failedIndex + 1)) {
            resetCellPanel(
              remaining,
              "Not run because an earlier cell failed",
            );
          }
        }
      } finally {
        activeCell = null;
      }
    });
  }
};
