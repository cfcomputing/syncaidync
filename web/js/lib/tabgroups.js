/** Pure workspace panel / tab-group layout. */

import { clampRect } from "./layout.js";

export const TOOLS = ["terminal", "files", "changes", "session"];

function clamp(n, lo, hi) {
  return Math.min(hi, Math.max(lo, n));
}

function clone(layout) {
  return {
    groups: (layout?.groups || []).map((g) => ({
      id: g.id,
      tabs: [...(g.tabs || [])],
      active: g.active,
      rect: g.rect ? { ...g.rect } : null,
      opacity: g.opacity,
      idleOpacity: g.idleOpacity,
    })),
  };
}

function newId(layout) {
  const used = new Set((layout.groups || []).map((g) => g.id));
  let i = 1;
  while (used.has(`g${i}`)) i += 1;
  return `g${i}`;
}

function findGroup(layout, groupId) {
  return layout.groups.find((g) => g.id === groupId) || null;
}

function findTool(layout, tool) {
  for (const g of layout.groups) {
    const i = g.tabs.indexOf(tool);
    if (i >= 0) return { group: g, index: i };
  }
  return null;
}

/** Drop empty groups and fix active tabs. */
export function tidy(layout) {
  const next = clone(layout);
  next.groups = next.groups.filter((g) => g.tabs.length > 0);
  for (const g of next.groups) {
    if (!g.tabs.includes(g.active)) g.active = g.tabs[0];
    g.opacity = clamp(Number(g.opacity) || 1, 0.4, 1);
    g.idleOpacity = clamp(Number(g.idleOpacity) || 1, 0.2, 1);
    if (g.rect) g.rect = clampRect({ ...g.rect, id: g.id });
  }
  return next;
}

/**
 * Default: Terminal left 58% width; Files/Changes/Session tabs on the right, Changes active.
 */
export function defaultLayout(width, height, { review = false } = {}) {
  const W = Math.max(Number(width) || 1200, 640);
  const H = Math.max(Number(height) || 800, 400);
  const margin = 16;
  const gap = 16;
  const innerW = W - margin * 2;
  const innerH = H - margin * 2;
  const leftW = review ? Math.max(260, Math.round(innerW * 0.33)) : Math.round(innerW * 0.58);
  const rightW = Math.max(320, innerW - leftW - gap);
  return {
    groups: [
      {
        id: "g1",
        tabs: ["terminal"],
        active: "terminal",
        rect: { x: margin, y: margin, w: leftW, h: innerH, z: 1 },
        opacity: 1,
        idleOpacity: 1,
      },
      {
        id: "g2",
        tabs: ["files", "changes", "session"],
        active: "changes",
        rect: { x: margin + leftW + gap, y: margin, w: rightW, h: innerH, z: 2 },
        opacity: 1,
        idleOpacity: 1,
      },
    ],
  };
}

export function moveTab(layout, tool, toGroupId, index) {
  const next = clone(layout);
  const found = findTool(next, tool);
  if (!found) return tidy(next);
  const from = found.group;
  from.tabs.splice(found.index, 1);
  if (from.active === tool) from.active = from.tabs[0] || null;

  let to = findGroup(next, toGroupId);
  if (!to) {
    // Orphan: put back
    from.tabs.splice(found.index, 0, tool);
    from.active = from.active || tool;
    return tidy(next);
  }

  let at = Math.max(0, Math.min(index ?? to.tabs.length, to.tabs.length));
  // Same group: adjust index after removal
  if (to.id === from.id && found.index < at) at -= 1;
  to.tabs.splice(at, 0, tool);
  to.active = tool;
  return tidy(next);
}

export function tearOut(layout, tool, rect) {
  const next = clone(layout);
  const found = findTool(next, tool);
  if (!found) return tidy(next);
  const from = found.group;
  if (from.tabs.length === 1) {
    // Just move this group's rect
    from.rect = clampRect({
      ...(rect || from.rect || { x: 24, y: 24, w: 520, h: 360, z: 1 }),
      id: from.id,
    });
    from.active = tool;
    // Raise
    const maxZ = next.groups.reduce((m, g) => Math.max(m, g.rect?.z || 1), 0);
    from.rect.z = maxZ + 1;
    return tidy(next);
  }
  from.tabs.splice(found.index, 1);
  if (from.active === tool) from.active = from.tabs[0];
  const maxZ = next.groups.reduce((m, g) => Math.max(m, g.rect?.z || 1), 0);
  const id = newId(next);
  next.groups.push({
    id,
    tabs: [tool],
    active: tool,
    rect: clampRect({
      x: rect?.x ?? 40,
      y: rect?.y ?? 40,
      w: rect?.w ?? 520,
      h: rect?.h ?? 360,
      z: maxZ + 1,
      id,
    }),
    opacity: 1,
    idleOpacity: 1,
  });
  return tidy(next);
}

export function setActive(layout, groupId, tool) {
  const next = clone(layout);
  const g = findGroup(next, groupId);
  if (g && g.tabs.includes(tool)) g.active = tool;
  return next;
}

export function setRect(layout, groupId, rect) {
  const next = clone(layout);
  const g = findGroup(next, groupId);
  if (g) g.rect = clampRect({ ...g.rect, ...rect, id: groupId });
  return next;
}

/**
 * `opacity` applies while the panel is active (or none is); `idleOpacity` while another panel
 * is active. Pass `undefined` to leave either as it is.
 */
export function setOpacity(layout, groupId, opacity, idleOpacity) {
  const next = clone(layout);
  const g = findGroup(next, groupId);
  if (g && opacity !== undefined) g.opacity = clamp(Number(opacity) || 1, 0.4, 1);
  if (g && idleOpacity !== undefined) g.idleOpacity = clamp(Number(idleOpacity) || 1, 0.2, 1);
  return next;
}

/**
 * Every tool appears exactly once; unknown tools dropped; missing tools added to the last
 * group; rectangles clamped. Junk input → default-ish layout.
 */
export function normalize(layout, width = 1200, height = 800) {
  if (!layout || typeof layout !== "object" || !Array.isArray(layout.groups)) {
    return defaultLayout(width, height);
  }
  const next = { groups: [] };
  const seen = new Set();

  for (const raw of layout.groups) {
    if (!raw || typeof raw !== "object") continue;
    const tabs = [];
    for (const t of raw.tabs || []) {
      if (!TOOLS.includes(t) || seen.has(t)) continue;
      seen.add(t);
      tabs.push(t);
    }
    if (!tabs.length) continue;
    const id = typeof raw.id === "string" && raw.id ? raw.id : newId(next);
    const active = tabs.includes(raw.active) ? raw.active : tabs[0];
    const rect = clampRect({
      x: raw.rect?.x ?? 16,
      y: raw.rect?.y ?? 16,
      w: raw.rect?.w ?? 520,
      h: raw.rect?.h ?? 360,
      z: raw.rect?.z ?? next.groups.length + 1,
      id,
    });
    next.groups.push({
      id,
      tabs,
      active,
      rect: { x: rect.x, y: rect.y, w: rect.w, h: rect.h, z: rect.z },
      opacity: clamp(Number(raw.opacity) || 1, 0.4, 1),
      idleOpacity: clamp(Number(raw.idleOpacity) || 1, 0.2, 1),
    });
  }

  const missing = TOOLS.filter((t) => !seen.has(t));
  if (missing.length) {
    if (!next.groups.length) return defaultLayout(width, height);
    const last = next.groups[next.groups.length - 1];
    last.tabs.push(...missing);
    if (!last.tabs.includes(last.active)) last.active = last.tabs[0];
  }

  if (!next.groups.length) return defaultLayout(width, height);
  return tidy(next);
}
