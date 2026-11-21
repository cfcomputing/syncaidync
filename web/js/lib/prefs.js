/** Typed localStorage access under syncaidync: keys. Never throws. */

const PREFIX = "syncaidync:";

const validators = {
  appearance: (v) => (v === "auto" || v === "day" || v === "night" ? v : null),
  theme: (v) => (typeof v === "string" && v.length > 0 ? v : null),
  "show-archived-sessions": (v) => (typeof v === "boolean" ? v : null),
  "theme-cycle": (v) => (typeof v === "boolean" ? v : null),
  alerts: (v) =>
    v &&
    typeof v === "object" &&
    typeof v.sound === "boolean" &&
    typeof v.volume === "number" &&
    typeof v.notify === "boolean" &&
    (v.remind_minutes === false || typeof v.remind_minutes === "number") &&
    typeof v.on_ready === "boolean"
      ? v
      : null,
  board: (v) =>
    v &&
    typeof v === "object" &&
    (v.layout === "rows" || v.layout === "grid" || v.layout === "free") &&
    Array.isArray(v.collapsed) &&
    v.free &&
    typeof v.free === "object"
      ? v
      : null,
  "new-session": (v) => (v && typeof v === "object" ? v : null),
  // Last arranged workspace layout; new sessions copy this until they get their own.
  "workspace-seed": (v) => (v && typeof v === "object" && Array.isArray(v.groups) ? v : null),
  "goal-defaults": (v) => (v && typeof v === "object" && !Array.isArray(v) ? v : null),
  changes: (v) =>
    v && typeof v === "object" && (v.view === "unified" || v.view === "split")
      ? v
      : null,
  "files-md-mode": (v) => (v === "view" || v === "source" ? v : null),
  "changes-md-mode": (v) => (v === "view" || v === "diff" ? v : null),
};

function storageKey(key) {
  return key.startsWith(PREFIX) ? key : PREFIX + key;
}

function validatorFor(key) {
  const bare = key.startsWith(PREFIX) ? key.slice(PREFIX.length) : key;
  if (validators[bare]) return validators[bare];
  if (bare.startsWith("workspace:")) {
    return (v) => (v && typeof v === "object" ? v : null);
  }
  return (v) => v;
}

export function get(key, fallback) {
  try {
    const raw = localStorage.getItem(storageKey(key));
    if (raw == null) return fallback;
    let parsed;
    try {
      parsed = JSON.parse(raw);
    } catch {
      return fallback;
    }
    const ok = validatorFor(key)(parsed);
    return ok == null ? fallback : ok;
  } catch {
    return fallback;
  }
}

export function set(key, value) {
  try {
    localStorage.setItem(storageKey(key), JSON.stringify(value));
    return true;
  } catch {
    return false;
  }
}

export function remove(key) {
  try {
    localStorage.removeItem(storageKey(key));
    return true;
  } catch {
    return false;
  }
}
