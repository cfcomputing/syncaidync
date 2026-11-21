/** Your-move queue: order, counts, and alert diffs. */

function asList(sessions) {
  if (Array.isArray(sessions)) return sessions.slice();
  if (sessions instanceof Map) return [...sessions.values()];
  return Object.values(sessions || {});
}

function asMap(sessions) {
  const m = new Map();
  for (const s of asList(sessions)) {
    if (s?.id) m.set(s.id, s);
  }
  return m;
}

/**
 * True when a session's state puts it in Your move (ignoring snooze). A shell sitting at its
 * prompt is `ready` but does not need you; it joins the queue only when it asks something.
 */
export function needsYou(s) {
  if (!s) return false;
  if (s.state === "blocked") return true;
  // A goal still driving the session (checking, sending a round) is not your move yet.
  if (s.state === "ready" && goalDriving(s)) return false;
  return s.state === "ready" && s.agent !== "shell";
}

const DRIVING = new Set(["baseline", "working", "checking", "reviewing"]);

function goalDriving(s) {
  return !!s?.goal && DRIVING.has(s.goal.status);
}

/**
 * Sessions that need you: blocked or ready (shells only when blocked), and not snoozed.
 * Blocked first, then ready; within each, longest wait first (oldest state_since).
 */
export function queueSessions(sessions) {
  return asList(sessions)
    .filter((s) => !s.snoozed && needsYou(s))
    .sort((a, b) => {
      const rank = (s) => (s.state === "blocked" ? 0 : 1);
      const r = rank(a) - rank(b);
      if (r !== 0) return r;
      return (a.state_since ?? 0) - (b.state_since ?? 0);
    });
}

export function queueCounts(sessions) {
  const q = queueSessions(sessions);
  let blocked = 0;
  let ready = 0;
  for (const s of q) {
    if (s.state === "blocked") blocked += 1;
    else ready += 1;
  }
  return { total: q.length, blocked, ready };
}

/**
 * Alert diffs between two session snapshots.
 * Pass `previous == null` for the first snapshot (yields nothing).
 * kind is `blocked` or `ready`.
 */
export function diffQueue(previous, next) {
  if (previous == null) return [];
  const prev = asMap(previous);
  const curr = asMap(next);
  const out = [];

  for (const [id, s] of curr) {
    if (s.snoozed) continue;
    const p = prev.get(id);

    if (s.state === "blocked") {
      if (!p || p.state !== "blocked" || blockedKind(p) !== blockedKind(s)) {
        out.push({ id, kind: "blocked" });
      }
      continue;
    }

    if (s.state === "ready" && needsYou(s)) {
      // Ready after a turn, or a goal that just stopped driving (met or paused).
      if (p && (p.state === "working" || (p.state === "ready" && goalDriving(p)))) {
        out.push({ id, kind: "ready" });
      }
    }
  }

  return out;
}

function blockedKind(s) {
  return s?.blocked?.kind || "";
}
