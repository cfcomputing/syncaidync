/** Live sessions/pulls/meta via SSE. */

import { api } from "./api.js";

const listeners = new Set();

export const store = {
  sessions: new Map(),
  hasSessionSnapshot: false,
  pulls: new Map(),
  meta: null,
  update: null,
  connected: false,
  subscribe(fn) {
    listeners.add(fn);
    return () => listeners.delete(fn);
  },
};

let source = null;
let started = false;

function emit() {
  for (const fn of listeners) {
    try {
      fn(store);
    } catch (_) {
      /* subscriber errors stay local */
    }
  }
}

function applyEvent(type, data) {
  switch (type) {
    case "state": {
      if (!Array.isArray(data?.sessions)) return;
      store.sessions = new Map(data.sessions.map((s) => [s.id, s]));
      store.pulls = new Map((data.pulls || []).map((p) => [p.id, p]));
      store.hasSessionSnapshot = true;
      break;
    }
    case "session":
      if (data?.id) store.sessions.set(data.id, data);
      break;
    case "session_gone":
      if (data?.id) store.sessions.delete(data.id);
      break;
    case "pull":
      if (data?.id) store.pulls.set(data.id, data);
      break;
    case "pull_gone":
      if (data?.id) store.pulls.delete(data.id);
      break;
    default:
      return;
  }
  emit();
}

function connect() {
  if (source) {
    source.close();
    source = null;
  }
  source = new EventSource("/api/v1/stream", { withCredentials: true });
  source.onopen = () => {
    store.connected = true;
    emit();
  };
  source.onerror = () => {
    if (store.connected) {
      store.connected = false;
      emit();
    }
  };
  for (const type of ["state", "session", "session_gone", "pull", "pull_gone"]) {
    source.addEventListener(type, (ev) => {
      try {
        applyEvent(type, JSON.parse(ev.data));
      } catch (_) {
        /* ignore bad frames */
      }
    });
  }
}

export async function startStore() {
  if (started) return store;
  started = true;
  store.meta = await api.get("/meta");
  connect();
  api.get("/update").then((update) => {
    store.update = update;
    window.dispatchEvent(new Event("dync-update"));
  }).catch(() => {});
  return store;
}

export function stopStore() {
  started = false;
  if (source) {
    source.close();
    source = null;
  }
  store.connected = false;
  store.hasSessionSnapshot = false;
}
