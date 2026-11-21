/**
 * The page backdrop: a faint mesh of random triangles behind everything.
 *
 * Points sit on a grid, each nudged by a random amount, and every cell is split along a random
 * diagonal, so the mesh reads as irregular polygons with no gaps or crossings. A few triangles
 * and corners glint, and short sparks run along the mesh's own crooked lines. The colours, fades
 * and motion are CSS (`.mesh*` in base.css); this file only draws the shapes.
 */

const VIEW_W = 1600;
const VIEW_H = 1000;
const CELL = 96;
const JITTER = 0.38; // of a cell, each way

/** Small seeded PRNG (mulberry32), so a given seed always draws the same mesh. */
export function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const r1 = (n) => Math.round(n * 10) / 10;

/** Twice the signed area of a triangle; positive when its points run clockwise on screen. */
function area2(a, b, c) {
  return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
}
const MIN_AREA2 = CELL * CELL * 0.08;

/**
 * Build the mesh: points in rows and columns (one cell of overhang on every side), the unique
 * edges as one path, the triangles, and the picks for glints and sparks.
 * @param {number} seed
 */
export function buildMesh(seed, { width = VIEW_W, height = VIEW_H, cell = CELL } = {}) {
  const rand = rng(seed);
  const cols = Math.ceil(width / cell) + 2;
  const rows = Math.ceil(height / cell) + 2;
  const pts = [];
  for (let r = 0; r <= rows; r++) {
    const row = [];
    for (let c = 0; c <= cols; c++) {
      const jx = (rand() * 2 - 1) * JITTER * cell;
      const jy = (rand() * 2 - 1) * JITTER * cell;
      row.push([r1((c - 1) * cell + jx), r1((r - 1) * cell + jy)]);
    }
    pts.push(row);
  }

  const lines = [];
  const tris = [];
  const seg = (a, b) => lines.push(`M${a[0]} ${a[1]}L${b[0]} ${b[1]}`);
  for (let r = 0; r <= rows; r++) {
    for (let c = 0; c <= cols; c++) {
      const p = pts[r][c];
      if (c < cols) seg(p, pts[r][c + 1]);
      if (r < rows) seg(p, pts[r + 1][c]);
      if (r < rows && c < cols) {
        const q = pts[r][c + 1];
        const s = pts[r + 1][c];
        const t = pts[r + 1][c + 1];
        // Pick a random diagonal, but never one that leaves a sliver or a folded triangle
        // (possible when the jitter dents the cell); the other diagonal is then always fine.
        const okA = area2(p, q, t) > MIN_AREA2 && area2(p, t, s) > MIN_AREA2;
        const okB = area2(p, q, s) > MIN_AREA2 && area2(q, t, s) > MIN_AREA2;
        const useA = okA && okB ? rand() < 0.5 : okA || !okB ? okA : false;
        if (useA) {
          seg(p, t);
          tris.push([p, q, t], [p, t, s]);
        } else {
          seg(q, s);
          tris.push([p, q, s], [q, t, s]);
        }
      }
    }
  }

  const glintTris = tris.filter(() => rand() < 0.05);
  const glintDots = pts.flat().filter(() => rand() < 0.045);

  // Sparks follow whole rows or columns of points, which are joined by mesh edges.
  const sparks = [];
  const pick = (n, max) => {
    const out = new Set();
    while (out.size < Math.min(n, max)) out.add(1 + Math.floor(rand() * (max - 1)));
    return [...out];
  };
  for (const r of pick(4, rows)) sparks.push(pts[r]);
  for (const c of pick(3, cols)) sparks.push(pts.map((row) => row[c]));

  return { rows, cols, pts, lines, tris, glintTris, glintDots, sparks, rand };
}

function svg(body) {
  return `<svg viewBox="0 0 ${VIEW_W} ${VIEW_H}" preserveAspectRatio="xMidYMid slice" aria-hidden="true">${body}</svg>`;
}

/** SVG markup for each layer: `lines`, `glints`, `sparks`. */
export function meshLayers(seed) {
  const m = buildMesh(seed);
  const { rand } = m;
  const timing = (lo, hi) =>
    `animation-duration:${r1(lo + rand() * (hi - lo))}s;animation-delay:-${r1(rand() * hi)}s`;

  const lines = svg(`<path class="m-lines" d="${m.lines.join("")}"/>`);

  const tris = m.glintTris
    .map(
      (t) =>
        `<polygon class="g-tri" points="${t.map((p) => p.join(",")).join(" ")}" style="${timing(6, 14)}"/>`,
    )
    .join("");
  const dots = m.glintDots
    .map((p) => `<circle class="g-dot" cx="${p[0]}" cy="${p[1]}" r="1.7" style="${timing(5, 12)}"/>`)
    .join("");
  const glints = svg(tris + dots);

  const sparks = svg(
    m.sparks
      .map((line) => {
        const pts = rand() < 0.5 ? line : [...line].reverse();
        const d = `M${pts.map((p) => p.join(" ")).join("L")}`;
        return `<path class="spark" pathLength="1000" d="${d}" style="${timing(36, 70)}"/>`;
      })
      .join(""),
  );

  return { lines, glints, sparks };
}

/** Fill every `[data-mesh]` layer in the page with a freshly seeded mesh. */
export function mountBackdrop(root = document) {
  const layers = meshLayers(Math.floor(Math.random() * 2 ** 31));
  for (const el of root.querySelectorAll("[data-mesh]")) {
    const kind = el.getAttribute("data-mesh");
    if (layers[kind]) el.innerHTML = layers[kind];
  }
}
