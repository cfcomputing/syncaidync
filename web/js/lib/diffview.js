/** Pure helpers for rendering unified and split diffs. */

/**
 * @param {{ lines: { kind: string, old?: number|null, new?: number|null, text: string }[] }} hunk
 * @returns {{ kind: string, old: number|null, new: number|null, text: string, sign: string }[]}
 */
export function unifiedRows(hunk) {
  const lines = hunk?.lines || [];
  return lines.map((line) => {
    const kind = line.kind || "context";
    const sign = kind === "add" ? "+" : kind === "remove" ? "−" : " ";
    return {
      kind,
      old: line.old ?? null,
      new: line.new ?? null,
      text: line.text ?? "",
      sign,
    };
  });
}

/**
 * Pair remove/add runs side by side. Context spans both columns.
 * @returns {{ kind: "context"|"change", left?: object|null, right?: object|null }[]}
 */
export function splitRows(hunk) {
  const lines = hunk?.lines || [];
  const out = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.kind === "context") {
      out.push({
        kind: "context",
        left: row(line),
        right: row(line),
      });
      i += 1;
      continue;
    }
    const removes = [];
    const adds = [];
    while (i < lines.length && lines[i].kind === "remove") {
      removes.push(lines[i]);
      i += 1;
    }
    while (i < lines.length && lines[i].kind === "add") {
      adds.push(lines[i]);
      i += 1;
    }
    const n = Math.max(removes.length, adds.length);
    for (let k = 0; k < n; k += 1) {
      out.push({
        kind: "change",
        left: removes[k] ? row(removes[k]) : null,
        right: adds[k] ? row(adds[k]) : null,
      });
    }
  }
  return out;
}

function row(line) {
  const kind = line.kind || "context";
  const sign = kind === "add" ? "+" : kind === "remove" ? "−" : " ";
  return {
    kind,
    old: line.old ?? null,
    new: line.new ?? null,
    text: line.text ?? "",
    sign,
  };
}

/** Stable key for a line number on a side (`old` | `new`). */
export function lineKey(side, number) {
  return `${side}:${number}`;
}
