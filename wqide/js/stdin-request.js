import { abortError } from "./eval-lifecycle.js";

function defaultRender({ prompt, submit, eof }) {
  const row = document.createElement("div");
  row.className = "stdin-request";
  row.setAttribute("role", "group");
  row.setAttribute("aria-label", "Program input request");

  const label = document.createElement("label");
  label.className = "stdin-request-prompt";
  label.textContent = prompt || "stdin:";

  const input = document.createElement("input");
  input.className = "stdin-request-input";
  input.type = "text";
  input.autocomplete = "off";
  input.spellcheck = false;
  input.setAttribute("aria-label", prompt || "stdin input");

  const send = document.createElement("button");
  send.className = "btn stdin-request-send";
  send.type = "button";
  send.textContent = "Send";

  const eofButton = document.createElement("button");
  eofButton.className = "btn stdin-request-eof-button";
  eofButton.type = "button";
  eofButton.textContent = "EOF";

  const actions = document.createElement("span");
  actions.className = "stdin-request-actions";
  actions.append(send, eofButton);
  row.append(label, input, actions);

  const sendLine = () => submit(input.value);
  send.addEventListener("click", sendLine);
  eofButton.addEventListener("click", eof);
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      sendLine();
    } else if (event.key === "d" && event.ctrlKey) {
      event.preventDefault();
      eof();
    }
  });

  return {
    element: row,
    focus() {
      input.focus();
    },
    complete(completion) {
      input.disabled = true;
      send.disabled = true;
      eofButton.disabled = true;
      row.classList.remove("stdin-request-active");
      row.classList.add(`stdin-request-${completion.kind}`);
      if (completion.kind === "line") {
        input.value = completion.value;
        actions.textContent = "Sent";
      } else {
        input.remove();
        actions.textContent = completion.kind === "eof" ? "EOF" : "Interrupted";
      }
    },
  };
}

export function createDomStdinRenderer(mount) {
  return (request) => {
    const view = defaultRender(request);
    mount.appendChild(view.element);
    mount.hidden = false;
    view.element.classList.add("stdin-request-active");
    view.focus();
    return view;
  };
}

export function createStdinRequester({
  render,
  onPendingChange = () => {},
}) {
  let active = null;

  return {
    get pending() {
      return active !== null;
    },
    request(prompt, { signal } = {}) {
      if (active) {
        return Promise.reject(new Error("stdin request is already active"));
      }
      if (signal?.aborted) {
        return Promise.reject(abortError(signal.reason));
      }

      return new Promise((resolve, reject) => {
        let settled = false;
        let view;
        const finish = (completion, result, error) => {
          if (settled) return;
          settled = true;
          signal?.removeEventListener("abort", onAbort);
          view?.complete(completion);
          active = null;
          onPendingChange(false);
          if (error) reject(error);
          else resolve(result);
        };
        const abortPending = (reason) =>
          finish({ kind: "aborted" }, undefined, abortError(reason));
        const onAbort = () => abortPending(signal.reason);
        const controls = {
          prompt: typeof prompt === "string" ? prompt : "",
          submit(value) {
            finish({ kind: "line", value: String(value) }, String(value));
          },
          eof() {
            finish({ kind: "eof" }, null);
          },
        };
        view = render(controls);
        active = { abort: abortPending };
        onPendingChange(true);
        signal?.addEventListener("abort", onAbort, { once: true });
      });
    },
    cancel(reason = "evaluation interrupted") {
      if (!active) return false;
      active.abort(reason);
      return true;
    },
  };
}
