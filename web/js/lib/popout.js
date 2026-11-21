/** Track tool pop-out windows via BroadcastChannel. */

const CHANNEL = "syncaidync";

/** @type {BroadcastChannel | null} */
let channel = null;
const listeners = new Set();
/** sessionId -> tool -> Window */
const windows = new Map();

function bc() {
  if (typeof BroadcastChannel === "undefined") return null;
  if (!channel) {
    channel = new BroadcastChannel(CHANNEL);
    channel.onmessage = (ev) => {
      const msg = ev.data;
      if (!msg || typeof msg !== "object") return;
      for (const fn of listeners) {
        try {
          fn(msg);
        } catch {
          /* ignore */
        }
      }
    };
  }
  return channel;
}

export function subscribe(fn) {
  bc();
  listeners.add(fn);
  return () => listeners.delete(fn);
}

export function announce(type, detail) {
  bc()?.postMessage({ type, ...detail });
}

export function windowName(sessionId, tool) {
  return `dync-${sessionId}-${tool}`;
}

export function openPopout(sessionId, tool) {
  const name = windowName(sessionId, tool);
  const existing = windows.get(sessionId)?.get(tool);
  if (existing && !existing.closed) {
    existing.focus();
    return existing;
  }
  const url = `${location.origin}${location.pathname}${location.search}#/s/${encodeURIComponent(sessionId)}/${tool}?solo=1`;
  const win = window.open(url, name, "width=900,height=700,menubar=no,toolbar=no");
  if (!win) return null;
  if (!windows.has(sessionId)) windows.set(sessionId, new Map());
  windows.get(sessionId).set(tool, win);
  return win;
}

export function closePopout(sessionId, tool) {
  const win = windows.get(sessionId)?.get(tool);
  if (win && !win.closed) win.close();
  windows.get(sessionId)?.delete(tool);
}

export function isPoppedOut(sessionId, tool) {
  const win = windows.get(sessionId)?.get(tool);
  return !!(win && !win.closed);
}

export function markClosed(sessionId, tool) {
  windows.get(sessionId)?.delete(tool);
}
