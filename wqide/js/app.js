import { parseMarkdown } from "./markdown.js";
import { getDocMarkdown } from "./wq-shared.js";

console.debug("[wqide] app shell loaded");

function html(strings, ...values) {
  return strings.reduce((acc, str, i) => acc + str + (values[i] || ""), "");
}

const ROUTE_ORDER = ["featured", "playground", "repl", "more"];
const SHELL_HTML = html`
  <header class="topbar">
    <div class="topbar-row">
      <div class="brand">wqide</div>
      <div class="pillbar" aria-label="Quick toggles">
        <div class="pills" role="list"></div>
      </div>
    </div>
    <nav class="tabs" role="tablist" aria-label="Sections">
      <a href="index.html" data-nav="featured">Featured</a>
      <a href="playground.html" data-nav="playground">Playground</a>
      <a href="repl.html" data-nav="repl">REPL</a>
      <a href="more.html" data-nav="more">More</a>
    </nav>
  </header>
  <main class="wrap" id="appMain"></main>
`;

const FEATURED_HTML = html`
  <main class="wrap featured-wrap">
    <section class="welcome-card">
      <div class="welcome-copy">
        <h2>Start here with wqide</h2>
        <p>wq(wqpl) is a programming language developed by tttiw.</p>
        <p>
          wqide is a compact, warm space for learning wq and trying random ideas
          in your browser.
        </p>
      </div>
      <div class="welcome-links" aria-label="Useful articles">
        <a class="article-link" href="article.html?slug=installation">
          <strong>Installation</strong>
          <span>How to get a copy of wq.</span>
        </a>
        <a class="article-link" href="article.html?slug=prelude">
          <strong>Prelude</strong>
          <span>A first feel of wq.</span>
        </a>
      </div>
    </section>

    <section class="card">
      <h2>The wq Programming Language</h2>
      <p>A short journey through the fundamentals of wq.</p>
      <span class="code">f:{(f_:{$[x=0;y;f_[x-1;z;y+z]]})[x;0;1]}</span>
      <a
        class="stretched"
        href="subfolder.html?section=wqpl"
        aria-label="Open wqpl folder"
        >Open</a
      >
    </section>

    <div class="divider"></div>

    <section class="card">
      <h2>Misc</h2>
      <p>Installation, CLI usage, etc.</p>
      <span class="code">!wqdb</span>
      <a
        class="stretched"
        href="subfolder.html?section=Misc"
        aria-label="Open Misc folder"
        >Open</a
      >
    </section>

    <div class="divider"></div>

    <section class="card">
      <h2>WIP</h2>
      <p>Tests and WIP articles.</p>
      <span class="code">//todo</span>
      <a
        class="stretched"
        href="subfolder.html?section=WIP"
        aria-label="Open WIP folder"
        >Open</a
      >
    </section>
  </main>
`;

const PLAYGROUND_HTML = html`
  <main class="wrap">
    <div class="playground-shell">
      <aside class="playground-sidebar" aria-labelledby="templateHeading">
        <h2 id="templateHeading">Examples</h2>
        <div class="playground-template-list" role="list">
          <button
            class="playground-template-card playground-template-card-1"
            type="button"
            data-template="asciiplot">
            <strong>Asciiplot</strong>
            <span class="playground-template-desc"
              >Plot ASCII art from numeric data.</span
            >
            <code class="playground-template-code">... |asciiplot</code>
          </button>
          <button
            class="playground-template-card playground-template-card-2"
            type="button"
            data-template="stdin">
            <strong>Input</strong>
            <span class="playground-template-desc"
              >Preload stdin and test interactive input handling.</span
            >
            <code class="playground-template-code">input[]</code>
          </button>
          <button
            class="playground-template-card playground-template-card-3"
            type="button"
            data-template="primes">
            <strong>Primes</strong>
            <span class="playground-template-desc"
              >Generate prime numbers up to a given limit.</span
            >
            <code class="playground-template-code">primes[100][-3..=-1]</code>
          </button>
          <button
            class="playground-template-card playground-template-card-4"
            type="button"
            data-template="cowsay">
            <strong>Cowsay</strong>
            <span class="playground-template-desc"
              >An ASCII cow that moos.</span
            >
            <code class="playground-template-code">cowsay:{...}</code>
          </button>
        </div>
      </aside>

      <div class="editor" role="region" aria-label="Playground code editor">
        <div class="toolbar">
          <div class="toolbar-main">
            <div class="toolbar-left">
              <button id="runBtn" class="btn primary" type="button">Exec</button>
              <span class="mini">Shift-Enter: exec</span>
            </div>
            <div class="toolbar-center">
              <div
                class="pills"
                role="list"
                aria-label="Playground runtime controls">
                <div class="runtime-control" data-runtime-menu>
                  <button
                    id="playgroundBoxBtn"
                    class="pill inactive"
                    type="button"
                    aria-expanded="false"
                    aria-controls="playgroundBoxPanel">
                    box
	                  </button>
	                  <div id="playgroundBoxPanel" class="runtime-panel">
	                    <div class="runtime-panel-head">
	                      <span class="mini">box</span>
	                    </div>
                    <div class="pills" role="list">
                      <button
                        class="pill inactive"
                        type="button"
                        data-box-flag="box">
                        box
                      </button>
                      <button
                        class="pill inactive"
                        type="button"
                        data-box-flag="axis">
                        axis
                      </button>
                      <button
                        class="pill inactive"
                        type="button"
                        data-box-flag="color">
                        color
                      </button>
                      <button
                        class="pill inactive"
                        type="button"
                        data-box-flag="xray">
                        xray
                      </button>
                    </div>
                  </div>
                </div>
	                <button id="playgroundTimeBtn" class="pill inactive" type="button">
	                  time
	                </button>
	                <div class="runtime-control debug-controls" data-runtime-menu>
	                  <button
	                    id="playgroundDebugToggle"
	                    class="pill inactive"
	                    type="button"
	                    aria-expanded="false"
	                    aria-controls="playgroundDebugPanel">
	                    debug
	                  </button>
	                  <div id="playgroundDebugPanel" class="runtime-panel debug-panel">
	                    <div class="runtime-panel-head">
	                      <span class="mini">debug</span>
	                    </div>
	                    <div class="pills" role="list">
	                      <button
	                        class="pill inactive"
	                        type="button"
	                        data-debug-flag="token">
	                        token
	                      </button>
	                      <button
	                        class="pill inactive"
	                        type="button"
	                        data-debug-flag="cst">
	                        cst
	                      </button>
	                      <button
	                        class="pill inactive"
	                        type="button"
	                        data-debug-flag="ast">
	                        ast
	                      </button>
	                      <button
	                        class="pill inactive"
	                        type="button"
	                        data-debug-flag="ast-v">
	                        ast-v
	                      </button>
	                      <button
	                        class="pill inactive"
	                        type="button"
	                        data-debug-flag="inst">
	                        inst
	                      </button>
	                      <button
	                        class="pill inactive"
	                        type="button"
	                        data-debug-flag="inst-v">
	                        inst-v
	                      </button>
	                      <button
	                        class="pill inactive"
	                        type="button"
	                        data-debug-flag="wqdb">
	                        wqdb
	                      </button>
	                      <button
	                        class="pill inactive"
	                        type="button"
	                        data-debug-flag="wqdb-v">
	                        wqdb-v
	                      </button>
	                      <button
	                        class="pill inactive"
	                        type="button"
	                        data-debug-flag="value">
	                        value
	                      </button>
	                      <button
	                        class="pill inactive"
	                        type="button"
	                        data-debug-flag="cas">
	                        cas
	                      </button>
	                      <button
	                        class="pill inactive"
	                        type="button"
	                        data-debug-flag="cas-v">
	                        cas-v
	                      </button>
	                    </div>
	                  </div>
	                </div>
	                <input id="playgroundDebugFlags" type="hidden" value="" />
	              </div>
            </div>
            <div class="toolbar-right">
              <button id="makePosterBtn" class="btn" type="button">
                Make Poster
              </button>
              <button id="resetBtn" class="btn" type="button">Reset</button>
              <button id="openInReplBtn" class="btn" type="button">
                Open in REPL
              </button>
            </div>
          </div>
          <div class="stdin-row">
            <span class="mini">stdin:</span>
            <textarea
              id="stdin"
              rows="2"
              placeholder="Provide stdin for your program..."></textarea>
          </div>
        </div>
        <div class="editor-area">
          <div class="gutter" aria-hidden="true"></div>
          <div class="codepane">
            <textarea
                class="editor-text"
                aria-label="Playground code"
                spellcheck="false"></textarea>
          </div>
        </div>
	        <div class="run-output-panel" aria-live="polite" hidden>
	          <div class="run-output-header">
	            <span class="run-output-title">Output</span>
	            <button
	              id="clearOutBtn"
	              class="pill inactive run-output-clear"
	              type="button">
	              Clear
	            </button>
	          </div>
          <pre class="run-output-body"></pre>
        </div>
      </div>

      <aside class="symbol-panel" aria-labelledby="symbolPanelHeading">
        <div class="symbol-panel-head">
          <h2 id="symbolPanelHeading">Symbols</h2>
          <span class="symbol-panel-count" data-symbol-count>0</span>
        </div>
        <div class="symbol-panel-status" data-symbol-status>No symbols yet.</div>
        <div class="symbol-panel-list" data-symbol-list></div>
      </aside>
    </div>
  </main>
`;

const REPL_HTML = html`
  <main class="wrap">
    <div class="repl repl-flow">
      <div class="toolbar headbar repl-topbar">
        <div class="repl-actions">
          <div class="repl-copy-actions">
            <button id="copyFlowBtn" class="btn repl-copy-btn" type="button">
              Copy Flow
            </button>
            <button id="copyOutputBtn" class="btn repl-copy-btn" type="button">
              Copy Output
            </button>
          </div>
          <div class="repl-runtime-actions" aria-label="REPL runtime controls">
            <div class="pills" role="list">
              <div class="runtime-control" data-runtime-menu>
                <button
                  id="pillBox"
                  class="pill inactive"
                  type="button"
                  aria-expanded="false"
                  aria-controls="boxPanel">
                  box
                </button>
                <div id="boxPanel" class="runtime-panel">
                  <div class="runtime-panel-head">
                    <span class="mini">box</span>
                  </div>
                  <div class="pills" role="list">
                    <button class="pill inactive" type="button" data-box-flag="box">
                      box
                    </button>
                    <button class="pill inactive" type="button" data-box-flag="axis">
                      axis
                    </button>
                    <button
                      class="pill inactive"
                      type="button"
                      data-box-flag="color">
                      color
                    </button>
                    <button class="pill inactive" type="button" data-box-flag="xray">
                      xray
                    </button>
                  </div>
                </div>
              </div>
              <button id="pillTime" class="pill inactive" type="button">
                time
              </button>
              <div class="runtime-control debug-controls" data-runtime-menu>
                <button
                  id="debugToggle"
                  class="pill inactive"
                  type="button"
                  aria-expanded="false"
                  aria-controls="debugPanel">
                  debug
                </button>
                <div id="debugPanel" class="runtime-panel debug-panel">
                  <div class="runtime-panel-head">
                    <span class="mini">debug</span>
                  </div>
                  <div class="pills" role="list">
                    <button
                      class="pill inactive"
                      type="button"
                      data-debug-flag="token">
                      token
                    </button>
                    <button
                      class="pill inactive"
                      type="button"
                      data-debug-flag="cst">
                      cst
                    </button>
                    <button
                      class="pill inactive"
                      type="button"
                      data-debug-flag="ast">
                      ast
                    </button>
                    <button
                      class="pill inactive"
                      type="button"
                      data-debug-flag="ast-v">
                      ast-v
                    </button>
                    <button
                      class="pill inactive"
                      type="button"
                      data-debug-flag="inst">
                      inst
                    </button>
                    <button
                      class="pill inactive"
                      type="button"
                      data-debug-flag="inst-v">
                      inst-v
                    </button>
                    <button
                      class="pill inactive"
                      type="button"
                      data-debug-flag="wqdb">
                      wqdb
                    </button>
                    <button
                      class="pill inactive"
                      type="button"
                      data-debug-flag="wqdb-v">
                      wqdb-v
                    </button>
                    <button
                      class="pill inactive"
                      type="button"
                      data-debug-flag="value">
                      value
                    </button>
                    <button
                      class="pill inactive"
                      type="button"
                      data-debug-flag="cas">
                      cas
                    </button>
                    <button
                      class="pill inactive"
                      type="button"
                      data-debug-flag="cas-v">
                      cas-v
                    </button>
                  </div>
                </div>
              </div>
            </div>
          </div>
          <div class="repl-session-actions">
            <button id="resetBtn" class="btn" type="button">Reset Session</button>
            <button id="clearBtn" class="btn" type="button">Clear Flow</button>
            <button id="openInPlaygroundBtn" class="btn" type="button">
              Open in Playground
            </button>
          </div>
        </div>
      </div>

      <div id="term" class="repl-thread" aria-live="polite"></div>

      <div class="repl-composer-area">
        <div
          id="historySearch"
          class="history-search"
          role="dialog"
          aria-label="REPL history"
          hidden>
          <input
            type="text"
            id="historySearchInput"
            placeholder="Search history..."
            autocomplete="off" />
          <button id="clearHistoryBtn" class="history-clear" type="button">
            Clear History
          </button>
          <div id="historySearchResults" class="history-search-results"></div>
        </div>
        <form id="composerForm" class="repl-composer">
          <div class="composer-frame">
            <textarea
                id="code"
                class="editor-text repl-input"
                aria-label="REPL code"
                spellcheck="false"
                placeholder="echo echo"
                enterkeyhint="send"
                rows="1"></textarea>
          </div>
          <div class="composer-actions">
            <div class="stdin composer-stdin">
              <span class="mini">stdin:</span>
              <input
                id="stdinLine"
                type="text"
                placeholder="Queue stdin for the next run..." />
              <button id="pushStdinBtn" class="btn" type="button">Queue</button>
            </div>
            <button
              id="newlineBtn"
              class="btn mini"
              type="button"
              title="Insert newline">
              Newline
            </button>
            <button
              id="historyToggleBtn"
              class="btn mini history-toggle"
              type="button"
              aria-expanded="false"
              aria-controls="historySearch">
              History
            </button>
            <span class="mini composer-hint"
              >Enter: exec | Shift-Enter: newline</span
            >
            <button
              id="evalBtn"
              class="btn primary composer-send"
              type="submit">
              Exec
            </button>
          </div>
        </form>
      </div>
    </div>
  </main>
`;

const MORE_HTML = html`
  <main class="wrap">
    <article class="article more-page">
      <div class="more-head">
        <h1>About wqide</h1>
      </div>
      <div class="more-grid">
        <section class="more-card span-2">
          <h2>Project</h2>
          <ul>
            <li>
              <a
                href="https://codeberg.org/wqpl/wq/src/branch/main/wqide"
                target="_blank"
                rel="noopener"
                >wqide</a
              >: the interactive wq development environment
            </li>
            <li>
              <a
                href="https://codeberg.org/wqpl/wq"
                target="_blank"
                rel="noopener"
                >wq</a
              >: a programming language
            </li>
          </ul>
        </section>

        <section class="more-card">
          <h2>Version</h2>
          <table>
            <thead>
              <tr>
                <th>Project</th>
                <th>Version</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>wqide</td>
                <td>0.2.0</td>
              </tr>
              <tr>
                <td>wq</td>
                <td>0.8.0</td>
              </tr>
            </tbody>
          </table>
        </section>

        <section class="more-card span-2">
          <h2>Licenses</h2>
          <table>
            <thead>
              <tr>
                <th>Project</th>
                <th>License</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>wqide</td>
                <td>MIT license. Copyright (c) 2026 tttiw</td>
              </tr>
              <tr>
                <td>wq</td>
                <td>MIT license. Copyright (c) 2026 tttiw</td>
              </tr>
            </tbody>
          </table>
        </section>

        <section class="more-card">
          <h2>Contact</h2>
          <p><a href="mailto:tttiw@nekoarch.cc">tttiw@nekoarch.cc</a></p>
        </section>
      </div>
    </article>
  </main>
`;

const SUBFOLDER_HTML = html`
  <main class="wrap">
    <nav class="breadcrumbs" aria-label="Breadcrumb">
      <button class="crumb-back" type="button" aria-label="Go back">
        &#8592;
      </button>
      <a href="index.html">~</a> <span class="sep">/</span>
      <span data-role="section-crumb">Basics</span>
    </nav>

    <div class="folder-head"><h1 data-role="section-title"></h1></div>

    <div class="grid" data-role="section-grid"></div>

    <div class="divider"></div>
  </main>
`;

const ARTICLE_HTML = html`
  <main class="wrap">
    <nav class="breadcrumbs" aria-label="Breadcrumb">
      <button class="crumb-back" type="button" aria-label="Go back">
        &#8592;
      </button>
      <a href="index.html">~</a> <span class="sep">/</span>
      <a data-role="article-section-link" href="subfolder.html">Section</a>
      <span class="sep">/</span>
      <span data-role="article-title-crumb">Loading...</span>
    </nav>

    <div class="layout-3col">
      <aside class="left-rail">
        <div class="sticky">
          <div class="outline">
            <h3>Outline</h3>
            <div data-role="outline-list" aria-label="Section navigation"></div>
          </div>
        </div>
      </aside>

      <article class="article" data-role="article-root">
        <details class="mobile-outline">
          <summary>Outline</summary>
          <div data-role="mobile-outline"></div>
        </details>
        <h1 data-role="article-title">Loading...</h1>
        <div data-role="article-content">
          Please wait while the tutorial loads.
        </div>
      </article>
    </div>
  </main>
`;

document.body.innerHTML = SHELL_HTML;

const main = document.getElementById("appMain");
const state = {
  manifestPromise: null,
  tutorialModulePromise: null,
  views: new Map(),
  activeRoute: null,
};

function syncHeaderHeight() {
  const header = document.querySelector(".topbar");
  if (!header) return;
  const h = header.getBoundingClientRect().height;
  document.documentElement.style.setProperty("--header-h", `${h}px`);
}

function syncTabIndicator() {
  const tabs = document.querySelector(".tabs");
  if (!tabs) return;
  const active = tabs.querySelector('a[aria-current="page"]');
  if (!active) {
    tabs.style.setProperty("--tabs-indicator-opacity", "0");
    return;
  }
  const tabsRect = tabs.getBoundingClientRect();
  const activeRect = active.getBoundingClientRect();
  const width = Math.min(74, Math.max(44, activeRect.width - 28));
  const left = activeRect.left - tabsRect.left + (activeRect.width - width) / 2;
  tabs.style.setProperty("--tabs-indicator-left", `${left}px`);
  tabs.style.setProperty("--tabs-indicator-width", `${width}px`);
  tabs.style.setProperty("--tabs-indicator-opacity", "1");
}

window.addEventListener("load", syncHeaderHeight);
window.addEventListener("load", syncTabIndicator);
window.addEventListener("resize", () => {
  syncHeaderHeight();
  syncTabIndicator();
});

function getPathFile() {
  return location.pathname.split("/").pop() || "index.html";
}

function parseRoute() {
  const outerParams = new URLSearchParams(location.search);
  const routedFile = outerParams.get("route");
  const slug = outerParams.get("slug");
  const section = outerParams.get("section");
  const file = routedFile || getPathFile();
  const params = new URLSearchParams(location.search);
  if (routedFile) {
    params.delete("route");
  }
  if (!routedFile && slug) {
    return {
      key: `article:${slug}`,
      area: "featured",
      params,
    };
  }
  if (!routedFile && section) {
    return {
      key: `subfolder:${section}`,
      area: "featured",
      params,
    };
  }
  if (file === "playground.html") {
    return { key: "playground", area: "playground", params };
  }
  if (file === "repl.html") {
    return { key: "repl", area: "repl", params };
  }
  if (file === "more.html") {
    return { key: "more", area: "more", params };
  }
  if (file === "subfolder.html") {
    return {
      key: `subfolder:${params.get("section") || "Basics"}`,
      area: "featured",
      params,
    };
  }
  if (file === "article.html") {
    return {
      key: `article:${params.get("slug") || ""}`,
      area: "featured",
      params,
    };
  }
  return { key: "featured", area: "featured", params };
}

function persistNav(area) {
  const file = getPathFile();
  const params = new URLSearchParams(location.search);
  const routedFile = params.get("route");
  if (routedFile) params.delete("route");
  if (area === "repl") params.delete("input");
  const query = params.toString();
  const logicalFile = area === "featured" ? "index.html" : routedFile || file;
  const withQuery =
    logicalFile + (query ? `?${query}` : "") + (location.hash || "");
  try {
    localStorage.setItem("nav:last:" + area, withQuery);
  } catch (e) {
    console.debug("nav persist failed", e);
  }
}

function replaceRouteParams(names) {
  const params = new URLSearchParams(location.search);
  for (const name of names) {
    params.delete(name);
  }
  const query = params.toString();
  const current = location.pathname + location.search + location.hash;
  const target =
    location.pathname + (query ? `?${query}` : "") + (location.hash || "");
  if (target !== current) {
    history.replaceState({}, "", target);
  }
}

function getLastNavMap() {
  const getClean = (key, def) => {
    const val = localStorage.getItem(key);
    if (!val) return def;
    return val;
  };
  return {
    featured: getClean("nav:last:featured", "index.html"),
    playground: getClean("nav:last:playground", "playground.html"),
    repl: getClean("nav:last:repl", "repl.html"),
    more: getClean("nav:last:more", "more.html"),
  };
}

function getAreaBaseHref(nav) {
  return nav === "featured"
    ? "index.html"
    : nav === "playground"
      ? "playground.html"
      : nav === "repl"
        ? "repl.html"
        : "more.html";
}

function getTabTargetHref(nav) {
  const route = state.activeRoute || parseRoute();
  const last = getLastNavMap();
  return last[nav] || getAreaBaseHref(nav);
}

function updateNav(area) {
  document.querySelectorAll(".tabs a").forEach((a) => {
    const nav = a.dataset.nav;
    if (nav === area) {
      a.setAttribute("aria-current", "page");
    } else {
      a.removeAttribute("aria-current");
    }
    a.setAttribute("href", getTabTargetHref(nav));
  });
  syncTabIndicator();
}

function navigate(url, options = {}) {
  const next = new URL(url, location.href);
  if (next.origin !== location.origin) {
    location.href = next.href;
    return;
  }
  const current = location.pathname + location.search + location.hash;
  const target = next.pathname + next.search + next.hash;
  if (current === target && !options.force) return;
  if (options.replace) {
    history.replaceState({}, "", target);
  } else {
    history.pushState({}, "", target);
  }
  renderRoute();
}
window.navigate = navigate;

document.addEventListener("click", (event) => {
  const link = event.target.closest("a[href]");
  if (!link) return;
  if (link.target === "_blank" || link.hasAttribute("download")) return;
  if (link.closest(".tabs")) return;
  const href = link.getAttribute("href");
  if (
    !href ||
    href.startsWith("#") ||
    href.startsWith("mailto:") ||
    href.startsWith("http")
  ) {
    return;
  }
  event.preventDefault();
  navigate(href);
});

window.addEventListener("popstate", () => {
  renderRoute();
});

async function getManifest() {
  if (!state.manifestPromise) {
    state.manifestPromise = fetch("manifest.json").then((res) => {
      if (!res.ok) throw new Error("Failed to load manifest: " + res.status);
      return res.json();
    });
  }
  return state.manifestPromise;
}

async function getTutorialModule() {
  if (!state.tutorialModulePromise) {
    state.tutorialModulePromise = import("./tutorial.js");
  }
  return state.tutorialModulePromise;
}

function createView(key, html) {
  const root = document.createElement("div");
  root.dataset.view = key;
  root.hidden = true;
  root.innerHTML = html;
  main.appendChild(root);
  return root;
}

function getView(key, html) {
  if (!state.views.has(key)) {
    state.views.set(key, createView(key, html));
  }
  return state.views.get(key);
}

function showView(root) {
  const activeView = Array.from(state.views.values()).find(
    (view) => !view.hidden,
  );
  if (activeView) {
    activeView.dataset.scrollY = String(
      window.scrollY || window.pageYOffset || 0,
    );
  }
  state.views.forEach((view) => {
    view.hidden = view !== root;
  });
  const savedScrollY = Number(root.dataset.scrollY || 0);
  window.scrollTo({ top: savedScrollY, behavior: "instant" });
  syncHeaderHeight();
  syncTabIndicator();
}

function wireBackButton(root) {
  const btn = root.querySelector(".crumb-back");
  if (!btn || btn.dataset.wired === "true") return;
  btn.dataset.wired = "true";
  btn.addEventListener("click", () => {
    const kind = root.dataset.view || "";
    if (kind.startsWith("article:")) {
      const sectionLink = root.querySelector(
        '[data-role="article-section-link"]',
      );
      navigate(sectionLink?.getAttribute("href") || "index.html");
      return;
    }
    if (kind.startsWith("subfolder:")) {
      navigate("index.html");
      return;
    }
    navigate("index.html");
  });
}

async function mountFeatured() {
  const root = getView("featured", FEATURED_HTML);
  document.title = "wqide - Featured";
  showView(root);
}

async function mountMore() {
  const root = getView("more", MORE_HTML);
  document.title = "wqide - More";
  showView(root);
}

async function mountSubfolder(route) {
  const sectionName =
    (route.params.get("section") || "Basics").trim() || "Basics";
  const key = `subfolder:${sectionName}`;
  const root = getView(key, SUBFOLDER_HTML);
  const crumb = root.querySelector('[data-role="section-crumb"]');
  const title = root.querySelector('[data-role="section-title"]');
  const grid = root.querySelector('[data-role="section-grid"]');
  if (crumb) crumb.textContent = sectionName;
  if (title) title.textContent = sectionName;
  wireBackButton(root);
  if (grid && !grid.dataset.loadedFor) {
    const manifest = await getManifest();
    const list = (manifest.tutorials || []).filter(
      (t) => (t.section || "").toLowerCase() === sectionName.toLowerCase(),
    );
    grid.innerHTML = "";
    list.forEach((t) => {
      const card = document.createElement("section");
      card.className = "card";
      const h2 = document.createElement("h2");
      h2.textContent = t.title;
      const p = document.createElement("p");
      p.textContent = t.description || "";
      const code = document.createElement("span");
      code.className = "code";
      code.textContent = t.code || "";
      const a = document.createElement("a");
      a.className = "stretched";
      a.href = `article.html?slug=${encodeURIComponent(t.slug)}`;
      a.setAttribute("aria-label", `${t.title} lesson`);
      card.append(h2, p);
      if (t.code) card.appendChild(code);
      card.appendChild(a);
      grid.appendChild(card);
    });
    if (!list.length) {
      const empty = document.createElement("p");
      empty.textContent = "No tutorials found for this section.";
      empty.style.color = "#355e78";
      grid.appendChild(empty);
    }
    grid.dataset.loadedFor = sectionName;
  }
  document.title = `wqide - ${sectionName}`;
  showView(root);
}

async function mountArticle(route) {
  const slug = route.params.get("slug") || "";
  const key = `article:${slug}`;
  const root = getView(key, ARTICLE_HTML);
  const titleEl = root.querySelector('[data-role="article-title"]');
  const contentEl = root.querySelector('[data-role="article-content"]');
  const crumbTitle = root.querySelector('[data-role="article-title-crumb"]');
  const crumbSection = root.querySelector('[data-role="article-section-link"]');
  const outlineList = root.querySelector('[data-role="outline-list"]');
  const mobileOutline = root.querySelector('[data-role="mobile-outline"]');
  const articleRoot = root.querySelector('[data-role="article-root"]');
  if (articleRoot) articleRoot.setAttribute("data-article-slug", slug);
  wireBackButton(root);

  function fail(msg) {
    if (titleEl) titleEl.textContent = "Not Found";
    if (contentEl) contentEl.textContent = msg;
  }

  if (!root.dataset.loaded) {
    try {
      if (!slug) {
        fail("Missing tutorial slug.");
      } else {
        const manifest = await getManifest();
        const tutorial = (manifest.tutorials || []).find(
          (x) => x.slug === slug,
        );
        if (!tutorial) {
          const topic = slug.startsWith("ref:") ? slug.slice(4) : slug;
          const md = await getDocMarkdown(topic);
          renderArticleMarkdown(md, {
            titleEl,
            contentEl,
            crumbTitle,
            crumbSection,
            section: "Reference",
          });
          root.dataset.loaded = "true";
        } else {
          if (titleEl) titleEl.textContent = tutorial.title;
          if (crumbTitle) crumbTitle.textContent = tutorial.title;
          if (crumbSection) {
            const sect = tutorial.section || "Tutorials";
            crumbSection.textContent = sect;
            crumbSection.setAttribute(
              "href",
              `subfolder.html?section=${encodeURIComponent(sect)}`,
            );
          }
          document.title = `wqide - ${tutorial.title}`;
          const md = await fetch(tutorial.file).then((res) => {
            if (!res.ok)
              throw new Error("Failed to load article: " + res.status);
            return res.text();
          });
          renderArticleMarkdown(md, {
            titleEl,
            contentEl,
            crumbTitle,
            title: tutorial.title,
          });
          root.dataset.loaded = "true";
        }
      }
    } catch (e) {
      const topic = slug.startsWith("ref:") ? slug.slice(4) : slug;
      try {
        const md = await getDocMarkdown(topic);
        renderArticleMarkdown(md, {
          titleEl,
          contentEl,
          crumbTitle,
          crumbSection,
          section: "Reference",
        });
        root.dataset.loaded = "true";
      } catch (docError) {
        console.error(e);
        console.error(docError);
        fail("Error loading tutorial.");
      }
    }
  } else {
    const text = titleEl?.textContent || "Article";
    document.title = `wqide - ${text}`;
  }
  showView(root);
  await getTutorialModule();
  if (window.initTutorialUI) {
    const previousArticle = document.querySelector(
      ".article[data-active-article='true']",
    );
    if (previousArticle && previousArticle !== articleRoot) {
      previousArticle.removeAttribute("data-active-article");
    }
    if (articleRoot) articleRoot.setAttribute("data-active-article", "true");
    if (outlineList) outlineList.id = "outlineList";
    if (mobileOutline) mobileOutline.id = "mobileOutline";
    window.initTutorialUI();
    if (outlineList) outlineList.removeAttribute("id");
    if (mobileOutline) mobileOutline.removeAttribute("id");
  }
}

function renderArticleMarkdown(md, options) {
  const container = document.createElement("div");
  container.innerHTML = parseMarkdown(md);
  const h1 = container.querySelector("h1");
  let title = options.title || "Reference";
  if (h1 && h1 === container.firstElementChild) {
    title = h1.textContent;
    h1.remove();
  }
  if (options.titleEl) options.titleEl.textContent = title;
  if (options.crumbTitle) options.crumbTitle.textContent = title;
  if (options.crumbSection && options.section) {
    options.crumbSection.textContent = options.section;
    options.crumbSection.setAttribute(
      "href",
      `subfolder.html?section=${encodeURIComponent(options.section)}`,
    );
  }
  if (options.contentEl) {
    options.contentEl.innerHTML = "";
    options.contentEl.append(...Array.from(container.childNodes));
  }
  document.title = `wqide - ${title}`;
}

async function mountPlayground(route) {
  const root = getView("playground", PLAYGROUND_HTML);
  const mod = await import("./playground.js");
  if (!root.dataset.booted) {
    if (mod.mountPlayground) {
      await mod.mountPlayground(root);
    }
    root.dataset.booted = "true";
  }
  await mod.activatePlayground?.(root);
  mod.applyPlaygroundRoute?.(root, route.params);
  document.title = "wqide - Playground";
  showView(root);
}

async function mountRepl(route) {
  const root = getView("repl", REPL_HTML);
  const mod = await import("./repl.js");
  if (!root.dataset.booted) {
    if (mod.mountRepl) {
      await mod.mountRepl(root);
    }
    root.dataset.booted = "true";
  }
  mod.activateRepl?.();
  if (route.params.get("input")) {
    const didApply = mod.applyReplRoute?.(root, route.params);
    if (didApply) replaceRouteParams(["input"]);
  }
  document.title = "wqide - REPL";
  showView(root);
}

async function renderRoute() {
  const route = parseRoute();
  state.activeRoute = route;
  persistNav(route.area);
  updateNav(route.area);
  if (route.key === "featured") {
    await mountFeatured();
    return;
  }
  if (route.key === "playground") {
    await mountPlayground(route);
    return;
  }
  if (route.key === "repl") {
    await mountRepl(route);
    return;
  }
  if (route.key === "more") {
    await mountMore();
    return;
  }
  if (route.key.startsWith("subfolder:")) {
    await mountSubfolder(route);
    return;
  }
  if (route.key.startsWith("article:")) {
    await mountArticle(route);
    return;
  }
  await mountFeatured();
}

document.querySelectorAll(".tabs a").forEach((a) => {
  a.addEventListener("click", (e) => {
    e.preventDefault();
    navigate(getTabTargetHref(a.dataset.nav || "featured"));
  });
});

renderRoute();
