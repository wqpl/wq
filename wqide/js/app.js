import { parseMarkdown } from "./markdown.js";
import { escapeHtml, getDocIndex, getDocMarkdown } from "./wq-shared.js";
import {
  FEATURED_SECTION_CARDS,
  buildFeaturedSearchIndex,
  searchFeaturedItems,
} from "./featured-search.js";
import {
  BUILTINS_GROUP,
  referenceBuiltinGroupCards,
  referenceParentHref,
  referenceRootCards,
  referenceSubfolderCrumb,
  referenceSubfolderTitle,
  referenceTopicCards,
} from "./reference-cards.js";
import { PLAYGROUND_EXAMPLES } from "./playground-examples.js";

console.debug("[wqide] app shell loaded");

function html(strings, ...values) {
  return strings.reduce((acc, str, i) => acc + str + (values[i] || ""), "");
}

function featuredDefaultCardHtml(item, index) {
  return html`
    ${index ? '<div class="divider"></div>' : ""}
    <section class="card" data-featured-default-card>
      <h2>${item.title}</h2>
      <p>${item.description}</p>
      <span class="code">${item.code}</span>
      <a class="stretched" href="${item.href}" aria-label="${item.label}">
        Open
      </a>
    </section>
  `;
}

function playgroundExampleCardHtml(example, index) {
  const accent = (index % 4) + 1;
  return html`
    <button
      class="playground-template-card playground-template-card-${accent}"
      type="button"
      data-template="${escapeHtml(example.id)}">
      <strong>${escapeHtml(example.title)}</strong>
      <span class="playground-template-desc"
        >${escapeHtml(example.description)}</span
      >
      <code class="playground-template-code"
        >${escapeHtml(example.sourcePath)}</code
      >
    </button>
  `;
}

const ROUTE_ORDER = ["featured", "playground", "viz", "repl", "more"];
const THEME_STORAGE_KEY = "wqide:theme";
const THEME_MIDNIGHT = "midnight";
const THEME_LIGHT = "light";
const SHELL_HTML = html`
  <header class="topbar">
    <div class="topbar-row">
      <div class="brand">wqide</div>
      <div class="pillbar" aria-label="Quick toggles">
        <div class="pills" role="list">
          <button
            class="pill inactive theme-toggle"
            data-theme-toggle
            type="button"
            aria-label="Toggle midnight mode"
            aria-pressed="false"
            title="Switch to midnight mode">
            theme
          </button>
        </div>
      </div>
    </div>
    <nav class="tabs" role="tablist" aria-label="Sections">
      <a href="index.html" data-nav="featured">Featured</a>
      <a href="playground.html" data-nav="playground">Playground</a>
      <a href="viz.html" data-nav="viz">Viz</a>
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
        <p>wq is a programming language by tttiw.</p>
        <p>
          wqide is a space for learning wq and trying random ideas in your
          browser.
        </p>
      </div>
      <div class="welcome-links" aria-label="Useful articles">
        <a class="article-link" href="article.html?slug=installation">
          <strong>Installation</strong>
          <span>Get a copy of wq.</span>
        </a>
        <a class="article-link" href="article.html?slug=arithmetic">
          <strong>Arithmetic</strong>
          <span>Start the wq book with numbers and list math.</span>
        </a>
      </div>
    </section>

    <section class="featured-search" aria-labelledby="featuredSearchHeading">
      <div class="featured-search-head">
        <h2 id="featuredSearchHeading">Search</h2>
        <span
          class="featured-search-count"
          data-featured-search-count
          aria-live="polite"></span>
      </div>
      <div class="featured-search-box">
        <input
          id="featuredSearchInput"
          data-featured-search-input
          type="search"
          autocomplete="off"
          spellcheck="false"
          placeholder="Search docs, tutorials, builtins, syntax" />
        <button
          class="featured-search-clear"
          data-featured-search-clear
          type="button"
          aria-label="Clear search"
          hidden>
          x
        </button>
      </div>
    </section>

    <div
      class="featured-search-results grid"
      data-featured-search-results
      hidden></div>
    <div class="featured-default" data-featured-default>
      ${FEATURED_SECTION_CARDS.map(featuredDefaultCardHtml).join("")}
    </div>
  </main>
`;

const PLAYGROUND_HTML = html`
  <main class="wrap">
    <div class="playground-shell">
      <aside class="playground-sidebar" aria-labelledby="templateHeading">
        <h2 id="templateHeading">Examples</h2>
        <div class="playground-template-list" role="list">
          ${PLAYGROUND_EXAMPLES.map(playgroundExampleCardHtml).join("")}
        </div>
      </aside>

      <div class="playground-main">
        <div class="editor" role="region" aria-label="Playground code editor">
          <div class="toolbar">
            <div class="toolbar-main">
              <div class="toolbar-left">
                <button id="runBtn" class="btn primary" type="button">
                  Exec
                </button>
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
                  <button
                    id="playgroundTimeBtn"
                    class="pill inactive"
                    type="button">
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
                    <div
                      id="playgroundDebugPanel"
                      class="runtime-panel debug-panel">
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
        </div>

      </div>

      <aside class="playground-inspector" aria-label="Playground inspector">
        <section class="symbol-panel" aria-labelledby="symbolPanelHeading">
          <div class="symbol-panel-head">
            <h2 id="symbolPanelHeading">Symbols</h2>
            <span class="symbol-panel-count" data-symbol-count>0</span>
          </div>
          <div class="symbol-panel-status" data-symbol-status>
            No symbols yet.
          </div>
          <div class="symbol-panel-list" data-symbol-list></div>
        </section>

        <section
          class="structure-panel"
          aria-labelledby="structurePanelHeading">
          <div class="structure-panel-head">
            <h2 id="structurePanelHeading">Structure</h2>
            <div
              class="structure-tabs"
              role="tablist"
              aria-label="Structure view">
              <button
                class="structure-tab active"
                type="button"
                role="tab"
                aria-selected="true"
                aria-controls="structurePanelBody"
                data-structure-mode="ast">
                AST
              </button>
              <button
                class="structure-tab"
                type="button"
                role="tab"
                aria-selected="false"
                aria-controls="structurePanelBody"
                data-structure-mode="cst">
                CST
              </button>
            </div>
          </div>
          <div class="structure-panel-status" data-structure-status hidden></div>
          <pre
            id="structurePanelBody"
            class="structure-panel-body empty"
            data-structure-output
            role="tabpanel"
            aria-live="polite">No code yet.</pre>
        </section>
      </aside>

      <div
        class="run-output-panel"
        role="region"
        aria-labelledby="runOutputHeading"
        aria-live="polite">
        <div class="run-output-header">
          <span id="runOutputHeading" class="run-output-title">Output</span>
          <button id="clearOutBtn" class="run-output-clear" type="button">
            Clear
          </button>
        </div>
        <pre class="run-output-body"></pre>
      </div>
    </div>
  </main>
`;

const VIZ_HTML = html`
  <main class="wrap viz-wrap">
    <div class="viz-shell">
      <section class="viz-topbar" aria-label="Viz summary">
        <div class="viz-stage-title">
          <div class="viz-stage-title-row">
            <h1 data-viz-title>Function plot</h1>
            <div class="viz-preset-menu" data-viz-preset-menu>
              <button
                class="viz-preset-trigger"
                type="button"
                data-viz-preset-toggle
                aria-haspopup="menu"
                aria-expanded="false"
                aria-controls="vizPresetMenu">
                <span>Presets</span>
              </button>
              <div
                class="viz-preset-popover"
                id="vizPresetMenu"
                data-viz-preset-panel
                role="menu"
                aria-label="Viz presets">
                <section
                  class="viz-preset-group"
                  role="group"
                  aria-labelledby="vizPresetAsciiplot">
                  <h2 id="vizPresetAsciiplot">asciiplot</h2>
                  <div class="viz-preset-list">
                    <button
                      class="viz-preset active"
                      type="button"
                      role="menuitemradio"
                      aria-checked="true"
                      data-viz-preset="trig">
                      <span class="viz-preset-title">Function plot</span>
                      <span class="viz-preset-meta">sin / cos</span>
                    </button>
                    <button
                      class="viz-preset"
                      type="button"
                      role="menuitemradio"
                      aria-checked="false"
                      data-viz-preset="data">
                      <span class="viz-preset-title">Data series</span>
                      <span class="viz-preset-meta">raw values</span>
                    </button>
                    <button
                      class="viz-preset"
                      type="button"
                      role="menuitemradio"
                      aria-checked="false"
                      data-viz-preset="tablePlot">
                      <span class="viz-preset-title">Table plot</span>
                      <span class="viz-preset-meta">x + y columns</span>
                    </button>
                    <button
                      class="viz-preset"
                      type="button"
                      role="menuitemradio"
                      aria-checked="false"
                      data-viz-preset="cas">
                      <span class="viz-preset-title">CAS curve</span>
                      <span class="viz-preset-meta">symbolic x</span>
                    </button>
                    <button
                      class="viz-preset"
                      type="button"
                      role="menuitemradio"
                      aria-checked="false"
                      data-viz-preset="modes">
                      <span class="viz-preset-title">Mode mixer</span>
                      <span class="viz-preset-meta">line + scatter</span>
                    </button>
                    <button
                      class="viz-preset"
                      type="button"
                      role="menuitemradio"
                      aria-checked="false"
                      data-viz-preset="bars">
                      <span class="viz-preset-title">Bars</span>
                      <span class="viz-preset-meta">series values</span>
                    </button>
                    <button
                      class="viz-preset"
                      type="button"
                      role="menuitemradio"
                      aria-checked="false"
                      data-viz-preset="complex">
                      <span class="viz-preset-title">Complex plane</span>
                      <span class="viz-preset-meta">sqrt[x]</span>
                    </button>
                  </div>
                </section>
                <section
                  class="viz-preset-group"
                  role="group"
                  aria-labelledby="vizPresetShowtable">
                  <h2 id="vizPresetShowtable">showtable</h2>
                  <div class="viz-preset-list">
                    <button
                      class="viz-preset"
                      type="button"
                      role="menuitemradio"
                      aria-checked="false"
                      data-viz-preset="table">
                      <span class="viz-preset-title">Show table</span>
                      <span class="viz-preset-meta">string cells</span>
                    </button>
                    <button
                      class="viz-preset"
                      type="button"
                      role="menuitemradio"
                      aria-checked="false"
                      data-viz-preset="tableMap">
                      <span class="viz-preset-title">Math map</span>
                      <span class="viz-preset-meta">dict of dicts</span>
                    </button>
                  </div>
                </section>
              </div>
            </div>
          </div>
        </div>
        <div class="viz-stage-actions">
          <label class="viz-live-switch">
            <input type="checkbox" data-viz-toggle="autoRun" checked />
            <span>Live</span>
          </label>
          <div
            class="viz-layout-toggle"
            role="group"
            aria-label="Control layout">
            <button class="active" type="button" data-viz-layout-option="below">
              Below
            </button>
            <button type="button" data-viz-layout-option="side">Side</button>
          </div>
          <span class="viz-builtin-chip" data-viz-builtin>asciiplot</span>
          <span class="viz-status" data-viz-status>ready</span>
          <button class="btn primary" type="button" data-viz-run>
            Refresh
          </button>
          <button class="btn" type="button" data-viz-open>
            Open in Playground
          </button>
        </div>
      </section>

      <div class="viz-workbench">
        <section class="viz-stage" aria-label="Viz output">
          <div class="viz-output-frame">
            <div class="viz-output-head">
              <span>Output</span>
            </div>
            <pre class="viz-output" data-viz-output aria-live="polite"></pre>
          </div>

          <section
            class="viz-control-group viz-data-panel"
            data-viz-control-group="source">
            <div class="viz-control-head">
              <h2>Data</h2>
            </div>
            <div class="viz-field viz-output-kind" data-viz-select="sourceKind">
              <label>Output</label>
              <button
                class="viz-select-button"
                type="button"
                aria-haspopup="listbox"
                aria-expanded="false">
                <span data-viz-select-value>plot</span>
              </button>
              <div class="viz-select-menu" role="listbox">
                <button type="button" role="option" data-viz-option="plot">
                  plot
                </button>
                <button type="button" role="option" data-viz-option="table">
                  table
                </button>
              </div>
            </div>
            <div class="viz-series-editor" data-viz-series-editor>
              <div class="viz-series-list" data-viz-series-list></div>
              <button class="viz-small-btn" type="button" data-viz-add-series>
                Add series
              </button>
            </div>
            <div class="viz-table-plot-config" data-viz-table-plot-config>
              <div class="viz-inline-fields">
                <label class="viz-text-field">
                  <span>X column</span>
                  <input
                    type="text"
                    spellcheck="false"
                    data-viz-input="tableXText" />
                </label>
                <label class="viz-text-field">
                  <span>Y columns</span>
                  <input
                    type="text"
                    spellcheck="false"
                    data-viz-input="tableYText" />
                </label>
              </div>
            </div>
            <div class="viz-table-config" data-viz-table-config>
              <div class="viz-control-grid">
                <div class="viz-field" data-viz-select="tableShape">
                  <label>Shape</label>
                  <button
                    class="viz-select-button"
                    type="button"
                    aria-haspopup="listbox"
                    aria-expanded="false">
                    <span data-viz-select-value>list of dicts</span>
                  </button>
                  <div class="viz-select-menu" role="listbox">
                    <button type="button" role="option" data-viz-option="text">
                      biology cells
                    </button>
                    <button type="button" role="option" data-viz-option="list">
                      physics rows
                    </button>
                    <button type="button" role="option" data-viz-option="dict">
                      chem columns
                    </button>
                    <button
                      type="button"
                      role="option"
                      data-viz-option="matrix">
                      math map
                    </button>
                  </div>
                </div>
                <div
                  class="viz-stepper"
                  role="group"
                  aria-label="Generated rows">
                  <span>Rows</span>
                  <button
                    class="viz-stepper-btn"
                    type="button"
                    aria-label="Fewer rows"
                    data-viz-step="rows"
                    data-viz-step-delta="-1">
                    -
                  </button>
                  <input
                    type="number"
                    min="1"
                    max="8"
                    value="5"
                    data-viz-range="rows" />
                  <button
                    class="viz-stepper-btn"
                    type="button"
                    aria-label="More rows"
                    data-viz-step="rows"
                    data-viz-step-delta="1">
                    +
                  </button>
                </div>
              </div>
            </div>
            <label
              class="viz-text-field viz-text-field-tall viz-table-source"
              data-viz-table-source>
              <span>Table value</span>
              <textarea
                class="editor-text"
                rows="6"
                spellcheck="false"
                data-viz-input="sourceExpr"></textarea>
            </label>
          </section>

          <div class="viz-code-panel-wrap">
            <details class="viz-code-panel">
              <summary>Code</summary>
              <pre><code data-viz-code></code></pre>
            </details>
            <button class="viz-code-copy" type="button" data-viz-copy-code>
              Copy
            </button>
          </div>
        </section>

        <aside
          class="viz-controls viz-style-panel"
          aria-label="Viz style controls">
          <section class="viz-control-group" data-viz-control-group="plot">
            <div class="viz-control-head">
              <h2>Plot</h2>
            </div>
            <div class="viz-control-grid">
              <div class="viz-field" data-viz-select="mode">
                <label>Mode</label>
                <button
                  class="viz-select-button"
                  type="button"
                  aria-haspopup="listbox"
                  aria-expanded="false">
                  <span data-viz-select-value>line</span>
                </button>
                <div class="viz-select-menu" role="listbox">
                  <button type="button" role="option" data-viz-option="line">
                    line
                  </button>
                  <button type="button" role="option" data-viz-option="scatter">
                    scatter
                  </button>
                  <button type="button" role="option" data-viz-option="step">
                    step
                  </button>
                  <button type="button" role="option" data-viz-option="bar">
                    bar
                  </button>
                  <button type="button" role="option" data-viz-option="area">
                    area
                  </button>
                </div>
              </div>
              <div class="viz-field" data-viz-select="complex">
                <label>Complex</label>
                <button
                  class="viz-select-button"
                  type="button"
                  aria-haspopup="listbox"
                  aria-expanded="false">
                  <span data-viz-select-value>re</span>
                </button>
                <div class="viz-select-menu" role="listbox">
                  <button type="button" role="option" data-viz-option="re">
                    re
                  </button>
                  <button type="button" role="option" data-viz-option="im">
                    im
                  </button>
                  <button type="button" role="option" data-viz-option="abs">
                    abs
                  </button>
                  <button type="button" role="option" data-viz-option="arg">
                    arg
                  </button>
                  <button type="button" role="option" data-viz-option="plane">
                    plane
                  </button>
                </div>
              </div>
              <div class="viz-field" data-viz-select="theme">
                <label>Theme</label>
                <button
                  class="viz-select-button"
                  type="button"
                  aria-haspopup="listbox"
                  aria-expanded="false">
                  <span data-viz-select-value>none</span>
                </button>
                <div class="viz-select-menu" role="listbox">
                  <button type="button" role="option" data-viz-option="none">
                    none
                  </button>
                  <button type="button" role="option" data-viz-option="minimal">
                    minimal
                  </button>
                  <button type="button" role="option" data-viz-option="maximal">
                    maximal
                  </button>
                </div>
              </div>
              <div class="viz-field" data-viz-select="axes">
                <label>Axes</label>
                <button
                  class="viz-select-button"
                  type="button"
                  aria-haspopup="listbox"
                  aria-expanded="false">
                  <span data-viz-select-value>full</span>
                </button>
                <div class="viz-select-menu" role="listbox">
                  <button type="button" role="option" data-viz-option="full">
                    full
                  </button>
                  <button type="button" role="option" data-viz-option="minimal">
                    minimal
                  </button>
                  <button type="button" role="option" data-viz-option="off">
                    off
                  </button>
                </div>
              </div>
              <div class="viz-field" data-viz-select="grid">
                <label>Grid</label>
                <button
                  class="viz-select-button"
                  type="button"
                  aria-haspopup="listbox"
                  aria-expanded="false">
                  <span data-viz-select-value>4</span>
                </button>
                <div class="viz-select-menu" role="listbox">
                  <button type="button" role="option" data-viz-option="off">
                    off
                  </button>
                  <button type="button" role="option" data-viz-option="4">
                    4
                  </button>
                  <button type="button" role="option" data-viz-option="8">
                    8
                  </button>
                </div>
              </div>
              <div class="viz-field" data-viz-select="palette">
                <label>Palette</label>
                <button
                  class="viz-select-button"
                  type="button"
                  aria-haspopup="listbox"
                  aria-expanded="false">
                  <span data-viz-select-value>classic</span>
                </button>
                <div class="viz-select-menu" role="listbox">
                  <button type="button" role="option" data-viz-option="classic">
                    classic
                  </button>
                  <button type="button" role="option" data-viz-option="bright">
                    bright
                  </button>
                  <button type="button" role="option" data-viz-option="ink">
                    ink
                  </button>
                  <button type="button" role="option" data-viz-option="off">
                    off
                  </button>
                </div>
              </div>
            </div>
          </section>

          <section class="viz-control-group" data-viz-control-group="limits">
            <div class="viz-control-head">
              <h2>Bounds</h2>
            </div>
            <div class="viz-range viz-range-smart">
              <span>Width</span>
              <input
                type="range"
                min="40"
                max="120"
                value="90"
                data-viz-range="width"
                aria-label="Manual plot width" />
              <strong data-viz-range-value="width">auto 90</strong>
              <label class="viz-lock-toggle viz-auto-toggle">
                <input type="checkbox" data-viz-toggle="widthAuto" checked />
                <span>Auto</span>
              </label>
            </div>
            <label class="viz-range">
              <span>Height</span>
              <input
                type="range"
                min="10"
                max="32"
                value="24"
                data-viz-range="height" />
              <strong data-viz-range-value="height">24</strong>
            </label>
            <label class="viz-range">
              <span>Samples</span>
              <input
                type="range"
                min="20"
                max="260"
                step="10"
                value="140"
                data-viz-range="samples" />
              <strong data-viz-range-value="samples">140</strong>
            </label>
            <div class="viz-limit-grid">
              <div
                class="viz-limit-pair"
                role="group"
                aria-labelledby="viz-xlim-label">
                <div class="viz-limit-pair-head">
                  <span id="viz-xlim-label">X lim</span>
                  <label class="viz-lock-toggle">
                    <input type="checkbox" data-viz-toggle="xlimLocked" />
                    <span>Lock</span>
                  </label>
                </div>
                <div class="viz-limit-inputs">
                  <label class="viz-text-field">
                    <span>Min</span>
                    <input
                      type="text"
                      spellcheck="false"
                      data-viz-input="xlimMinText" />
                  </label>
                  <label class="viz-text-field">
                    <span>Max</span>
                    <input
                      type="text"
                      spellcheck="false"
                      data-viz-input="xlimMaxText" />
                  </label>
                </div>
              </div>
              <div
                class="viz-limit-pair"
                role="group"
                aria-labelledby="viz-ylim-label">
                <div class="viz-limit-pair-head">
                  <span id="viz-ylim-label">Y lim</span>
                  <label class="viz-lock-toggle">
                    <input type="checkbox" data-viz-toggle="ylimLocked" />
                    <span>Lock</span>
                  </label>
                </div>
                <div class="viz-limit-inputs">
                  <label class="viz-text-field">
                    <span>Min</span>
                    <input
                      type="text"
                      spellcheck="false"
                      data-viz-input="ylimMinText" />
                  </label>
                  <label class="viz-text-field">
                    <span>Max</span>
                    <input
                      type="text"
                      spellcheck="false"
                      data-viz-input="ylimMaxText" />
                  </label>
                </div>
              </div>
            </div>
            <div class="viz-inline-fields">
              <label class="viz-text-field">
                <span>Title</span>
                <input
                  type="text"
                  spellcheck="false"
                  data-viz-input="titleText" />
              </label>
              <label class="viz-text-field">
                <span>X label</span>
                <input
                  type="text"
                  spellcheck="false"
                  data-viz-input="xlabelText" />
              </label>
              <label class="viz-text-field">
                <span>Y label</span>
                <input
                  type="text"
                  spellcheck="false"
                  data-viz-input="ylabelText" />
              </label>
            </div>
          </section>

          <section class="viz-control-group" data-viz-control-group="series">
            <div class="viz-control-head">
              <h2>Series</h2>
            </div>
            <div class="viz-switch-row">
              <label class="viz-switch">
                <input type="checkbox" data-viz-toggle="labels" checked />
                <span>Labels</span>
              </label>
              <label class="viz-switch">
                <input
                  type="checkbox"
                  data-viz-toggle="seriesOptions"
                  checked />
                <span>Per-series</span>
              </label>
              <label class="viz-switch">
                <input type="checkbox" data-viz-toggle="ascii" />
                <span>ASCII</span>
              </label>
            </div>
          </section>

          <section class="viz-control-group" data-viz-control-group="table">
            <div class="viz-control-head">
              <h2>Table Display</h2>
            </div>
            <div class="viz-control-grid">
              <div class="viz-field" data-viz-select="tableStyle">
                <label>Style</label>
                <button
                  class="viz-select-button"
                  type="button"
                  aria-haspopup="listbox"
                  aria-expanded="false">
                  <span data-viz-select-value>plain</span>
                </button>
                <div class="viz-select-menu" role="listbox">
                  <button type="button" role="option" data-viz-option="plain">
                    plain
                  </button>
                  <button
                    type="button"
                    role="option"
                    data-viz-option="markdown">
                    markdown
                  </button>
                </div>
              </div>
              <label class="viz-text-field">
                <span>Columns</span>
                <input
                  type="text"
                  spellcheck="false"
                  data-viz-input="tableColsText" />
              </label>
              <label class="viz-text-field">
                <span>Limit</span>
                <input
                  type="text"
                  inputmode="numeric"
                  spellcheck="false"
                  data-viz-input="tableLimitText" />
              </label>
              <label class="viz-text-field">
                <span>Cell width</span>
                <input
                  type="text"
                  inputmode="numeric"
                  spellcheck="false"
                  placeholder="auto"
                  data-viz-input="tableWidthText" />
              </label>
              <label class="viz-text-field">
                <span>Missing</span>
                <input
                  type="text"
                  spellcheck="false"
                  data-viz-input="tableMissingText" />
              </label>
            </div>
          </section>
        </aside>
      </div>
    </div>
  </main>
`;

const REPL_HTML = html`
  <main class="wrap repl-wrap">
    <div class="repl-shell">
      <div class="repl repl-flow">
        <div class="toolbar headbar repl-topbar">
          <div class="repl-actions">
            <div class="repl-copy-actions">
              <button id="copyFlowBtn" class="btn repl-copy-btn" type="button">
                Copy Flow
              </button>
              <button
                id="copyOutputBtn"
                class="btn repl-copy-btn"
                type="button">
                Copy Output
              </button>
            </div>
            <div
              class="repl-runtime-actions"
              aria-label="REPL runtime controls">
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
              <button id="resetBtn" class="btn" type="button">
                Reset Session
              </button>
              <button id="clearBtn" class="btn" type="button">
                Clear Flow
              </button>
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
              <span class="mini composer-hint"
                >Enter: exec | Shift-Enter: newline</span
              >
            </div>
            <div class="composer-actions">
              <div class="stdin composer-stdin">
                <span class="mini">stdin:</span>
                <input
                  id="stdinLine"
                  type="text"
                  placeholder="Queue stdin for the next run..." />
                <button id="pushStdinBtn" class="btn" type="button">
                  Queue
                </button>
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
      <aside class="globals-panel" aria-labelledby="globalsPanelHeading">
        <div class="globals-panel-head">
          <h2 id="globalsPanelHeading">Globals</h2>
          <div class="globals-panel-actions">
            <span id="globalsCount" class="globals-panel-count">0</span>
            <button id="refreshGlobalsBtn" class="btn" type="button">
              Refresh
            </button>
          </div>
        </div>
        <pre
          id="globalsBody"
          class="globals-panel-body"
          aria-live="polite"></pre>
      </aside>
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
        <span aria-hidden="true">&#8592;</span>
      </button>
      <div class="crumb-path">
        <a href="index.html">~</a><span class="sep">/</span
        ><span class="crumb-current" data-role="section-crumb">Basics</span>
      </div>
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
        <span aria-hidden="true">&#8592;</span>
      </button>
      <div class="crumb-path">
        <a href="index.html">~</a><span class="sep">/</span
        ><a
          class="crumb-section"
          data-role="article-section-link"
          href="subfolder.html"
          >Section</a
        ><span class="sep">/</span
        ><span class="crumb-current" data-role="article-title-crumb"
          >Loading...</span
        >
      </div>
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

function readStoredTheme() {
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    if (stored === THEME_MIDNIGHT || stored === THEME_LIGHT) return stored;
  } catch (err) {
    console.debug("theme read failed", err);
  }
  return document.documentElement.dataset.theme === THEME_MIDNIGHT
    ? THEME_MIDNIGHT
    : THEME_LIGHT;
}

function syncThemeToggle() {
  const button = document.querySelector("[data-theme-toggle]");
  if (!button) return;
  const isMidnight = document.documentElement.dataset.theme === THEME_MIDNIGHT;
  button.classList.toggle("active", isMidnight);
  button.classList.toggle("inactive", !isMidnight);
  button.setAttribute("aria-pressed", String(isMidnight));
  button.title = isMidnight
    ? "Switch to light mode"
    : "Switch to midnight mode";
}

function applyTheme(theme, options = {}) {
  const next = theme === THEME_MIDNIGHT ? THEME_MIDNIGHT : THEME_LIGHT;
  document.documentElement.dataset.theme = next;
  if (options.persist !== false) {
    try {
      localStorage.setItem(THEME_STORAGE_KEY, next);
    } catch (err) {
      console.debug("theme persist failed", err);
    }
  }
  syncThemeToggle();
  return next;
}

function wireThemeToggle() {
  const button = document.querySelector("[data-theme-toggle]");
  if (!button || button.dataset.wired === "true") return;
  button.dataset.wired = "true";
  syncThemeToggle();
  button.addEventListener("click", () => {
    const isMidnight =
      document.documentElement.dataset.theme === THEME_MIDNIGHT;
    applyTheme(isMidnight ? THEME_LIGHT : THEME_MIDNIGHT);
  });
}

applyTheme(readStoredTheme(), { persist: false });
document.body.innerHTML = SHELL_HTML;
wireThemeToggle();

const main = document.getElementById("appMain");
const state = {
  manifestPromise: null,
  featuredSearchIndexPromise: null,
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
  if (file === "viz.html") {
    return { key: "viz", area: "viz", params };
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
    viz: getClean("nav:last:viz", "viz.html"),
    repl: getClean("nav:last:repl", "repl.html"),
    more: getClean("nav:last:more", "more.html"),
  };
}

function getAreaBaseHref(nav) {
  return nav === "featured"
    ? "index.html"
    : nav === "playground"
      ? "playground.html"
      : nav === "viz"
        ? "viz.html"
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

async function getFeaturedSearchIndex() {
  if (!state.featuredSearchIndexPromise) {
    state.featuredSearchIndexPromise = Promise.allSettled([
      getManifest(),
      getDocIndex(),
    ]).then(([manifestResult, docsResult]) => {
      const warnings = [];
      const tutorials =
        manifestResult.status === "fulfilled"
          ? manifestResult.value.tutorials || []
          : [];
      const docs = docsResult.status === "fulfilled" ? docsResult.value : [];
      if (manifestResult.status === "rejected") {
        console.warn(
          "featured search manifest load failed",
          manifestResult.reason,
        );
        warnings.push("Tutorials unavailable.");
      }
      if (docsResult.status === "rejected") {
        console.warn("featured search docs load failed", docsResult.reason);
        warnings.push("Reference docs unavailable.");
      }
      return {
        index: buildFeaturedSearchIndex({ tutorials, docs }),
        warnings,
      };
    });
  }
  return state.featuredSearchIndexPromise;
}

function setFeaturedSearchQueryParam(query) {
  const params = new URLSearchParams(location.search);
  const trimmed = query.trim();
  if (trimmed) {
    params.set("q", trimmed);
  } else {
    params.delete("q");
  }
  const next =
    location.pathname +
    (params.toString() ? `?${params.toString()}` : "") +
    (location.hash || "");
  const current = location.pathname + location.search + location.hash;
  if (next !== current) {
    history.replaceState({}, "", next);
  }
  persistNav("featured");
  updateNav("featured");
}

function setFeaturedSearchStatus(root, text) {
  const count = root.querySelector("[data-featured-search-count]");
  if (count) count.textContent = text;
}

function clearFeaturedSearchResults(root) {
  const results = root.querySelector("[data-featured-search-results]");
  const defaults = root.querySelector("[data-featured-default]");
  if (results) {
    results.innerHTML = "";
    results.hidden = true;
  }
  if (defaults) defaults.hidden = false;
  setFeaturedSearchStatus(root, "");
}

function renderFeaturedSearchResults(root, matches, warnings) {
  const results = root.querySelector("[data-featured-search-results]");
  if (!results) return;
  results.innerHTML = "";
  if (matches.length) {
    matches.forEach((item) =>
      appendSectionCard(results, item, { showMeta: true }),
    );
  } else {
    const empty = document.createElement("p");
    empty.className = "featured-search-empty";
    empty.textContent = "No results.";
    results.appendChild(empty);
  }
  const resultText =
    matches.length === 1 ? "1 result" : `${matches.length} results`;
  setFeaturedSearchStatus(
    root,
    [resultText, ...(warnings || [])].filter(Boolean).join(" "),
  );
}

async function runFeaturedSearch(root, query) {
  const input = root.querySelector("[data-featured-search-input]");
  const clear = root.querySelector("[data-featured-search-clear]");
  const results = root.querySelector("[data-featured-search-results]");
  const defaults = root.querySelector("[data-featured-default]");
  const trimmed = query.trim();
  const seq = Number(root.dataset.featuredSearchSeq || 0) + 1;
  root.dataset.featuredSearchSeq = String(seq);
  if (clear) clear.hidden = !trimmed;
  if (!trimmed) {
    clearFeaturedSearchResults(root);
    return;
  }
  if (defaults) defaults.hidden = true;
  if (results) {
    results.hidden = false;
    results.innerHTML = "";
  }
  setFeaturedSearchStatus(root, "Searching...");
  try {
    const { index, warnings } = await getFeaturedSearchIndex();
    if (root.dataset.featuredSearchSeq !== String(seq)) return;
    if (input && input.value.trim() !== trimmed) return;
    const matches = searchFeaturedItems(index, trimmed);
    renderFeaturedSearchResults(root, matches, warnings);
  } catch (err) {
    console.error("featured search failed", err);
    if (root.dataset.featuredSearchSeq === String(seq)) {
      renderFeaturedSearchResults(root, [], ["Search unavailable."]);
    }
  }
}

function clearFeaturedSearch(root) {
  const input = root.querySelector("[data-featured-search-input]");
  if (!input) return;
  input.value = "";
  setFeaturedSearchQueryParam("");
  runFeaturedSearch(root, "");
}

function wireFeaturedSearch(root) {
  const input = root.querySelector("[data-featured-search-input]");
  const clear = root.querySelector("[data-featured-search-clear]");
  if (!input || input.dataset.wired === "true") return;
  input.dataset.wired = "true";
  input.addEventListener("input", () => {
    setFeaturedSearchQueryParam(input.value);
    runFeaturedSearch(root, input.value);
  });
  input.addEventListener("focus", () => {
    getFeaturedSearchIndex().catch((err) => {
      console.warn("featured search preload failed", err);
    });
  });
  input.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && input.value) {
      event.preventDefault();
      clearFeaturedSearch(root);
      return;
    }
    if (event.key === "Enter" && input.value.trim()) {
      const firstLink = root.querySelector(
        "[data-featured-search-results] a.stretched",
      );
      if (firstLink) {
        event.preventDefault();
        navigate(firstLink.getAttribute("href"));
      }
    }
  });
  clear?.addEventListener("click", () => {
    clearFeaturedSearch(root);
    input.focus();
  });
}

function applyFeaturedSearchRoute(root, route) {
  const input = root.querySelector("[data-featured-search-input]");
  if (!input) return;
  const query = route?.params?.get("q") || "";
  if (input.value !== query) input.value = query;
  runFeaturedSearch(root, query);
}

function isReferenceSection(sectionName) {
  return sectionName.toLowerCase() === "reference";
}

function appendSectionCard(grid, item, options = {}) {
  const card = document.createElement("section");
  card.className = "card";
  if (options.showMeta && item.meta) {
    const meta = document.createElement("span");
    meta.className = "card-meta";
    if (item.type) meta.dataset.cardKind = item.type;
    meta.textContent = item.meta;
    card.appendChild(meta);
  }
  const h2 = document.createElement("h2");
  h2.textContent = item.title;
  const p = document.createElement("p");
  p.textContent = item.description || "";
  const code = document.createElement("span");
  code.className = "code";
  code.textContent = item.code || "";
  const a = document.createElement("a");
  a.className = "stretched";
  a.href = item.href;
  a.setAttribute("aria-label", item.label);
  card.append(h2, p);
  if (item.code) card.appendChild(code);
  card.appendChild(a);
  grid.appendChild(card);
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
      navigate(root.dataset.parentHref || "index.html");
      return;
    }
    navigate("index.html");
  });
}

async function mountFeatured(route) {
  const root = getView("featured", FEATURED_HTML);
  wireFeaturedSearch(root);
  applyFeaturedSearchRoute(root, route);
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
  const referenceGroup = isReferenceSection(sectionName)
    ? (route.params.get("group") || "").trim()
    : "";
  const builtinGroup =
    referenceGroup === BUILTINS_GROUP
      ? (route.params.get("builtinGroup") || "").trim()
      : "";
  const key = builtinGroup
    ? `subfolder:${sectionName}:${referenceGroup}:${builtinGroup}`
    : referenceGroup
      ? `subfolder:${sectionName}:${referenceGroup}`
      : `subfolder:${sectionName}`;
  const root = getView(key, SUBFOLDER_HTML);
  const crumb = root.querySelector('[data-role="section-crumb"]');
  const title = root.querySelector('[data-role="section-title"]');
  const grid = root.querySelector('[data-role="section-grid"]');
  const titleText = isReferenceSection(sectionName)
    ? referenceSubfolderTitle({ sectionName, referenceGroup, builtinGroup })
    : sectionName;
  root.dataset.parentHref = isReferenceSection(sectionName)
    ? referenceParentHref({ referenceGroup, builtinGroup })
    : "index.html";
  if (crumb) {
    crumb.textContent = isReferenceSection(sectionName)
      ? referenceSubfolderCrumb({ sectionName, referenceGroup, builtinGroup })
      : sectionName;
  }
  if (title) title.textContent = titleText;
  wireBackButton(root);
  if (grid && !grid.dataset.loadedFor) {
    const docs = isReferenceSection(sectionName) ? await getDocIndex() : [];
    const list = isReferenceSection(sectionName)
      ? builtinGroup
        ? referenceTopicCards(docs, { builtinGroup })
        : referenceGroup === BUILTINS_GROUP
          ? referenceBuiltinGroupCards(docs)
          : referenceGroup
            ? referenceTopicCards(docs, { group: referenceGroup })
            : referenceRootCards(docs)
      : ((await getManifest()).tutorials || []).filter(
          (t) => (t.section || "").toLowerCase() === sectionName.toLowerCase(),
        );
    grid.innerHTML = "";
    list.forEach((t) => {
      appendSectionCard(
        grid,
        isReferenceSection(sectionName)
          ? t
          : {
              title: t.title,
              description: t.description,
              code: t.code,
              href: `article.html?slug=${encodeURIComponent(t.slug)}`,
              label: `${t.title} lesson`,
            },
      );
    });
    if (!list.length) {
      const empty = document.createElement("p");
      empty.textContent = "No tutorials found for this section.";
      empty.style.color = "#355e78";
      grid.appendChild(empty);
    }
    grid.dataset.loadedFor = titleText;
  }
  document.title = `wqide - ${titleText}`;
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

async function mountViz(route) {
  const root = getView("viz", VIZ_HTML);
  const mod = await import("./viz.js");
  if (!root.dataset.booted) {
    if (mod.mountViz) {
      await mod.mountViz(root);
    }
    root.dataset.booted = "true";
  }
  await mod.activateViz?.(root);
  mod.applyVizRoute?.(root, route.params);
  document.title = "wqide - Viz";
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
    await mountFeatured(route);
    return;
  }
  if (route.key === "playground") {
    await mountPlayground(route);
    return;
  }
  if (route.key === "viz") {
    await mountViz(route);
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
  await mountFeatured(route);
}

document.querySelectorAll(".tabs a").forEach((a) => {
  a.addEventListener("click", (e) => {
    e.preventDefault();
    navigate(getTabTargetHref(a.dataset.nav || "featured"));
  });
});

renderRoute();
