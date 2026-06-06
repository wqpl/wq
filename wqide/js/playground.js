import {
  WasmWqSession,
  set_stdout_callback,
  set_stdin_callback,
  set_stderr_callback,
  highlight_wq,
} from "wq-wasm";
import { createAnsiRenderer } from "./ansi.js";
import { toCanvas } from "html-to-image";
import {
  ensureWasm,
  DEBUG_FLAGS,
  parseDebugFlags,
  formatDebugFlags,
  setActive,
  syncDebugButtons,
  alignTurnBody,
  escapeHtml,
  queueEval,
  handleTabKey,
} from "./wq-shared.js";
import { createCmOverlay } from "./cm-overlay.js";

function readDebugFlags(instance) {
  return parseDebugFlags(instance.debugFlagsInput?.value || "");
}

function writeDebugFlags(instance, flags) {
  const formatted = formatDebugFlags(flags);
  if (instance.debugFlagsInput) {
    instance.debugFlagsInput.value = formatted;
  }
  syncDebugButtons(instance.debugButtons, flags);
}

function toggleDebugFlag(instance, flag) {
  const current = readDebugFlags(instance);
  const next = current.includes(flag)
    ? current.filter((item) => item !== flag)
    : [...current, flag];
  writeDebugFlags(instance, next);
}

function ensureStateSavingSession(instance) {
  if (!instance.stateSavingSession) {
    instance.stateSavingSession = new WasmWqSession();
  }
  return instance.stateSavingSession;
}

function applyBoxMode(targetSession, instance) {
  const wantBoxMode = ensureStateSavingSession(instance).get_box_mode();
  if (targetSession.get_box_mode() !== wantBoxMode) {
    targetSession.toggle_box_mode();
  }
}

function syncBoxControl(instance) {
  setActive(instance.boxBtn, ensureStateSavingSession(instance).get_box_mode());
}

const instances = new WeakMap();
const DRAFT_KEY = "wqide:playground:draft";
let saveDraftTimeout = null;

const PLAYGROUND_TEMPLATES = {
  asciiplot: {
    code: "iota 80|map{50+35*sin[x/7]+12*sin[x/2]}|asciiplot",
    stdin: "",
  },
  primes: {
    code: `primes:{p:iota[x+1]>1;limit:floor sqrt x;i:2
  W[i<=limit;$.[p[i];j:i*i;p[j+i*iota[1+floor[(x-j)/i]]]:false];i:$[i=2;3;i+2]];where p}
primes[10000][-3..=-1]`,
    stdin: "",
  },
  stdin: {
    code: 'name:input[];echo@f"Hello, {name}"',
    stdin: "C",
  },
  cowsay: {
    code: `repeat:{[s;n]acc:();N[n;acc:acc,s];acc}
cowsay:{[msg]border:repeat["-";#msg+2]
  echo(" ",border)
  echo("< ",str msg," >")
  echo(" ",border)
  echo"        \\\\   ^__^"
  echo"         \\\\  (oo)\\\\_______"
  echo"            (__)\\\\       )\\\\/\\\\"
  echo"                 ||-----w-|"
  echo"                 ||      ||"
}
cowsay input[]`,
    stdin: "Moooving on!",
  },
};

function saveDraft(instance) {
  try {
    const draft = {
      code: instance.ta.value,
      stdin: instance.stdinInput?.value || "",
      boxMode: ensureStateSavingSession(instance).get_box_mode(),
      timeMode: instance.timeMode,
      debugFlags: instance.debugFlagsInput?.value || "0",
      updatedAt: Date.now(),
    };
    localStorage.setItem(DRAFT_KEY, JSON.stringify(draft));
  } catch (e) {
    console.debug("[playground] failed to save draft", e);
  }
}

function loadDraft() {
  try {
    const raw = localStorage.getItem(DRAFT_KEY);
    if (!raw) return null;
    return JSON.parse(raw);
  } catch (e) {
    console.debug("[playground] failed to load draft", e);
    return null;
  }
}

function clearDraft() {
  try {
    localStorage.removeItem(DRAFT_KEY);
  } catch (e) {
    console.debug("[playground] failed to clear draft", e);
  }
}

function debouncedSaveDraft(instance) {
  if (saveDraftTimeout) clearTimeout(saveDraftTimeout);
  saveDraftTimeout = setTimeout(() => saveDraft(instance), 500);
}

function showDraftNotice(instance, draft) {
  const notice = instance.draftNotice;
  if (!notice) return;
  notice.hidden = false;
  notice.innerHTML = `
    <span>Draft from ${new Date(draft.updatedAt).toLocaleTimeString()}</span>
    <button class="btn mini" type="button" id="restoreDraftBtn">Restore</button>
    <button class="btn mini" type="button" id="dismissDraftBtn">Dismiss</button>
  `;
  notice.querySelector("#restoreDraftBtn")?.addEventListener("click", () => {
    restoreDraft(instance, draft);
    notice.hidden = true;
  });
  notice.querySelector("#dismissDraftBtn")?.addEventListener("click", () => {
    notice.hidden = true;
  });
}

function restoreDraft(instance, draft) {
  if (!draft) return;
  if (draft.code !== undefined) {
    instance.ta.value = draft.code;
    instance.ta.dispatchEvent(new Event("input", { bubbles: true }));
    refreshLines(instance);
  }
  if (draft.stdin !== undefined && instance.stdinInput) {
    instance.stdinInput.value = draft.stdin;
  }
  if (draft.timeMode !== undefined) {
    instance.timeMode = draft.timeMode;
    setActive(instance.timeBtn, instance.timeMode);
  }
  if (draft.debugFlags !== undefined && instance.debugFlagsInput) {
    instance.debugFlagsInput.value = draft.debugFlags;
    syncDebugButtons(instance.debugButtons, parseDebugFlags(draft.debugFlags));
  }
  if (draft.boxMode !== undefined && instance.stateSavingSession) {
    const current = instance.stateSavingSession.get_box_mode();
    if (current !== draft.boxMode) {
      instance.stateSavingSession.toggle_box_mode();
    }
    setActive(instance.boxBtn, instance.stateSavingSession.get_box_mode());
  }
}

function refreshLines(instance) {
  const lines = instance.ta.value.split("\n").length || 1;
  const frag = document.createDocumentFragment();
  for (let i = 1; i <= lines; i++) {
    const div = document.createElement("div");
    div.className = "ln";
    div.textContent = i;
    frag.appendChild(div);
  }
  instance.gutter.innerHTML = "";
  instance.gutter.appendChild(frag);
}

async function doEval(instance) {
  instance.runBtn.disabled = true;
  instance.output.innerHTML = "";
  instance.outputPanel.hidden = true;

  // stdout/stderr — no bar
  const streamRenderer = createAnsiRenderer(instance.output);

  try {
    const code = instance.ta.value;
    const stdinArr = instance.stdinInput.value
      ? instance.stdinInput.value.replace(/\\n/g, "\n").split(/\r?\n/)
      : [];
    await ensureWasm();
    const flags = instance.debugFlagsInput?.value || "0";
    const start = performance.now();
    const result = await queueEval(() => {
      set_stdout_callback((chunk) => {
        streamRenderer.append(chunk);
        instance.output.scrollTop = instance.output.scrollHeight;
        instance.outputPanel.hidden = false;
      });
      set_stderr_callback((chunk) => {
        streamRenderer.append("\x1b[31m" + chunk + "\x1b[0m");
        instance.output.scrollTop = instance.output.scrollHeight;
        instance.outputPanel.hidden = false;
      });
      const queue = [...stdinArr];
      set_stdin_callback((_prompt) =>
        queue.length ? String(queue.shift()) : null,
      );
      const session = new WasmWqSession();
      try {
        applyBoxMode(session, instance);
        if (flags) {
          session.set_debug_flags(flags);
        }
        return session.eval_wq_result(code);
      } finally {
        session.free();
      }
    });
    const end = performance.now();
    if (
      result.value !== undefined &&
      result.value !== null &&
      String(result.value).length
    ) {
      // ensure a newline before the bar if stdout left content on the same line
      // if (instance.output.textContent && !instance.output.textContent.endsWith("\n")) {
      //   instance.output.appendChild(document.createTextNode("\n"));
      // }
      if (result.is_cas) {
        const bar = document.createElement("span");
        bar.className = "repl-bar repl-bar-success";
        bar.textContent = "\u258d ";
        instance.output.appendChild(bar);
        const casSpan = document.createElement("span");
        casSpan.innerHTML = highlight_wq(alignTurnBody(result.value));
        instance.output.appendChild(casSpan);
      } else {
        const bar = document.createElement("span");
        bar.className = "repl-bar repl-bar-success";
        bar.textContent = "\u258d ";
        instance.output.appendChild(bar);
        const resultRenderer = createAnsiRenderer(instance.output, bar);
        resultRenderer.append(alignTurnBody(String(result.value)) + "\n");
      }
      instance.output.scrollTop = instance.output.scrollHeight;
    }
    if (instance.timeMode) {
      const needsNL =
        instance.output.textContent &&
        !instance.output.textContent.endsWith("\n");
      streamRenderer.append(
        (needsNL ? "\n" : "") +
          alignTurnBody(`time elapsed: ${end - start}ms\n`),
      );
      instance.output.scrollTop = instance.output.scrollHeight;
    }
    instance.outputPanel.hidden = false;
  } catch (err) {
    console.error(err);
    const bar = document.createElement("span");
    bar.className = "repl-bar repl-bar-error";
    bar.textContent = "\u258d ";
    instance.output.appendChild(bar);
    const errorRenderer = createAnsiRenderer(instance.output, bar);
    errorRenderer.append(alignTurnBody((err?.message ?? String(err)) + "\n"));
    instance.outputPanel.hidden = false;
    instance.output.scrollTop = instance.output.scrollHeight;
  } finally {
    instance.runBtn.disabled = false;
  }
}

async function runForPoster(instance) {
  const stdoutDiv = document.createElement("div");
  const stdoutRenderer = createAnsiRenderer(stdoutDiv);
  const resultDiv = document.createElement("div");
  const errorDiv = document.createElement("div");

  try {
    const code = instance.ta.value;
    const stdinArr = instance.stdinInput.value
      ? instance.stdinInput.value.replace(/\\n/g, "\n").split(/\r?\n/)
      : [];
    await ensureWasm();
    set_stdout_callback((chunk) => stdoutRenderer.append(chunk));
    set_stderr_callback((chunk) =>
      stdoutRenderer.append("\x1b[31m" + chunk + "\x1b[0m"),
    );
    const queue = [...stdinArr];
    set_stdin_callback((_prompt) =>
      queue.length ? String(queue.shift()) : null,
    );
    const flags = instance.debugFlagsInput?.value || "0";
    const session = new WasmWqSession();
    try {
      applyBoxMode(session, instance);
      if (flags) session.set_debug_flags(flags);
      const result = session.eval_wq_result(code);
      if (
        result.value !== undefined &&
        result.value !== null &&
        String(result.value).length
      ) {
        if (result.is_cas) {
          const bar = document.createElement("span");
          bar.className = "repl-bar repl-bar-success";
          bar.textContent = "\u258d ";
          resultDiv.appendChild(bar);
          const casSpan = document.createElement("span");
          casSpan.innerHTML = highlight_wq(alignTurnBody(result.value));
          resultDiv.appendChild(casSpan);
        } else {
          const bar = document.createElement("span");
          bar.className = "repl-bar repl-bar-success";
          bar.textContent = "\u258d ";
          resultDiv.appendChild(bar);
          const resultRenderer = createAnsiRenderer(resultDiv, bar);
          resultRenderer.append(alignTurnBody(String(result.value)) + "\n");
        }
      }
    } finally {
      session.free();
    }
  } catch (err) {
    const bar = document.createElement("span");
    bar.className = "repl-bar repl-bar-error";
    bar.textContent = "\u258d ";
    errorDiv.appendChild(bar);
    const errorRenderer = createAnsiRenderer(errorDiv, bar);
    errorRenderer.append(alignTurnBody((err?.message ?? String(err)) + "\n"));
  }

  return {
    stdoutHTML: stdoutDiv.innerHTML,
    resultHTML: resultDiv.innerHTML,
    errorHTML: errorDiv.innerHTML,
  };
}

function createPosterConfigModal() {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "poster-modal-overlay";
    overlay.innerHTML = `
      <div class="poster-config-modal">
        <h3>Make Poster</h3>
        <div class="poster-field">
          <label for="posterTitle">Title</label>
          <input type="text" id="posterTitle" placeholder="Untitled" />
        </div>
        <div class="poster-field">
          <label for="posterDesc">Description</label>
          <textarea id="posterDesc" rows="3" placeholder="Optional description..."></textarea>
        </div>
        <div class="poster-field poster-field-inline">
          <input type="checkbox" id="posterRunCode" />
          <label for="posterRunCode">Run code and include output</label>
        </div>
        <div class="poster-modal-actions">
          <button class="btn" type="button" id="posterCancel">Cancel</button>
          <button class="btn primary" type="button" id="posterConfirm">Generate</button>
        </div>
      </div>
    `;

    const titleInput = overlay.querySelector("#posterTitle");
    const descInput = overlay.querySelector("#posterDesc");
    const runCheck = overlay.querySelector("#posterRunCode");
    const cancelBtn = overlay.querySelector("#posterCancel");
    const confirmBtn = overlay.querySelector("#posterConfirm");

    function close() {
      overlay.remove();
    }

    cancelBtn.addEventListener("click", () => {
      close();
      resolve(null);
    });

    confirmBtn.addEventListener("click", () => {
      const data = {
        title: titleInput.value.trim() || "Untitled",
        description: descInput.value.trim(),
        runCode: runCheck.checked,
      };
      close();
      resolve(data);
    });

    overlay.addEventListener("click", (e) => {
      if (e.target === overlay) {
        close();
        resolve(null);
      }
    });

    titleInput.addEventListener("keydown", (e) => {
      if (e.key === "Enter") confirmBtn.click();
    });

    document.body.appendChild(overlay);
    titleInput.focus();
  });
}

async function savePosterJPEG(cardEl, title) {
  try {
    // Use the device pixel ratio so the image stays sharp on Retina screens
    const dpr = Math.max(window.devicePixelRatio || 1, 2);

    // Capture the card as a high-res canvas (rounded corners are transparent)
    const cardCanvas = await toCanvas(cardEl, {
      pixelRatio: dpr,
      style: { boxShadow: "none" },
    });

    // Compose onto a larger canvas with the project's light-blue background and padding
    const padding = 80 * dpr;
    const canvas = document.createElement("canvas");
    const ctx = canvas.getContext("2d");
    canvas.width = cardCanvas.width + padding * 2;
    canvas.height = cardCanvas.height + padding * 2;

    // Fill background (matches CSS --bg: #d7e9f6)
    ctx.fillStyle = "#d7e9f6";
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    // Paint a white rounded backing slightly larger than the card to ensure
    // the corners stay clean even if html-to-image clips the right edge.
    const computedStyle = window.getComputedStyle(cardEl);
    const borderRadius =
      parseFloat(computedStyle.borderRadius) * dpr || 16 * dpr;
    ctx.fillStyle = "#ffffff";
    ctx.beginPath();
    ctx.roundRect(
      padding - 2,
      padding - 2,
      cardCanvas.width + 4,
      cardCanvas.height + 4,
      borderRadius + 2,
    );
    ctx.fill();

    // Draw card centered with padding
    ctx.drawImage(cardCanvas, padding, padding);

    // Export final JPEG (single compression step)
    const dataUrl = canvas.toDataURL("image/jpeg", 0.92);
    const link = document.createElement("a");
    link.download = `${title.replace(/\s+/g, "_") || "poster"}.jpg`;
    link.href = dataUrl;
    link.click();
  } catch (err) {
    console.error("[poster] failed to generate JPEG", err);
    alert("Failed to save poster as JPEG. See console for details.");
  }
}

function showPosterModal(posterHTML, title = "poster") {
  const overlay = document.createElement("div");
  overlay.className = "poster-modal-overlay poster-show-overlay";
  overlay.innerHTML = `
    <div class="poster-show-modal">
      <div class="poster-card">
        ${posterHTML}
      </div>
      <div class="poster-modal-actions" style="justify-content:center;margin-top:0;">
        <button class="btn" type="button" id="posterSaveJpeg">Save JPEG</button>
        <button class="btn primary" type="button" id="posterClose">Close</button>
      </div>
    </div>
  `;

  const card = overlay.querySelector(".poster-card");

  function close() {
    overlay.remove();
  }
  overlay.addEventListener("click", (e) => {
    if (e.target === overlay) close();
  });
  document.addEventListener("keydown", function onKey(e) {
    if (e.key === "Escape") {
      close();
      document.removeEventListener("keydown", onKey);
    }
  });

  overlay.querySelector("#posterClose")?.addEventListener("click", close);
  overlay.querySelector("#posterSaveJpeg")?.addEventListener("click", () => {
    savePosterJPEG(card, title);
  });

  document.body.appendChild(overlay);
}

async function makePoster(instance) {
  await ensureWasm();
  const config = await createPosterConfigModal();
  if (!config) return;

  let runOutput = null;
  if (config.runCode) {
    runOutput = await runForPoster(instance);
  }

  const code = instance.ta.value;
  const highlightedCode = highlight_wq(code);

  let runSection = "";
  if (config.runCode && runOutput) {
    let outputBlock = "";
    if (runOutput.errorHTML) {
      outputBlock = `<div class="poster-run-result poster-run-error">${runOutput.errorHTML}</div>`;
    } else {
      if (runOutput.stdoutHTML) {
        outputBlock += `<div class="poster-run-stdout">${runOutput.stdoutHTML}</div>`;
      }
      if (runOutput.resultHTML) {
        outputBlock += `<div class="poster-run-result">${runOutput.resultHTML}</div>`;
      }
    }
    if (outputBlock) {
      runSection = `
        <div class="poster-run-section">
          <div class="poster-run-head">Output</div>
          ${outputBlock}
        </div>
      `;
    }
  }

  const hasRunSection = runSection.length > 0;
  const posterContent = `
    <div class="poster-header">
      <h2 class="poster-title">${escapeHtml(config.title)}</h2>
      ${config.description ? `<p class="poster-desc">${escapeHtml(config.description)}</p>` : ""}
    </div>
    <div class="poster-body">
      <div class="poster-code-wrapper${hasRunSection ? " attached" : ""}">
        <div class="poster-code-header"><span class="lang">wq</span></div>
        <pre class="poster-code-pre"><code class="language-wq">${highlightedCode}</code></pre>
      </div>
      ${runSection}
    </div>
    <div class="poster-footer">
      <img src="./wq_transparent_bg.png" alt="wq logo" class="poster-logo" />
      <span class="poster-url">wq-pl.com</span>
    </div>
  `;

  showPosterModal(posterContent, config.title);
}

export async function mountPlayground(root) {
  const ta = root.querySelector("textarea.editor-text");
  const gutter = root.querySelector(".gutter");
  const output = root.querySelector(".run-output-body");
  const outputPanel = root.querySelector(".run-output-panel");
  const stdinInput = root.querySelector("#stdin");
  const clearOutBtn = root.querySelector("#clearOutBtn");
  const makePosterBtn = root.querySelector("#makePosterBtn");
  const runBtn = root.querySelector("#runBtn");
  const editor = root.querySelector(".editor");
  const debugFlagsInput = root.querySelector("#playgroundDebugFlags");
  const boxBtn = root.querySelector("#playgroundBoxBtn");
  const timeBtn = root.querySelector("#playgroundTimeBtn");
  const templateButtons = Array.from(root.querySelectorAll("[data-template]"));
  const resetBtn = root.querySelector("#resetBtn");
  const draftNotice = root.querySelector("#draftNotice");
  const openInReplBtn = root.querySelector("#openInReplBtn");
  const instance = {
    ta,
    gutter,
    output,
    outputPanel,
    stdinInput,
    clearOutBtn,
    runBtn,
    editor,
    debugFlagsInput,
    boxBtn,
    timeBtn,
    timeMode: false,
    stateSavingSession: null,
    debugButtons: Object.fromEntries(
      DEBUG_FLAGS.map((flag) => [
        flag,
        root.querySelector(`[data-debug-flag="${flag}"]`),
      ]),
    ),
    templateButtons,
    resetBtn,
    draftNotice,
    openInReplBtn,
  };
  instances.set(root, instance);

  await ensureWasm();

  const cmRoot = root.querySelector(".cm-editor");
  let overlay = null;
  if (cmRoot) {
    overlay = createCmOverlay(cmRoot, {
      highlight: highlight_wq,
      onInput: () => {
        refreshLines(instance);
        debouncedSaveDraft(instance);
      },
      onExec: () => doEval(instance),
    });
  }
  instance.overlay = overlay;

  ta.addEventListener("input", () => {
    refreshLines(instance);
    debouncedSaveDraft(instance);
  });
  stdinInput?.addEventListener("input", () => debouncedSaveDraft(instance));
  runBtn?.addEventListener("click", async (e) => {
    e.preventDefault();
    await doEval(instance);
  });
  ta.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey || e.shiftKey) && e.key === "Enter") {
      e.preventDefault();
      doEval(instance);
    } else if (e.key === "Tab" && !cmRoot) {
      handleTabKey(e, ta, () => {
        refreshLines(instance);
        debouncedSaveDraft(instance);
      });
    }
  });
  clearOutBtn?.addEventListener("click", () => {
    instance.output.innerHTML = "";
    instance.outputPanel.hidden = true;
  });
  makePosterBtn?.addEventListener("click", async () => {
    await makePoster(instance);
  });
  boxBtn?.addEventListener("click", async () => {
    await ensureWasm();
    const on = ensureStateSavingSession(instance).toggle_box_mode();
    setActive(boxBtn, on);
    debouncedSaveDraft(instance);
    console.log(`[playground] box mode -> ${on ? "on" : "off"}\n`);
  });
  timeBtn?.addEventListener("click", () => {
    instance.timeMode = !instance.timeMode;
    setActive(timeBtn, instance.timeMode);
    debouncedSaveDraft(instance);
    console.log(
      `[playground] time mode -> ${instance.timeMode ? "on" : "off"}\n`,
    );
  });
  DEBUG_FLAGS.forEach((flag) => {
    instance.debugButtons[flag]?.addEventListener("click", () => {
      toggleDebugFlag(instance, flag);
      debouncedSaveDraft(instance);
    });
  });
  templateButtons.forEach((button) => {
    button.addEventListener("click", () => {
      const template = PLAYGROUND_TEMPLATES[button.dataset.template];
      if (!template) return;
      ta.value = template.code;
      stdinInput.value = template.stdin;
      refreshLines(instance);
      ta.focus();
      ta.setSelectionRange(ta.value.length, ta.value.length);
      debouncedSaveDraft(instance);
    });
  });
  resetBtn?.addEventListener("click", () => {
    if (instance.overlay) {
      instance.overlay.value = "";
    } else {
      ta.value = "";
    }
    stdinInput.value = "";
    instance.output.innerHTML = "";
    instance.outputPanel.hidden = true;
    refreshLines(instance);
    instance.timeMode = false;
    setActive(timeBtn, false);
    writeDebugFlags(instance, []);
    if (instance.stateSavingSession) {
      if (!instance.stateSavingSession.get_box_mode()) {
        instance.stateSavingSession.toggle_box_mode();
      }
      setActive(boxBtn, true);
    }
    clearDraft();
    if (draftNotice) draftNotice.hidden = true;
    if (instance.overlay) {
      instance.overlay.focus();
    } else {
      ta.focus();
    }
  });
  openInReplBtn?.addEventListener("click", () => {
    const code = ta.value.trim();
    if (!code) return;
    window.navigate(`repl.html?input=${encodeURIComponent(code)}`);
  });

  refreshLines(instance);
  await ensureWasm();
  syncBoxControl(instance);
  setActive(timeBtn, instance.timeMode);
  writeDebugFlags(instance, []);

  // Restore draft if no URL code is present (defer to applyPlaygroundRoute)
  const draft = loadDraft();
  if (draft) {
    instance._pendingDraft = draft;
  }
}

export async function activatePlayground(root) {
  const instance = instances.get(root);
  if (!instance) return;
  await ensureWasm();
  syncBoxControl(instance);
  setActive(instance.timeBtn, instance.timeMode);
}

export function applyPlaygroundRoute(root, params) {
  const instance = instances.get(root);
  if (!instance) return;
  const code = params.get("code");
  const sin = params.get("stdin");
  const draft = instance._pendingDraft;
  delete instance._pendingDraft;

  if (code) {
    instance.ta.value = decodeURIComponent(code);
    instance.ta.dispatchEvent(new Event("input", { bubbles: true }));
    refreshLines(instance);
    if (sin) {
      instance.stdinInput.value = decodeURIComponent(sin);
    }
    // If draft exists and differs from URL, offer to restore
    if (draft && draft.code !== instance.ta.value) {
      showDraftNotice(instance, draft);
    } else if (draft) {
      // Same as draft, just save it
      saveDraft(instance);
    }
    return;
  }

  // No URL code: restore draft if available
  if (draft) {
    restoreDraft(instance, draft);
  }
  if (sin && instance.stdinInput) {
    instance.stdinInput.value = decodeURIComponent(sin);
  }
  if (instance.draftNotice) {
    instance.draftNotice.hidden = true;
  }
}
