/** Day / night / auto appearance. */

import * as prefs from "./prefs.js";

const TRANSITION_MS = 300;
let timer = null;
let minuteTimer = null;

function resolve(pref) {
  if (pref === "day" || pref === "night") return pref;
  const h = new Date().getHours();
  return h >= 7 && h < 19 ? "day" : "night";
}

function apply(appearance) {
  const root = document.documentElement;
  root.classList.add("appearance-transition");
  root.setAttribute("data-appearance", appearance);
  clearTimeout(timer);
  timer = setTimeout(() => root.classList.remove("appearance-transition"), TRANSITION_MS);
}

export function current() {
  return prefs.get("appearance", "auto");
}

export function set(pref) {
  const next = pref === "day" || pref === "night" || pref === "auto" ? pref : "auto";
  prefs.set("appearance", next);
  apply(resolve(next));
  ensureMinuteTick();
  window.dispatchEvent(new CustomEvent("dync-appearance", { detail: { preference: next, appearance: resolve(next) } }));
  return next;
}

export function init() {
  apply(resolve(current()));
  ensureMinuteTick();
}

function ensureMinuteTick() {
  if (minuteTimer != null) return;
  minuteTimer = setInterval(() => {
    if (current() === "auto") apply(resolve("auto"));
  }, 60_000);
}

// Transition helper class lives in base/chrome via attribute; add a tiny rule here via style if needed.
if (typeof document !== "undefined") {
  const style = document.createElement("style");
  style.textContent =
    "html.appearance-transition, html.appearance-transition * { transition: background-color 300ms ease, color 300ms ease, border-color 300ms ease !important; }";
  document.documentElement.appendChild(style);
}
