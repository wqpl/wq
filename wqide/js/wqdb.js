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

    setGranularity(granularity) {
      const active = requirePause();
      active.stop.setGranularity(granularity);
      refresh();
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

function section(title) {
  const node = element("section", "wqdb-section");
  node.append(element("h3", "wqdb-section-title", title));
  return node;
}

function renderValues(title, values) {
  const node = section(title);
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

function renderSource(state, actions) {
  const node = section("Source");
  if (!state.source) {
    node.append(element("p", "wqdb-empty", "Source unavailable"));
    return node;
  }
  const lines = element("div", "wqdb-source");
  const activeLine = state.pause?.location?.line;
  const breakpointLines = new Set(state.breakpointLines);
  state.source.split("\n").forEach((text, index) => {
    const lineNumber = index + 1;
    const row = element(
      "div",
      `wqdb-source-line${activeLine === lineNumber ? " active" : ""}`,
    );
    const breakpoint = element(
      "button",
      `wqdb-breakpoint${breakpointLines.has(lineNumber) ? " active" : ""}`,
      breakpointLines.has(lineNumber) ? "●" : "○",
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
    row.append(
      breakpoint,
      element("span", "wqdb-line-number", String(lineNumber)),
      element("code", "wqdb-source-code", text || " "),
    );
    lines.append(row);
  });
  node.append(lines);
  return node;
}

function renderStack(state, actions) {
  const node = section("Stack");
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

function renderInstruction(state) {
  const node = section("Instruction");
  if (!state.instruction) {
    node.append(element("p", "wqdb-empty", "Instruction unavailable"));
    return node;
  }
  const instruction = state.instruction;
  const code = [
    `${instruction.pc}: ${instruction.opcode}${instruction.operands ? ` ${instruction.operands}` : ""}`,
    ...instruction.annotations.map((annotation) => `  ${annotation}`),
  ].join("\n");
  node.append(element("pre", "wqdb-instruction", code));
  return node;
}

function renderTracking(state, actions) {
  const node = section("Symbol tracking");
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
      item.append(element("span", "", targetLabel(tracker.target)));
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

function renderNotifications(state) {
  if (!state.notifications.length) return null;
  const node = section("Changes");
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

export function renderWqdbPanel(body, state, actions) {
  if (!body) return;
  if (!state.pause) {
    const empty = element("div", "globals-panel-empty");
    empty.append(
      element("strong", "", "Debugger ready"),
      element(
        "span",
        "",
        "Enable \\wqdb, then run code to pause before its first instruction.",
      ),
    );
    body.replaceChildren(empty);
    return;
  }

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
  const granularityOptions = element("div", "wqdb-granularity-options");
  granularityOptions.setAttribute("role", "group");
  granularityOptions.setAttribute("aria-label", "Step granularity");
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
    option.setAttribute(
      "aria-pressed",
      String(state.granularity === value),
    );
    option.addEventListener("click", () => actions.setGranularity(value));
    granularityOptions.append(option);
  }
  granularity.append(granularityOptions);
  controls.append(granularity);

  fragment.append(summary, controls, renderSource(state, actions));
  fragment.append(
    renderStack(state, actions),
    renderValues("Locals", state.locals),
    renderValues("Globals", state.globals),
    renderInstruction(state),
    renderTracking(state, actions),
  );
  const notifications = renderNotifications(state);
  if (notifications) fragment.append(notifications);
  body.replaceChildren(fragment);
}
