export function bookReplOptions(contract) {
  const repl = contract?.repl;
  if (repl === true) return { debugger: false };
  if (!repl || Array.isArray(repl) || typeof repl !== "object") return null;
  return { debugger: repl.wqdb === true };
}

export function bookReplRoute(source) {
  return `repl.html?input=${encodeURIComponent(String(source))}`;
}

export function bookReplStatusLabel(state) {
  if (state === "queued") return "Queued";
  if (state === "running") return "Running";
  if (state === "awaiting-input") return "Waiting for input";
  if (state === "paused") return "Paused";
  if (state === "stopping") return "Stopping";
  return "Ready";
}
