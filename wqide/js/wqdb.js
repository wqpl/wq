import { highlightedSourceLineFragments } from "./syntax-highlight.js";
import { wireSegmentedControl } from "./ui-segmented.js";

const RESUME_ACTIONS = new Set([
  "continue",
  "step_in",
  "step_over",
  "step_out",
]);

function initialState() {
  return {
    status: "idle",
    pause: null,
    source: "",
    sourcePath: null,
    stack: [],
    selectedFrame: 0,
    locals: [],
    globals: [],
    instruction: null,
    granularity: "expr",
    breakpointLines: [],
    breakpoints: [],
    trackers: [],
    notifications: [],
  };
}

function targetLabel(target) {
  if (target.name) return `${target.scope} ${target.name}`;
  if (target.slot !== null) return `${target.scope} slot ${target.slot}`;
  return target.scope;
}

function reasonLabel(reason) {
  return reason.replaceAll("_", " ");
}

function locationLabel(pause) {
  const location = pause?.location;
  if (!location) return "location unavailable";
  const source =
    location.path && location.line !== null
      ? `${location.path}:${location.line}${location.column === null ? "" : `:${location.column}`}`
      : `chunk ${location.chunk}, pc ${location.pc}`;
  return location.function ? `${source} in ${location.function}` : source;
}

function readStop(stop, selectedFrame) {
  const stack = stop.stack();
  const frame = Math.max(
    0,
    Math.min(selectedFrame, Math.max(0, stack.length - 1)),
  );
  let locals = [];
  if (stack.length) {
    try {
      locals = stop.locals(frame);
    } catch {
      locals = [];
    }
  }
  return {
    stack,
    selectedFrame: frame,
    locals,
    globals: stop.globals(),
    instruction: stop.instruction(),
    granularity: stop.granularity(),
    trackers: stop.trackers(),
  };
}

export function createWqdbController(onChange = () => {}) {
  let state = initialState();
  let pending = null;

  function publish(next) {
    state = { ...state, ...next };
    onChange(state);
  }

  function requirePause() {
    if (!pending) throw new Error("The debugger is not paused");
    return pending;
  }

  function refresh(extra = {}) {
    const active = requirePause();
    const selectedFrame = extra.selectedFrame ?? state.selectedFrame;
    const inspected = readStop(active.stop, selectedFrame);
    publish({
      ...inspected,
      ...extra,
      selectedFrame: inspected.selectedFrame,
    });
  }

  return {
    get state() {
      return state;
    },

    pause(stop, { source, sourcePath }) {
      if (pending) {
        pending.resolve("continue");
      }
      let resolve;
      const result = new Promise((next) => {
        resolve = next;
      });
      pending = { stop, resolve };
      const sameSource = state.sourcePath === sourcePath;
      state = {
        ...initialState(),
        status: "paused",
        pause: stop.pause,
        source: String(source),
        sourcePath,
        breakpointLines: sameSource ? state.breakpointLines : [],
        breakpoints: sameSource ? state.breakpoints : [],
        notifications: state.notifications,
        ...readStop(stop, 0),
      };
      onChange(state);
      return result;
    },

    resume(action) {
      if (!RESUME_ACTIONS.has(action)) {
        throw new TypeError(`Unknown debugger resume action '${action}'`);
      }
      if (!pending) return false;
      const active = pending;
      pending = null;
      publish({ status: "running" });
      active.resolve(action);
      return true;
    },

    finish() {
      if (pending) {
        const active = pending;
        pending = null;
        active.resolve("continue");
      }
      publish({ status: "idle" });
    },

    reset() {
      if (pending) {
        const active = pending;
        pending = null;
        active.resolve("continue");
      }
      state = initialState();
      onChange(state);
    },

    selectFrame(frameIndex) {
      requirePause();
      refresh({ selectedFrame: frameIndex });
    },

    setGranularity(granularity, { render = true } = {}) {
      const active = requirePause();
      const selected = active.stop.setGranularity(granularity);
      const nextGranularity =
        typeof selected === "string" ? selected : active.stop.granularity();
      state = { ...state, granularity: nextGranularity };
      if (render) onChange(state);
      return nextGranularity;
    },

    toggleBreakpoint(line) {
      const active = requirePause();
      const lines = new Set(state.breakpointLines);
      if (lines.has(line)) {
        lines.delete(line);
      } else {
        lines.add(line);
      }
      const sorted = [...lines].sort((lhs, rhs) => lhs - rhs);
      const breakpoints = active.stop.setSourceBreakpoints(sorted);
      refresh({ breakpointLines: sorted, breakpoints });
    },

    trackSymbol(name) {
      const active = requirePause();
      const result = active.stop.trackSymbol(name.trim());
      refresh();
      return result;
    },

    removeTracker(id) {
      const active = requirePause();
      const removed = active.stop.removeTracker(id);
      refresh();
      return removed;
    },

    clearTrackers() {
      const active = requirePause();
      active.stop.clearTrackers();
      refresh();
    },

    recordNotification(notification) {
      const notifications = [...state.notifications, notification].slice(-50);
      publish({ notifications });
    },
  };
}

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function section(title, { foldable = false, open = true } = {}) {
  if (foldable) {
    const node = element("details", "wqdb-section wqdb-foldable");
    node.dataset.wqdbSection = title;
    node.open = open;
    const summary = element("summary", "wqdb-section-title");
    const chevron = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "svg"
    );
    chevron.classList.add("wqdb-section-chevron");
    chevron.setAttribute("viewBox", "0 0 24 24");
    chevron.setAttribute("aria-hidden", "true");
    const path = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "path"
    );
    path.setAttribute("d", "m6 9 6 6 6-6");
    chevron.append(path);
    summary.append(
      chevron,
      element("span", "wqdb-section-title-label", title)
    );
    node.append(summary);
    return node;
  }
  const node = element("section", "wqdb-section");
  node.append(element("h3", "wqdb-section-title", title));
  return node;
}

function sectionOpen(foldState, title) {
  return foldState.get(title) ?? true;
}

function readFoldState(body) {
  const foldState = new Map();
  for (const node of body.querySelectorAll(
    ".wqdb-foldable[data-wqdb-section]",
  )) {
    foldState.set(node.dataset.wqdbSection, node.open);
  }
  return foldState;
}

function renderValues(title, values, open) {
  const node = section(title, { foldable: true, open });
  if (!values.length) {
    node.append(element("p", "wqdb-empty", "None"));
    return node;
  }
  const list = element("dl", "wqdb-values");
  for (const value of values) {
    const item = element("div", "wqdb-value");
    const head = element("div", "wqdb-value-head");
    head.append(
      element(
        "dt",
        "wqdb-value-name",
        value.name ?? (value.slot === null ? "value" : `slot ${value.slot}`),
      ),
      element("dd", "wqdb-value-kind", value.kind),
    );
    item.append(head, element("dd", "wqdb-value-display", value.display));
    list.append(item);
  }
  node.append(list);
  return node;
}

function renderSource(state, actions, frontend, open) {
  const node = section("Source", { foldable: true, open });
  if (!state.source) {
    node.append(element("p", "wqdb-empty", "Source unavailable"));
    return node;
  }
  const lines = element("div", "wqdb-source");
  const activeLine = state.pause?.location?.line;
  const breakpointLines = new Set(state.breakpointLines);
  const highlightedLines = frontend
    ? highlightedSourceLineFragments(document, frontend, state.source)
    : null;
  state.source.split("\n").forEach((text, index) => {
    const lineNumber = index + 1;
    const row = element(
      "div",
      `wqdb-source-line${activeLine === lineNumber ? " active" : ""}`,
    );
    const breakpoint = element(
      "button",
      `wqdb-breakpoint${breakpointLines.has(lineNumber) ? " active" : ""}`
    );
    breakpoint.type = "button";
    breakpoint.disabled = state.status !== "paused";
    breakpoint.title = `Toggle breakpoint at line ${lineNumber}`;
    breakpoint.setAttribute(
      "aria-label",
      `Toggle breakpoint at line ${lineNumber}`,
    );
    breakpoint.addEventListener("click", () =>
      actions.toggleBreakpoint(lineNumber),
    );
    const code = element("code", "wqdb-source-code");
    const highlightedLine = highlightedLines?.[index];
    if (highlightedLine?.hasChildNodes()) {
      code.append(highlightedLine);
    } else {
      code.textContent = text || " ";
    }
    row.append(
      breakpoint,
      element("span", "wqdb-line-number", String(lineNumber)),
      code,
    );
    lines.append(row);
  });
  node.append(lines);
  return node;
}

function renderStack(state, actions, open) {
  const node = section("Stack", { foldable: true, open });
  if (!state.stack.length) {
    node.append(element("p", "wqdb-empty", "No stack frames"));
    return node;
  }
  const list = element("div", "wqdb-stack");
  state.stack.forEach((frame, index) => {
    const button = element(
      "button",
      `wqdb-frame${index === state.selectedFrame ? " active" : ""}`,
    );
    button.type = "button";
    button.disabled = state.status !== "paused";
    button.append(
      element("strong", "", frame.function),
      element(
        "span",
        "",
        frame.path && frame.line !== null
          ? `${frame.path}:${frame.line}`
          : `chunk ${frame.chunk ?? "?"}, pc ${frame.pc ?? "?"}`,
      ),
    );
    button.addEventListener("click", () => actions.selectFrame(index));
    list.append(button);
  });
  node.append(list);
  return node;
}

function renderInstruction(state, open) {
  const node = section("Instruction", { foldable: true, open });
  if (!state.instruction) {
    node.append(element("p", "wqdb-empty", "Instruction unavailable"));
    return node;
  }
  const instruction = state.instruction;
  const code = element("pre", "wqdb-instruction");
  const line = element("span", "wqdb-instruction-line");
  line.append(
    element(
      "span",
      "wqdb-instruction-pc",
      String(instruction.pc).padStart(4, " "),
    ),
    " ",
    element(
      "span",
      `wqdb-instruction-opcode ${instruction.class}${instruction.is_special ? " special" : ""}`,
      instruction.opcode,
    ),
    element("span", "wqdb-instruction-operands", instruction.operands),
  );
  if (instruction.annotations.length) {
    line.append(
      "  ",
      element(
        "span",
        "wqdb-instruction-annotations",
        `// ${instruction.annotations.join("; ")}`,
      ),
    );
  }
  code.append(line);
  node.append(code);
  return node;
}

function renderTracking(state, actions, open) {
  const node = section("Symbol tracking", { foldable: true, open });
  const form = element("form", "wqdb-track-form");
  const input = element("input", "wqdb-track-input");
  input.name = "symbol";
  input.placeholder = "Global or visible local";
  input.setAttribute("aria-label", "Symbol to track");
  input.disabled = state.status !== "paused";
  const add = element("button", "btn", "Track");
  add.type = "submit";
  add.disabled = state.status !== "paused";
  form.append(input, add);
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    if (!input.value.trim()) return;
    actions.trackSymbol(input.value);
    input.value = "";
  });
  node.append(form);

  if (state.trackers.length) {
    const list = element("ul", "wqdb-trackers");
    for (const tracker of state.trackers) {
      const item = element("li", "wqdb-tracker");
      const target = element("span", "wqdb-tracker-target");
      if (tracker.target.name) {
        target.append(
          `${tracker.target.scope} `,
          element("span", "wqdb-tracker-symbol", tracker.target.name),
        );
      } else {
        target.textContent = targetLabel(tracker.target);
      }
      item.append(target);
      const remove = element("button", "btn", "Remove");
      remove.type = "button";
      remove.disabled = state.status !== "paused";
      remove.addEventListener("click", () => actions.removeTracker(tracker.id));
      item.append(remove);
      list.append(item);
    }
    node.append(list);
  } else {
    node.append(element("p", "wqdb-empty", "No tracked symbols"));
  }
  return node;
}

function renderNotifications(state, open) {
  if (!state.notifications.length) return null;
  const node = section("Changes", { foldable: true, open });
  const list = element("ol", "wqdb-notifications");
  for (const notification of state.notifications.slice().reverse()) {
    const item = element("li", "wqdb-notification");
    item.append(
      element("strong", "", targetLabel(notification.target)),
      element(
        "code",
        "",
        `${notification.operation}: ${notification.old_value?.display ?? "unset"} → ${notification.new_value.display}`,
      ),
    );
    list.append(item);
  }
  node.append(list);
  return node;
}

export function renderWqdbPanel(
  body,
  state,
  actions,
  {
    frontend = null,
    emptyMessage =
      "Enable \\wqdb, then run code to pause before its first instruction.",
  } = {},
) {
  if (!body) return;
  body.classList.toggle("is-empty", !state.pause);
  if (!state.pause) {
    const empty = element("div", "globals-panel-empty");
    empty.append(
      element("strong", "", "Debugger ready"),
      element(
        "span",
        "",
        emptyMessage,
      ),
    );
    body.replaceChildren(empty);
    return;
  }

  const foldState = readFoldState(body);
  const fragment = document.createDocumentFragment();
  const summary = element("section", "wqdb-summary");
  const status = element(
    "span",
    `wqdb-status ${state.status}`,
    state.status === "paused"
      ? "Paused"
      : state.status === "running"
        ? "Running"
        : "Idle",
  );
  summary.append(
    status,
    element("strong", "wqdb-reason", reasonLabel(state.pause.reason)),
    element("span", "wqdb-location", locationLabel(state.pause)),
  );

  const controls = element("div", "wqdb-controls");
  for (const [action, label] of [
    ["continue", "Continue"],
    ["step_over", "Step over"],
    ["step_in", "Step in"],
    ["step_out", "Step out"],
  ]) {
    const button = element(
      "button",
      `btn${action === "continue" ? " primary" : ""}`,
      label,
    );
    button.type = "button";
    button.disabled = state.status !== "paused";
    button.addEventListener("click", () => actions.resume(action));
    controls.append(button);
  }
  const granularity = element("div", "wqdb-granularity");
  granularity.append(element("span", "", "Step by"));
  const granularityOptions = element(
    "div",
    "wqdb-granularity-options segmented-control"
  );
  granularityOptions.setAttribute("role", "group");
  granularityOptions.setAttribute("aria-label", "Step granularity");
  const granularityThumb = element("span", "segmented-control-thumb");
  granularityThumb.setAttribute("aria-hidden", "true");
  granularityOptions.append(granularityThumb);
  for (const [value, label] of [
    ["line", "Line"],
    ["expr", "Expression"],
    ["inst", "Instruction"],
  ]) {
    const option = element(
      "button",
      `wqdb-granularity-option${state.granularity === value ? " active" : ""}`,
      label,
    );
    option.type = "button";
    option.disabled = state.status !== "paused";
    option.dataset.wqdbGranularity = value;
    option.setAttribute(
      "aria-pressed",
      String(state.granularity === value),
    );
    granularityOptions.append(option);
  }
  wireSegmentedControl(granularityOptions, {
    optionSelector: "[data-wqdb-granularity]",
    isSelected: (option) => option.classList.contains("active"),
    onSelect(option) {
      const selected = actions.setGranularity(
        option.dataset.wqdbGranularity,
        { render: false }
      );
      for (const button of granularityOptions.querySelectorAll(
        "[data-wqdb-granularity]"
      )) {
        const active = button.dataset.wqdbGranularity === selected;
        button.classList.toggle("active", active);
        button.setAttribute("aria-pressed", String(active));
      }
    }
  });
  granularity.append(granularityOptions);
  controls.append(granularity);

  fragment.append(
    summary,
    controls,
    renderSource(
      state,
      actions,
      frontend,
      sectionOpen(foldState, "Source"),
    ),
  );
  fragment.append(
    renderStack(state, actions, sectionOpen(foldState, "Stack")),
    renderValues(
      "Locals",
      state.locals,
      sectionOpen(foldState, "Locals"),
    ),
    renderValues(
      "Globals",
      state.globals,
      sectionOpen(foldState, "Globals"),
    ),
    renderInstruction(state, sectionOpen(foldState, "Instruction")),
    renderTracking(
      state,
      actions,
      sectionOpen(foldState, "Symbol tracking"),
    ),
  );
  const notifications = renderNotifications(
    state,
    sectionOpen(foldState, "Changes"),
  );
  if (notifications) fragment.append(notifications);
  body.replaceChildren(fragment);
}
