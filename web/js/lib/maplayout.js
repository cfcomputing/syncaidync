/** Pure layout for the change-import map. */

/** A layer taller than this wraps into extra columns, so big maps stay roughly square. */
const MAX_ROWS = 18;

/**
 * @param {{ nodes: { path: string, changed: boolean }[], edges: { from: string, to: string }[] }} map
 */
export function layoutMap(map) {
  const nodes = map?.nodes || [];
  const paths = nodes.map((n) => n.path);
  const pathSet = new Set(paths);
  const incoming = new Map(); // to -> from[]
  const outgoing = new Map(); // from -> to[]
  for (const p of paths) {
    incoming.set(p, []);
    outgoing.set(p, []);
  }
  for (const e of map?.edges || []) {
    if (!pathSet.has(e.from) || !pathSet.has(e.to) || e.from === e.to) continue;
    outgoing.get(e.from).push(e.to);
    incoming.get(e.to).push(e.from);
  }

  // Layer = longest chain of imports below (edges point from importer to imported).
  // Layer 0 = imports nothing in the map.
  const layerOf = new Map();
  const visiting = new Set();

  function layer(path) {
    if (layerOf.has(path)) return layerOf.get(path);
    if (visiting.has(path)) return 0; // cycle: ignore back-edge
    visiting.add(path);
    let best = 0;
    for (const to of outgoing.get(path) || []) {
      best = Math.max(best, layer(to) + 1);
    }
    visiting.delete(path);
    layerOf.set(path, best);
    return best;
  }
  for (const p of paths) layer(p);

  const byLayer = new Map();
  for (const n of nodes) {
    const L = layerOf.get(n.path) || 0;
    if (!byLayer.has(L)) byLayer.set(L, []);
    byLayer.get(L).push(n);
  }

  const layers = [...byLayer.keys()].sort((a, b) => a - b);
  const colW = 180;
  const rowH = 36;
  const pad = 24;
  let maxRows = 0;
  let col = 0;
  const placed = [];
  for (const L of layers) {
    const group = byLayer.get(L);
    group.sort((a, b) => a.path.localeCompare(b.path));
    const cols = Math.ceil(group.length / MAX_ROWS);
    const rows = Math.ceil(group.length / cols);
    maxRows = Math.max(maxRows, rows);
    group.forEach((n, i) => {
      placed.push({
        path: n.path,
        changed: !!n.changed,
        x: pad + (col + Math.floor(i / rows)) * colW,
        y: pad + (i % rows) * rowH,
        layer: L,
      });
    });
    col += cols;
  }
  const width = pad * 2 + Math.max(1, col) * colW;
  const height = pad * 2 + Math.max(1, maxRows) * rowH;
  return {
    nodes: placed,
    edges: (map?.edges || []).filter((e) => pathSet.has(e.from) && pathSet.has(e.to)),
    width,
    height,
  };
}

/** File order by layer (providers / imported first), then path. */
export function providersFirst(files, map) {
  const layout = layoutMap(map || { nodes: [], edges: [] });
  const layer = new Map(layout.nodes.map((n) => [n.path, n.layer]));
  return [...(files || [])].sort((a, b) => {
    const la = layer.get(a.path) ?? 0;
    const lb = layer.get(b.path) ?? 0;
    if (la !== lb) return la - lb;
    return a.path.localeCompare(b.path);
  });
}
