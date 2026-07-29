export function cellGroupId(contract) {
  const group = contract?.cellGroup;
  if (typeof group !== "string") return null;
  const normalized = group.trim();
  return normalized || null;
}

export function planCellRuns(cells, areAdjacent = () => true) {
  const runs = [];

  for (const cell of cells) {
    const group = cellGroupId(cell.contract);
    if (!group) {
      runs.push({ id: null, cells: [cell] });
      continue;
    }
    const previousRun = runs.at(-1);
    const previousCell = previousRun?.cells.at(-1);
    if (
      previousRun?.id === group &&
      previousCell &&
      areAdjacent(previousCell, cell)
    ) {
      previousRun.cells.push(cell);
    } else {
      runs.push({ id: group, cells: [cell] });
    }
  }

  return runs;
}

export function cellRunLabel(total) {
  return total < 2 ? "Run" : `Run ${total} cells`;
}

export function hasFinalResult(result) {
  return (
    result?.display !== undefined &&
    result.display !== null &&
    String(result.display).length > 0
  );
}
