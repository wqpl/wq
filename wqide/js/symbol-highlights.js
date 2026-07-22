function utf8ByteLength(ch) {
  const codePoint = ch.codePointAt(0) || 0;
  if (codePoint <= 0x7f) return 1;
  if (codePoint <= 0x7ff) return 2;
  if (codePoint <= 0xffff) return 3;
  return 4;
}

function floorPoint(points, key, offset) {
  const target = Math.max(0, Number(offset) || 0);
  let lo = 0;
  let hi = points.length - 1;
  while (lo <= hi) {
    const mid = Math.floor((lo + hi) / 2);
    const point = points[mid];
    if (point[key] === target) return point;
    if (point[key] < target) {
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return points[Math.max(0, Math.min(hi, points.length - 1))];
}

export function createSourceMapper(src) {
  const source = String(src);
  const points = [{ byte: 0, unit: 0 }];
  let byte = 0;
  let unit = 0;
  for (const ch of source) {
    byte += utf8ByteLength(ch);
    unit += ch.length;
    points.push({ byte, unit });
  }

  function unitAtByte(byteOffset) {
    return floorPoint(points, "byte", byteOffset).unit;
  }

  function byteAtUnit(unitOffset) {
    return floorPoint(points, "unit", unitOffset).byte;
  }

  function lineCol(byteOffset) {
    const offset = unitAtByte(byteOffset);
    const before = source.slice(0, offset);
    const line = before.split("\n").length;
    const lastNewline = before.lastIndexOf("\n");
    const col = lastNewline === -1 ? offset + 1 : offset - lastNewline;
    return { line, col };
  }

  return {
    byteAtUnit,
    lineCol,
    unitAtByte,
    unitRange(span) {
      return [unitAtByte(span[0]), unitAtByte(span[1])];
    },
  };
}

function spanContains(span, byteOffset) {
  return (
    Array.isArray(span) &&
    span.length === 2 &&
    span[0] <= byteOffset &&
    byteOffset < span[1]
  );
}

function occurrenceRole(kind) {
  return String(kind).endsWith("write") ? "write" : "read";
}

export function activeBindingHighlights(analysis, byteOffset) {
  const occurrences = Array.isArray(analysis?.occurrences)
    ? analysis.occurrences
    : [];
  const current =
    occurrences.find((occurrence) =>
      spanContains(occurrence.span, byteOffset),
    ) ||
    occurrences.find(
      (occurrence) =>
        Array.isArray(occurrence.span) && occurrence.span[1] === byteOffset,
    );
  if (!current) return [];

  return occurrences
    .filter((occurrence) => occurrence.def === current.def)
    .map((occurrence) => ({
      span: occurrence.span,
      role: occurrenceRole(occurrence.kind),
      current: occurrence === current,
    }));
}
