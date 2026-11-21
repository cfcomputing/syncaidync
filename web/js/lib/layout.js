/** Pure board layout math. Gap 16px; strip height 44px. */

import { queueSessions } from "./queue.js";

const GAP = 16;
const STRIP_H = 44;
const TOPBAR = 56;

function clamp(n, lo, hi) {
  return Math.min(hi, Math.max(lo, n));
}

function asList(sessions) {
  if (Array.isArray(sessions)) return sessions.slice();
  if (sessions instanceof Map) return [...sessions.values()];
  return Object.values(sessions || {});
}

/**
 * Order: Your-move queue, then working, launching, ended; snoozed last.
 * Ties by created_at ascending.
 */
export function orderSessions(sessions) {
  const list = asList(sessions);
  const snoozed = list.filter((s) => s.snoozed);
  const active = list.filter((s) => !s.snoozed);
  const queueIds = new Set(queueSessions(active).map((s) => s.id));
  const queue = queueSessions(active);
  const rest = active
    .filter((s) => !queueIds.has(s.id))
    .sort((a, b) => {
      const rank = (s) =>
        s.state === "working" ? 0 : s.state === "launching" ? 1 : s.state === "ended" ? 2 : 3;
      const r = rank(a) - rank(b);
      if (r !== 0) return r;
      return (a.created_at ?? 0) - (b.created_at ?? 0);
    });
  snoozed.sort((a, b) => (a.created_at ?? 0) - (b.created_at ?? 0));
  return [...queue, ...rest, ...snoozed];
}

/**
 * Where cards sit on the board: oldest first, snoozed last. State changes do not move a card,
 * so typing into one session never reshuffles the others. Keyboard focus still uses
 * `orderSessions` (queue first).
 */
export function boardOrder(sessions) {
  const list = asList(sessions);
  const byAge = (a, b) => (a.created_at ?? 0) - (b.created_at ?? 0);
  const active = list.filter((s) => !s.snoozed).sort(byAge);
  const snoozed = list.filter((s) => s.snoozed).sort(byAge);
  return [...active, ...snoozed];
}

function isStrip(session, collapsedIds) {
  if (!session) return false;
  if (session.snoozed) return true;
  if (collapsedIds instanceof Set) return collapsedIds.has(session.id);
  if (Array.isArray(collapsedIds)) return collapsedIds.includes(session.id);
  return !!collapsedIds?.[session.id];
}

function itemHeight(session, collapsedIds, cardH) {
  return isStrip(session, collapsedIds) ? STRIP_H : cardH;
}

/**
 * @param {{ width: number, viewportHeight: number, sessions: any[], collapsed?: string[]|Set }} input
 */
export function rowsLayout(input) {
  const width = Math.max(0, Math.floor(input.width || 0));
  const vh = Math.max(0, Math.floor(input.viewportHeight || 0));
  const collapsed = input.collapsed || [];
  const ordered = boardOrder(input.sessions || []);
  const cardH = clamp(Math.round(vh * 0.42), 260, 380);
  const items = [];
  let y = 0;
  for (const s of ordered) {
    const collapsedFlag = isStrip(s, collapsed);
    const h = collapsedFlag ? STRIP_H : cardH;
    items.push({
      id: s.id,
      x: 0,
      y,
      w: width,
      h,
      z: 1,
      collapsed: collapsedFlag,
    });
    y += h + GAP;
  }
  return {
    items,
    canvas: { w: width, h: Math.max(y - GAP, 0) },
  };
}

/**
 * @param {{ width: number, viewportHeight: number, sessions: any[], collapsed?: string[]|Set }} input
 */
export function gridLayout(input) {
  const W = Math.max(0, Math.floor(input.width || 0));
  const vh = Math.max(0, Math.floor(input.viewportHeight || 0));
  const collapsed = input.collapsed || [];
  const ordered = boardOrder(input.sessions || []);
  const n = ordered.length;
  if (n === 0) return { items: [], canvas: { w: W, h: 0 } };

  let columns = clamp(Math.floor((W + GAP) / (440 + GAP)), 1, n);
  let colW = (W - GAP * (columns - 1)) / columns;
  colW = clamp(colW, 320, 960);
  // Recompute if clamp changed usable columns
  const maxColsByWidth = Math.max(1, Math.floor((W + GAP) / (320 + GAP)));
  columns = clamp(columns, 1, Math.min(n, maxColsByWidth));
  colW = (W - GAP * (columns - 1)) / columns;
  colW = clamp(colW, 320, 960);
  const used = colW * columns + GAP * (columns - 1);
  const offsetX = Math.max(0, Math.floor((W - used) / 2));

  const cardH = clamp(Math.round((vh - TOPBAR - 3 * GAP) / 2), 300, 640);
  const colHeights = Array(columns).fill(0);
  const items = [];

  for (const s of ordered) {
    let col = 0;
    for (let i = 1; i < columns; i++) {
      if (colHeights[i] < colHeights[col]) col = i;
    }
    const collapsedFlag = isStrip(s, collapsed);
    const h = collapsedFlag ? STRIP_H : cardH;
    const x = Math.round(offsetX + col * (colW + GAP));
    const y = colHeights[col];
    items.push({
      id: s.id,
      x,
      y,
      w: Math.round(colW),
      h,
      z: 1,
      collapsed: collapsedFlag,
    });
    colHeights[col] = y + h + GAP;
  }

  const canvasH = Math.max(0, Math.max(...colHeights) - GAP);
  return { items, canvas: { w: W, h: canvasH } };
}

export function clampRect(rect) {
  const x = clamp(Math.round(rect.x ?? 0), 0, 5000);
  const y = clamp(Math.round(rect.y ?? 0), 0, 5000);
  const w = clamp(Math.round(rect.w ?? 560), 320, 3000);
  const h = clamp(Math.round(rect.h ?? 380), 240, 3000);
  const z = Math.round(rect.z ?? 1);
  return {
    id: rect.id,
    x,
    y,
    w,
    h,
    z,
    collapsed: !!rect.collapsed,
    savedH: rect.savedH,
  };
}

export function nextFreeSlot(rects, width) {
  const W = Math.max(width || 0, 1200);
  const size = { w: 560, h: 380 };
  let x = 24;
  let y = 24;
  const occupied = (rects || []).map(clampRect);
  const hits = (nx, ny) =>
    occupied.some(
      (r) =>
        nx < r.x + r.w && nx + size.w > r.x && ny < r.y + r.h && ny + size.h > r.y,
    );
  // Walk the same sequence as freeLayout placement
  for (let guard = 0; guard < 500; guard++) {
    if (!hits(x, y)) return { x, y, w: size.w, h: size.h, z: 1 };
    x += size.w + GAP;
    if (x + size.w > W) {
      x = 24;
      y += size.h + GAP;
    }
  }
  return { x: 24, y: 24, w: size.w, h: size.h, z: 1 };
}

export function raise(rects, id) {
  const list = (rects || []).map((r) => ({ ...r }));
  const maxZ = list.reduce((m, r) => Math.max(m, r.z || 1), 0);
  const target = list.find((r) => r.id === id);
  if (target) target.z = maxZ + 1;
  if (list.some((r) => (r.z || 0) > 10000)) {
    list.sort((a, b) => (a.z || 0) - (b.z || 0));
    list.forEach((r, i) => {
      r.z = i + 1;
    });
  }
  return list;
}

/**
 * @param {{ width: number, viewportHeight: number, sessions: any[], collapsed?: any, free?: Record<string, {x,y,w,h,z,savedH?}> }} input
 */
export function freeLayout(input) {
  const W = Math.max(0, Math.floor(input.width || 0));
  const collapsed = input.collapsed || [];
  const free = input.free || {};
  const ordered = boardOrder(input.sessions || []);
  const placed = [];
  const items = [];

  for (const s of ordered) {
    const collapsedFlag = isStrip(s, collapsed);
    let rect = free[s.id] ? { ...free[s.id], id: s.id } : null;
    if (!rect) {
      const slot = nextFreeSlot(placed, Math.max(W, 1200));
      rect = { id: s.id, ...slot, z: placed.length + 1 };
    }
    rect = clampRect(rect);
    const savedH = rect.savedH ?? rect.h;
    if (collapsedFlag) {
      rect.savedH = savedH;
      rect.h = STRIP_H;
      rect.collapsed = true;
    } else {
      rect.h = clamp(Math.round(savedH), 240, 3000);
      rect.savedH = rect.h;
      rect.collapsed = false;
    }
    placed.push(rect);
    items.push(rect);
  }

  let maxX = 0;
  let maxY = 0;
  for (const r of items) {
    maxX = Math.max(maxX, r.x + r.w);
    maxY = Math.max(maxY, r.y + r.h);
  }
  return {
    items,
    canvas: { w: maxX + 400, h: maxY + 400 },
  };
}

/** Tidy: replace free rects with grid result at current width. */
export function tidy(input) {
  const grid = gridLayout(input);
  const free = {};
  for (const item of grid.items) {
    free[item.id] = {
      x: item.x,
      y: item.y,
      w: item.w,
      h: item.collapsed ? 380 : item.h,
      z: item.z,
      savedH: item.collapsed ? 380 : item.h,
    };
  }
  return freeLayout({ ...input, free });
}

export { GAP, STRIP_H };
