export function wireSegmentedControl(
  control,
  {
    optionSelector = "button",
    isSelected = (option) =>
      option.classList.contains("active") ||
      option.getAttribute("aria-selected") === "true" ||
      option.getAttribute("aria-pressed") === "true",
    onSelect
  } = {}
) {
  if (!control) return null;

  let pointerId = null;
  let pendingOption = null;
  let suppressPointerClick = false;

  function options() {
    return Array.from(control.querySelectorAll(optionSelector));
  }

  function selectedOption() {
    const items = options();
    return items.find(isSelected) || items[0];
  }

  function ensureContrastLabels(items) {
    let labelWindow = control.querySelector(
      ":scope > .segmented-control-label-window"
    );
    if (!labelWindow) {
      labelWindow = document.createElement("span");
      labelWindow.className = "segmented-control-label-window";
      labelWindow.setAttribute("aria-hidden", "true");
      for (const [index, option] of items.entries()) {
        const label = document.createElement("span");
        label.className = "segmented-control-label";
        label.style.setProperty("--segment-index", String(index));
        label.textContent = option.textContent.trim();
        labelWindow.append(label);
      }
      control.append(labelWindow);
      return;
    }
    for (const [index, option] of items.entries()) {
      const label = labelWindow.children[index];
      if (!label) continue;
      label.style.setProperty("--segment-index", String(index));
      label.textContent = option.textContent.trim();
    }
  }

  function sync(selected = selectedOption()) {
    const items = options();
    const index = Math.max(0, items.indexOf(selected));
    ensureContrastLabels(items);
    control.style.setProperty(
      "--segment-count",
      String(Math.max(1, items.length))
    );
    control.style.setProperty("--segment-position", String(index));
  }

  function preview(clientX) {
    const items = options();
    const rect = control.getBoundingClientRect();
    if (!items.length || rect.width <= 0) return null;
    const segmentWidth = rect.width / items.length;
    const position = Math.max(
      0,
      Math.min(
        items.length - 1,
        (clientX - rect.left - segmentWidth / 2) / segmentWidth
      )
    );
    control.style.setProperty("--segment-count", String(items.length));
    control.style.setProperty("--segment-position", String(position));
    return items[Math.round(position)];
  }

  function activate(option) {
    if (!option || option.disabled) {
      sync();
      return;
    }
    onSelect?.(option);
    sync(option);
  }

  control.addEventListener("pointerdown", (event) => {
    if (!event.isPrimary || event.button !== 0) return;
    if (!options().some((option) => !option.disabled)) return;
    pointerId = event.pointerId;
    suppressPointerClick = true;
    control.classList.add("is-dragging");
    control.setPointerCapture(pointerId);
    pendingOption = preview(event.clientX);
  });

  control.addEventListener("pointermove", (event) => {
    if (event.pointerId !== pointerId) return;
    pendingOption = preview(event.clientX);
  });

  control.addEventListener("pointerup", (event) => {
    if (event.pointerId !== pointerId) return;
    pendingOption = preview(event.clientX);
    control.releasePointerCapture(pointerId);
    pointerId = null;
    control.classList.remove("is-dragging");
    activate(pendingOption);
    pendingOption = null;
  });

  control.addEventListener("pointercancel", (event) => {
    if (event.pointerId !== pointerId) return;
    if (control.hasPointerCapture(pointerId)) {
      control.releasePointerCapture(pointerId);
    }
    pointerId = null;
    pendingOption = null;
    suppressPointerClick = false;
    control.classList.remove("is-dragging");
    sync();
  });

  control.addEventListener("click", (event) => {
    const option = event.target.closest(optionSelector);
    if (!option || !control.contains(option)) return;
    if (event.detail > 0 && suppressPointerClick) {
      suppressPointerClick = false;
      event.preventDefault();
      return;
    }
    activate(option);
  });

  sync();
  return { sync };
}
