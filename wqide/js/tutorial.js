// Build outline from headings, scrollspy, copy-to-clipboard buttons after article content is injected.

import {
  eval_wq,
  highlight_wq,
  set_stdout_callback,
  set_stdin_callback,
  set_stderr_callback,
} from "wq-wasm";
import { createAnsiRenderer } from "./ansi.js";
import { ensureWasm, queueEval, createOutputBar } from "./wq-shared.js";

let __outlineObserver = null;
let __outlineLockUntil = 0;
// Defer WASM init until the first Run click.

function animateButtonWidth(btn, newText) {
  // Clear any stale inline width from a previous animation
  btn.style.width = "";
  const currentWidth = btn.offsetWidth;

  // Lock to current width
  btn.style.width = currentWidth + "px";

  // Measure target width after text change
  btn.textContent = newText;
  btn.style.width = "auto";
  const targetWidth = btn.offsetWidth;

  // Reset to current width and animate to target
  btn.style.width = currentWidth + "px";
  btn.offsetWidth; // force reflow
  btn.style.width = targetWidth + "px";

  // Clean up inline width after transition, letting the button be auto-sized again
  setTimeout(() => {
    btn.style.width = "";
  }, 300);
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

  // Async highlight for wq code blocks
  (async () => {
    await ensureWasm();
    article.querySelectorAll("pre code.language-wq").forEach((codeEl) => {
      const raw = codeEl.textContent;
      codeEl.innerHTML = highlight_wq(raw);
    });
  })();

  // Enhance code blocks: wrap pre in .code-wrapper with header and copy button
  article.querySelectorAll("pre").forEach((pre) => {
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
    if (codeEl) {
      const m = Array.from(codeEl.classList).find((c) =>
        c.startsWith("language-"),
      );
      if (m) lang = m.replace("language-", "").trim();
      codeMeta = (codeEl.dataset.codeMeta || "").split(/\s+/).filter(Boolean);
    }
    const expectedError = codeMeta.includes("error");
    const noRun = codeMeta.includes("no-run");

    // Left: language label (only if provided)
    if (lang) {
      const langSpan = document.createElement("span");
      langSpan.className = "lang";
      const labels = [lang.toLowerCase()];
      if (expectedError) labels.push("expected error");
      if (noRun) labels.push("no run");
      codeMeta
        .filter((item) => item !== "error" && item !== "no-run")
        .forEach((item) => labels.push(item));
      langSpan.textContent = labels.join(" | ");
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

    if (lang === "wq" && !noRun) {
      const run = document.createElement("button");
      run.type = "button";
      run.className = "code-action-btn";
      run.dataset.action = "run";
      run.textContent = "Run";
      run.addEventListener("click", async () => {
        const code = pre.innerText || (codeEl ? codeEl.innerText : "");
        const stdinArr = [];
        // Render result panel after this code block
        let panel = wrapper.nextElementSibling;
        if (
          !panel ||
          !panel.classList ||
          !panel.classList.contains("run-result")
        ) {
          panel = document.createElement("div");
          panel.className = "run-result";
          const head = document.createElement("div");
          head.className = "run-head";
          head.textContent = "Result";
          const preOut = document.createElement("pre");
          const codeOut = document.createElement("code");
          preOut.appendChild(codeOut);
          panel.appendChild(head);
          panel.appendChild(preOut);
          wrapper.parentNode.insertBefore(panel, wrapper.nextSibling);
          // visually attach by marking wrapper as attached
          wrapper.classList.add("attached");
        }
        const panelHead = panel.querySelector(".run-head");
        if (panelHead) {
          panelHead.textContent = expectedError ? "error" : "result";
        }
        const codeOut = panel.querySelector("code");
        const outputRenderer =
          codeOut.__ansiRenderer ||
          (codeOut.__ansiRenderer = createAnsiRenderer(codeOut));

        run.disabled = true;
        // stream printed output; clear previous
        outputRenderer.clear();
        try {
          await ensureWasm();
          const result = await queueEval(() => {
            set_stdout_callback((chunk) => {
              outputRenderer.append(chunk);
            });
            set_stderr_callback((chunk) => {
              outputRenderer.append(chunk);
            });
            const queue = [...stdinArr];
            set_stdin_callback((_prompt) =>
              queue.length ? String(queue.shift()) : null,
            );
            return eval_wq(code);
          });
          if (
            result !== undefined &&
            result !== null &&
            String(result).length
          ) {
            const needsNL =
              codeOut.textContent && !codeOut.textContent.endsWith("\n");
            outputRenderer.append(
              (needsNL ? "\n" : "") + "\u{258D} " + String(result),
            );
          }
        } catch (err) {
          if (!expectedError) console.error(err);
          outputRenderer.clear();
          const bar = createOutputBar("error");
          codeOut.appendChild(bar);
          const errorRenderer = createAnsiRenderer(codeOut, bar);
          errorRenderer.append((err?.message ?? String(err)) + "\n");
        } finally {
          run.disabled = false;
        }
      });
      actions.appendChild(run);
    }

    // move pre inside wrapper
    pre.parentNode.insertBefore(wrapper, pre);
    wrapper.appendChild(header);
    wrapper.appendChild(pre);

    btn.addEventListener("click", async () => {
      const text = pre.innerText;
      try {
        if (navigator.clipboard && window.isSecureContext) {
          await navigator.clipboard.writeText(text);
        } else {
          const textArea = document.createElement("textarea");
          textArea.value = text;
          textArea.style.position = "fixed";
          textArea.style.opacity = "0";
          document.body.appendChild(textArea);
          textArea.focus();
          textArea.select();
          const success = document.execCommand("copy");
          textArea.remove();
          if (!success) throw new Error("Fallback copy failed");
        }
        animateButtonWidth(btn, "✓ Copied");
        setTimeout(() => {
          animateButtonWidth(btn, "Copy");
        }, 1400);
      } catch (e) {
        animateButtonWidth(btn, "Error");
        setTimeout(() => {
          animateButtonWidth(btn, "Copy");
        }, 1400);
      }
    });
  });
};
