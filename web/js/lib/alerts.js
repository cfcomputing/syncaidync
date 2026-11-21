/** Title, favicon, notifications, and reminder loop. */

import { store } from "./store.js";
import { queueCounts, queueSessions, diffQueue } from "./queue.js";
import { blockedLabel } from "./labels.js";
import { pillText } from "./goal.js";
import { play, unlock } from "./sound.js";
import * as prefs from "./prefs.js";
import * as route from "./route.js";
import * as native from "./native.js";

const DEFAULT_ALERTS = {
  sound: true,
  volume: 0.7,
  notify: true,
  remind_minutes: 5,
  on_ready: true,
};

const BASE_FAVICON = "/favicon.svg";
const WARN_DOT = "#f0b429";

let started = false;
let previous = null; // null until first snapshot processed
let lastRemindAt = 0;
let remindTimer = null;
let iconLink = null;

export function titleForCount(count) {
  const n = Number(count) || 0;
  return n > 0 ? `(${n}) SyncAiDync` : "SyncAiDync";
}

/**
 * Whether a reminder tone is due.
 * @param {{ remind_minutes: number|false, blockedCount: number, now: number, lastRemindAt: number }} input
 */
export function shouldRemind(input) {
  const minutes = input.remind_minutes;
  if (minutes === false || minutes == null || !(minutes > 0)) return false;
  if (!(input.blockedCount > 0)) return false;
  const last = input.lastRemindAt || 0;
  if (!last) return false; // wait one full interval after start / last alert
  return input.now - last >= minutes * 60_000;
}

function alertsPrefs() {
  return prefs.get("alerts", DEFAULT_ALERTS) || DEFAULT_ALERTS;
}

function faviconWithDot() {
  return `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" fill="none">
  <defs>
    <linearGradient id="g" x1="4" y1="4" x2="28" y2="28" gradientUnits="userSpaceOnUse">
      <stop stop-color="#a78bfa"/>
      <stop offset="1" stop-color="#818cf8"/>
    </linearGradient>
  </defs>
  <path d="M 24.485 7.515 A 12 12 0 0 1 24.485 24.485" stroke="url(#g)" stroke-width="2.5" stroke-linecap="round"/>
  <path d="M 7.515 24.485 A 12 12 0 0 1 7.515 7.515" stroke="url(#g)" stroke-width="2.5" stroke-linecap="round"/>
  <polyline points="11,12 15,16 11,20" stroke="#e8e8ed" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/>
  <line x1="17" y1="20" x2="21" y2="20" stroke="#e8e8ed" stroke-width="2.5" stroke-linecap="round"/>
  <circle cx="26" cy="26" r="5" fill="${WARN_DOT}"/>
</svg>`)}`;
}

function ensureIconLink() {
  if (iconLink && iconLink.isConnected) return iconLink;
  iconLink = document.querySelector('link[rel="icon"]');
  if (!iconLink) {
    iconLink = document.createElement("link");
    iconLink.rel = "icon";
    document.head.appendChild(iconLink);
  }
  return iconLink;
}

export function applyChrome(count) {
  document.title = titleForCount(count);
  const link = ensureIconLink();
  link.href = count > 0 ? faviconWithDot() : BASE_FAVICON;
  if (native.isNative()) native.setBadge(count);
}

function focusedTerminalSession() {
  const term = document.querySelector('dync-terminal[mode="edit"]');
  return term?.getAttribute("session") || null;
}

function alertsEnabled() {
  const r = route.current();
  if (r.solo) return false;
  return true;
}

function notifySession(session, kind) {
  const settings = alertsPrefs();
  if (!settings.notify) return;


  const title = session.title || "Untitled session";
  const goalDone = session.goal && ["met", "paused"].includes(session.goal.status);
  const body =
    kind === "blocked"
      ? `Blocked on you: ${blockedLabel(session.blocked) || "Needs a look"}`
      : goalDone
        ? pillText(session.goal)
        : "Ready for you";

  if (native.isNative()) {
    native.notify({ id: session.id, title, body });
    return;
  }

  if (!document.hidden || typeof Notification === "undefined") return;
  if (Notification.permission !== "granted") return;

  try {
    const n = new Notification(title, { body, tag: `dync-${session.id}` });
    n.onclick = () => {
      try {
        window.focus();
      } catch {
        /* ignore */
      }
      route.go({ name: "session", id: session.id });
      n.close();
    };
  } catch {
    /* ignore */
  }
}

function handleDiffs(diffs) {
  const settings = alertsPrefs();
  for (const d of diffs) {
    if (d.kind === "ready" && !settings.on_ready) continue;
    const session = store.sessions.get(d.id);
    if (!session || session.snoozed) continue;

    const looking =
      !document.hidden && focusedTerminalSession() === d.id;
    if (looking) continue;

    play(d.kind === "blocked" ? "blocked" : "ready");
    notifySession(session, d.kind);
    lastRemindAt = Date.now();
  }
}

function tick() {
  if (!alertsEnabled()) {
    applyChrome(0);
    return;
  }

  const sessions = store.sessions;
  const counts = queueCounts(sessions);
  applyChrome(counts.total);

  const first = previous == null;
  const diffs = diffQueue(previous, sessions);
  previous = new Map(
    [...sessions].map(([id, s]) => [id, { ...s, blocked: s.blocked ? { ...s.blocked } : null }]),
  );
  if (first) {
    if (counts.blocked > 0) lastRemindAt = Date.now();
  } else {
    handleDiffs(diffs);
  }

  const settings = alertsPrefs();
  if (
    shouldRemind({
      remind_minutes: settings.remind_minutes,
      blockedCount: counts.blocked,
      now: Date.now(),
      lastRemindAt,
    })
  ) {
    play("remind");
    lastRemindAt = Date.now();
  }

  window.dispatchEvent(
    new CustomEvent("dync-queue", {
      detail: { ...counts, first: queueSessions(sessions)[0] || null },
    }),
  );
}

export function startAlerts() {
  if (started) return;
  started = true;

  // Unlock audio on first gesture.
  const unlockOnce = () => {
    unlock();
    window.removeEventListener("pointerdown", unlockOnce, true);
    window.removeEventListener("keydown", unlockOnce, true);
  };
  window.addEventListener("pointerdown", unlockOnce, true);
  window.addEventListener("keydown", unlockOnce, true);

  store.subscribe(() => tick());
  document.addEventListener("visibilitychange", () => tick());
  window.addEventListener("dync-alerts-prefs", () => tick());

  remindTimer = setInterval(() => tick(), 15_000);
  tick();
}

export function stopAlerts() {
  started = false;
  previous = null;
  if (remindTimer) {
    clearInterval(remindTimer);
    remindTimer = null;
  }
}

export { DEFAULT_ALERTS };
