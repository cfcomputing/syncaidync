/** Pure helpers for workspace margins. */

/** Group index entries by 1-based start line. */
export function byStartLine(margins) {
  const map = new Map();
  for (const m of margins || []) {
    const line = Math.max(1, Number(m.start) || 1);
    if (!map.has(line)) map.set(line, []);
    map.get(line).push(m);
  }
  return map;
}

/** Quote up to 6 lines from text for a 1-based inclusive range. */
export function quoteLines(text, start, end) {
  const lines = String(text ?? "").split("\n");
  const a = Math.max(1, start | 0);
  const b = Math.max(a, end | 0);
  const slice = lines.slice(a - 1, Math.min(b, a + 5));
  return slice.join("\n");
}

/** Collapse runs of whitespace so indent/wrapping drift still matches. */
export function normalizeQuote(s) {
  return String(s ?? "")
    .replace(/\s+$/g, "")
    .replace(/^\s+/gm, "")
    .replace(/[ \t]+/g, " ")
    .replace(/\s*\n\s*/g, "\n")
    .trim();
}

/**
 * Similarity in [0, 1] from normalized Levenshtein distance.
 * @param {string} a
 * @param {string} b
 */
export function similarity(a, b) {
  const x = normalizeQuote(a);
  const y = normalizeQuote(b);
  if (x === y) return 1;
  if (!x || !y) return 0;
  const maxLen = Math.max(x.length, y.length);
  if (maxLen > 4000) {
    // Cap work on huge windows; fall back to prefix/suffix equality checks.
    if (x.slice(0, 80) === y.slice(0, 80) && x.slice(-80) === y.slice(-80)) {
      return 0.85;
    }
    return x.includes(y) || y.includes(x) ? 0.7 : 0;
  }
  return 1 - levenshtein(x, y) / maxLen;
}

function levenshtein(a, b) {
  if (a === b) return 0;
  if (!a.length) return b.length;
  if (!b.length) return a.length;
  const prev = new Array(b.length + 1);
  const cur = new Array(b.length + 1);
  for (let j = 0; j <= b.length; j++) prev[j] = j;
  for (let i = 1; i <= a.length; i++) {
    cur[0] = i;
    const ca = a.charCodeAt(i - 1);
    for (let j = 1; j <= b.length; j++) {
      const cost = ca === b.charCodeAt(j - 1) ? 0 : 1;
      cur[j] = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + cost);
    }
    for (let j = 0; j <= b.length; j++) prev[j] = cur[j];
  }
  return prev[b.length];
}

/** Minimum similarity to treat a fuzzy hit as the same quote (moved). */
export const FUZZY_MOVE_THRESHOLD = 0.72;

/**
 * Find where a margin's quote sits in `text`.
 * Exact → ok/moved; unique near-match after normalize/fuzzy → moved; else outdated.
 * @returns {{ status: "ok"|"moved"|"outdated", start: number, end: number }}
 */
export function relocateMargin(text, margin) {
  const start = Math.max(1, Number(margin?.start) || 1);
  const end = Math.max(start, Number(margin?.end) || start);
  const quoteRaw = String(margin?.quote ?? "").replace(/\s+$/, "");
  if (!quoteRaw) {
    return { status: "ok", start, end };
  }
  const lines = String(text ?? "").split("\n");
  const qLines = quoteRaw.split("\n");
  const span = Math.max(1, qLines.length);
  const quoteNorm = normalizeQuote(quoteRaw);

  const windowAt = (line1) => {
    if (line1 < 1 || line1 + span - 1 > lines.length) return null;
    return lines.slice(line1 - 1, line1 - 1 + span).join("\n");
  };

  const exactAt = (line1) => {
    const w = windowAt(line1);
    return w != null && w === quoteRaw;
  };

  const normAt = (line1) => {
    const w = windowAt(line1);
    return w != null && normalizeQuote(w) === quoteNorm;
  };

  if (exactAt(start) || normAt(start)) {
    return { status: "ok", start, end: start + span - 1 };
  }

  const exactHits = [];
  const normHits = [];
  for (let i = 0; i < lines.length; i++) {
    const line1 = i + 1;
    if (exactAt(line1)) exactHits.push(line1);
    else if (normAt(line1)) normHits.push(line1);
  }
  if (exactHits.length === 1) {
    const s = exactHits[0];
    return { status: "moved", start: s, end: s + span - 1 };
  }
  if (exactHits.length === 0 && normHits.length === 1) {
    const s = normHits[0];
    return { status: "moved", start: s, end: s + span - 1 };
  }
  if (exactHits.length > 1 || normHits.length > 1) {
    return { status: "outdated", start, end };
  }

  // Fuzzy: strong unique window, prefer nearer the original start on a close call.
  const candidates = [];
  for (let i = 0; i <= lines.length - span; i++) {
    const line1 = i + 1;
    const w = windowAt(line1);
    if (w == null) continue;
    const score = similarity(w, quoteRaw);
    if (score < FUZZY_MOVE_THRESHOLD) continue;
    candidates.push({ line1, score, dist: Math.abs(line1 - start) });
  }
  candidates.sort((a, b) => b.score - a.score || a.dist - b.dist || a.line1 - b.line1);
  if (candidates.length === 1) {
    const s = candidates[0].line1;
    return { status: "moved", start: s, end: s + span - 1 };
  }
  if (candidates.length > 1) {
    const [a, b] = candidates;
    // Clear score winner, or one hit sits near the old lines and the other does not.
    const scoreWinner = a.score - b.score > 0.05;
    const nearWinner = a.dist <= 3 && b.dist >= a.dist + 8 && a.score + 0.02 >= b.score;
    if (scoreWinner || nearWinner) {
      return { status: "moved", start: a.line1, end: a.line1 + span - 1 };
    }
  }
  return { status: "outdated", start, end };
}

/** Apply relocateMargin to a list; returns { margins, moved[] } with updated starts. */
export function relocateAll(text, margins) {
  const moved = [];
  const out = (margins || []).map((m) => {
    const r = relocateMargin(text, m);
    if (r.status === "moved") {
      const next = { ...m, start: r.start, end: r.end, status: "moved" };
      moved.push(next);
      return next;
    }
    return { ...m, status: r.status, start: r.start, end: r.end };
  });
  return { margins: out, moved };
}
