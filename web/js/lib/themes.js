/** Accent themes from uiGradients + contrast helpers. */

import * as prefs from "./prefs.js";

const INK = "#0b0716";
const PAPER = "#ffffff";
const AA = 4.5;

const DEFAULT_NIGHT = { name: "Default", accent: "#a78bfa", accent2: "#818cf8" };
const DEFAULT_DAY = { name: "Default", accent: "#6d4fe0", accent2: "#4f5fe8" };

/** @type {{ name: string, accent: string, accent2: string }[] | null} */
let catalog = null;

export function parseHex(input) {
  if (typeof input !== "string") return null;
  const raw = input.trim();
  if (!raw.startsWith("#")) return null;
  let hex = raw.slice(1);
  if (hex.length === 3) hex = hex.split("").map((c) => c + c).join("");
  if (hex.length !== 6 || /[^0-9a-f]/i.test(hex)) return null;
  const n = Number.parseInt(hex, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function channel(c) {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

export function luminance(hex) {
  const rgb = parseHex(hex);
  if (!rgb) return null;
  return 0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2]);
}

export function contrast(a, b) {
  const la = luminance(a);
  const lb = luminance(b);
  if (la == null || lb == null) return 0;
  const [hi, lo] = la > lb ? [la, lb] : [lb, la];
  return (hi + 0.05) / (lo + 0.05);
}

function toHex(rgb) {
  return `#${rgb.map((v) => Math.round(v).toString(16).padStart(2, "0")).join("")}`;
}

export function mix(from, to, t) {
  const a = parseHex(from);
  const b = parseHex(to);
  if (!a || !b) return from;
  return toHex([
    a[0] + (b[0] - a[0]) * t,
    a[1] + (b[1] - a[1]) * t,
    a[2] + (b[2] - a[2]) * t,
  ]);
}

export function pickOnAccent(accent) {
  if (!parseHex(accent)) return INK;
  return contrast(accent, INK) >= contrast(accent, PAPER) ? INK : PAPER;
}

/**
 * The accent as a button fill plus the ink drawn on it. A few mid-tone accents miss 4.5:1
 * with both inks; those are nudged away from the ink (2% steps) until the label clears it.
 */
export function accentFill(accent) {
  const on = pickOnAccent(accent);
  if (!parseHex(accent)) return { fill: accent, on };
  const away = on === PAPER ? "#000000" : "#ffffff";
  let fill = accent;
  for (let i = 1; i <= 25 && contrast(fill, on) < AA; i++) fill = mix(accent, away, i / 50);
  return { fill, on };
}

/**
 * Mix accent toward --text until it clears 4.5:1 on every given background (max 10 steps).
 * `bg` is one colour or a list (the page and its surfaces, since accent text sits on all of them).
 */
export function readableInk(accent, bg, text = "#e8e8ed") {
  const bgs = (Array.isArray(bg) ? bg : [bg]).filter((b) => parseHex(b));
  if (!parseHex(accent) || bgs.length === 0) return accent;
  const clears = (c) => bgs.every((b) => contrast(c, b) >= AA);
  if (clears(accent)) return accent;
  let chosen = accent;
  for (let i = 1; i <= 10; i++) {
    chosen = mix(accent, text, i / 10);
    if (clears(chosen)) return chosen;
  }
  return chosen;
}

function loadCatalogSync() {
  if (catalog) return catalog;
  // Default only until async load finishes.
  catalog = [DEFAULT_NIGHT];
  return catalog;
}

export async function loadThemes() {
  if (catalog && catalog.length > 1) return catalog;
  const list = [DEFAULT_NIGHT];
  try {
    const res = await fetch("/vendor/uigradients/gradients.json", { credentials: "same-origin" });
    if (res.ok) {
      const raw = await res.json();
      for (const g of raw) {
        if (!g?.name || !Array.isArray(g.colors) || g.colors.length < 2) continue;
        const accent = String(g.colors[0]);
        const accent2 = String(g.colors[g.colors.length - 1]);
        if (!parseHex(accent) || !parseHex(accent2)) continue;
        list.push({ name: g.name, accent, accent2 });
      }
    }
  } catch {
    /* keep Default */
  }
  catalog = list;
  return catalog;
}

export function listThemes() {
  return loadCatalogSync().slice();
}

export function currentName() {
  return prefs.get("theme", "Default");
}

export function cycleEnabled() {
  return prefs.get("theme-cycle", true) !== false;
}

export function setCycleEnabled(on) {
  prefs.set("theme-cycle", !!on);
  window.dispatchEvent(new CustomEvent("dync-theme-cycle", { detail: { on: !!on } }));
}

function appearanceIsDay() {
  return document.documentElement.getAttribute("data-appearance") === "day";
}

function cssVar(name, fallback) {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v || fallback;
}

export function applyTheme(name, { animate = true } = {}) {
  const themes = loadCatalogSync();
  const day = appearanceIsDay();
  let theme = themes.find((t) => t.name === name);
  if (!theme) {
    theme = day ? DEFAULT_DAY : DEFAULT_NIGHT;
    name = "Default";
  } else if (name === "Default") {
    theme = day ? DEFAULT_DAY : DEFAULT_NIGHT;
  }

  const root = document.documentElement;
  if (animate) {
    root.classList.add("theme-easing");
    clearTimeout(applyTheme._t);
    applyTheme._t = setTimeout(() => root.classList.remove("theme-easing"), 520);
  }

  const bg = cssVar("--bg", day ? "#f6f5fb" : "#06060b");
  const surface = cssVar("--surface", day ? "#ffffff" : "#101018");
  const surface2 = cssVar("--surface-2", day ? "#efedf7" : "#181822");
  const text = cssVar("--text", day ? "#17152a" : "#e8e8ed");
  const { fill, on: onAccent } = accentFill(theme.accent);
  const ink = readableInk(theme.accent, [bg, surface, surface2], text);

  root.style.setProperty("--accent", fill);
  root.style.setProperty("--accent-2", theme.accent2);
  root.style.setProperty("--accent-ink", ink);
  root.style.setProperty("--on-accent", onAccent);

  prefs.set("theme", name);
  window.dispatchEvent(
    new CustomEvent("dync-theme", { detail: { name, accent: theme.accent, accent2: theme.accent2 } }),
  );
  return theme;
}

export function nextTheme(delta = 1) {
  const themes = usableThemes();
  if (themes.length === 0) return applyTheme("Default");
  const cur = currentName();
  let i = themes.findIndex((t) => t.name === cur);
  if (i < 0) i = 0;
  const next = themes[(i + delta + themes.length) % themes.length];
  return applyTheme(next.name);
}

/** Skip themes whose first colour is within 1.3:1 of --bg. */
export function usableThemes() {
  const bg = cssVar("--bg", "#06060b");
  return loadCatalogSync().filter((t) => {
    if (t.name === "Default") return true;
    return contrast(t.accent, bg) >= 1.3;
  });
}

let cycleTimer = null;
let cyclePausedUntil = 0;

export function pauseCycle(ms = 45_000) {
  cyclePausedUntil = Date.now() + ms;
}

export function startCycle(intervalMs = 18_000) {
  stopCycle();
  if (typeof window === "undefined") return;
  if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;
  cycleTimer = setInterval(() => {
    if (!cycleEnabled()) return;
    if (Date.now() < cyclePausedUntil) return;
    if (document.hidden) return;
    nextTheme(1);
  }, intervalMs);
}

export function stopCycle() {
  if (cycleTimer != null) {
    clearInterval(cycleTimer);
    cycleTimer = null;
  }
}

export async function initThemes() {
  await loadThemes();
  applyTheme(currentName(), { animate: false });
  startCycle();
  window.addEventListener("dync-appearance", () => {
    applyTheme(currentName(), { animate: true });
  });
}
