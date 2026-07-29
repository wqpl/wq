export function nextTabIndex(key, currentIndex, count) {
  if (count < 1) return null;
  if (key === "Home") return 0;
  if (key === "End") return count - 1;
  if (key === "ArrowRight") return (currentIndex + 1) % count;
  if (key === "ArrowLeft") return (currentIndex - 1 + count) % count;
  return null;
}

export function nextPopupIndex(key, currentIndex, count) {
  if (count < 1) return null;
  if (key === "Home") return 0;
  if (key === "End") return count - 1;
  if (key === "ArrowDown") return (currentIndex + 1) % count;
  if (key === "ArrowUp") return (currentIndex - 1 + count) % count;
  return null;
}
