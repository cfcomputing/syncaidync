/** Pure helpers for goals: labels, pipeline stages, and the checks the dialog makes before
 * Start. Words match docs/plan/00-ground-rules.md §5. */

export const ACTIVE = new Set(["baseline", "working", "checking", "reviewing"]);

const PAUSE_LABELS = {
  rounds: "round limit",
  cost: "cost limit",
  time: "time limit",
  guard: "checks changed",
  manual: "your check",
  ended: "session ended",
  review: "reviewer couldn’t run",
};

export function goalActive(goal) {
  return !!goal && ACTIVE.has(goal.status);
}

export function commandChecks(goal) {
  return (goal?.checks || []).filter((c) => (c.kind || "command") === "command");
}

export function passedCount(goal) {
  return commandChecks(goal).filter((c) => c.last?.passed && !c.last?.tampered).length;
}

/** The short text on a card's goal pill. */
export function pillText(goal) {
  if (!goal) return "";
  const cmds = commandChecks(goal);
  const total = cmds.length;
  const passed = passedCount(goal);
  const limit = goal.limits?.rounds ?? 0;
  switch (goal.status) {
    case "baseline":
      return "Checking before start…";
    case "checking": {
      const running = goal.checks?.find((c) => c.id === goal.running);
      return running ? `Checking · ${running.title}` : "Checking…";
    }
    case "reviewing":
      return "Reviewer reading…";
    case "working":
      return goal.round > 0
        ? `Goal · ${passed}/${total} · round ${goal.round}/${limit}`
        : `Goal · ${passed}/${total}`;
    case "paused":
      return `Paused: ${PAUSE_LABELS[goal.paused?.reason] || "needs a look"}`;
    case "met":
      return goal.report?.committed ? "Goal met · committed" : "Goal met";
    case "dropped":
      return "Goal dropped";
    default:
      return "Goal";
  }
}

/** `active`, `warn`, `ok` or `quiet`, for the pill's colour. */
export function pillTone(goal) {
  if (!goal) return "quiet";
  if (goal.status === "paused") return "warn";
  if (goal.status === "met") return "ok";
  if (goal.status === "dropped") return "quiet";
  return "active";
}

/**
 * The pipeline the goal moves through, left to right, each with a state:
 * `done`, `now`, `fail`, `wait` or `off`.
 */
export function stages(goal) {
  if (!goal) return [];
  const cmds = commandChecks(goal);
  const quick = cmds.filter((c) => (c.stage || "quick") === "quick");
  const full = cmds.filter((c) => c.stage === "full");
  const manual = (goal.checks || []).filter((c) => c.kind === "manual");
  const s = goal.status;
  const runningCheck = cmds.find((c) => c.id === goal.running);

  const groupState = (list, stageName) => {
    if (!list.length) return "off";
    if (s === "checking" || s === "baseline") {
      if (runningCheck && (runningCheck.stage || "quick") === stageName) return "now";
    }
    const results = list.map((c) => c.last);
    if (results.some((r) => !r)) return "wait";
    if (results.some((r) => r.skipped)) return "wait";
    if (results.some((r) => !r.passed || r.tampered)) return "fail";
    return "done";
  };

  const agentState =
    s === "working" ? "now" : s === "met" ? "done" : s === "paused" ? "wait" : "done";
  const quickState = groupState(quick, "quick");
  const fullState = groupState(full, "full");
  let reviewState = "off";
  if (goal.reviewer?.on) {
    if (s === "reviewing") reviewState = "now";
    else if (goal.verdict?.approved && goal.verdict.round === goal.round) reviewState = "done";
    else if (goal.verdict && !goal.verdict.approved) reviewState = "fail";
    else reviewState = "wait";
  }
  let manualState = "off";
  if (manual.length) manualState = manual.every((c) => c.done) ? "done" : "wait";
  const metState = s === "met" ? "done" : s === "paused" ? "fail" : "wait";

  const out = [
    { id: "agent", label: "Agent", detail: roundDetail(goal), state: agentState },
    { id: "quick", label: "Quick checks", detail: groupDetail(quick), state: quickState },
    { id: "full", label: "Full checks", detail: groupDetail(full), state: fullState },
    {
      id: "reviewer",
      label: "Reviewer",
      detail: goal.reviewer?.on ? goal.reviewer.model || goal.reviewer.agent || "" : "",
      state: reviewState,
    },
    {
      id: "manual",
      label: "Your checks",
      detail: manual.length ? `${manual.filter((c) => c.done).length}/${manual.length}` : "",
      state: manualState,
    },
    {
      id: "met",
      label: s === "paused" ? "Paused" : "Goal met",
      detail: s === "paused" ? PAUSE_LABELS[goal.paused?.reason] || "" : "",
      state: metState,
    },
  ];
  return out.filter((st) => st.state !== "off");
}

function roundDetail(goal) {
  const limit = goal.limits?.rounds ?? 0;
  return goal.round > 0 ? `round ${goal.round}/${limit}` : "first turn";
}

function groupDetail(list) {
  if (!list.length) return "";
  const passed = list.filter((c) => c.last?.passed && !c.last?.tampered).length;
  return `${passed}/${list.length}`;
}

/** One check's last result as a word. */
export function resultLabel(check) {
  if ((check.kind || "command") === "manual") return check.done ? "Ticked" : "Waiting on you";
  const r = check.last;
  if (!r) return "Not run yet";
  if (r.tampered) return "Changed";
  if (r.skipped) return "Skipped";
  if (r.timed_out) return "Timed out";
  return r.passed ? "Pass" : "Fail";
}

export function resultTone(check) {
  const label = resultLabel(check);
  if (label === "Pass" || label === "Ticked") return "ok";
  if (label === "Fail" || label === "Timed out") return "danger";
  if (label === "Changed" || label === "Waiting on you") return "warn";
  return "quiet";
}

/** Fraction 0..1 for each limit, or null when there is no limit. */
export function limitMeters(goal, session, now = Date.now()) {
  const limits = goal?.limits || {};
  const working =
    (goal?.working_ms || 0) +
    (session?.state === "working" && goal?.status === "working"
      ? Math.max(0, now - (session.state_since || now))
      : 0);
  const cost =
    typeof session?.usage?.cost_usd === "number"
      ? Math.max(0, session.usage.cost_usd - (goal?.cost_at_start || 0))
      : null;
  return {
    rounds: {
      used: goal?.round || 0,
      limit: limits.rounds || 0,
      frac: limits.rounds ? Math.min(1, (goal?.round || 0) / limits.rounds) : null,
    },
    cost: {
      used: cost,
      limit: limits.cost_usd ?? null,
      frac: limits.cost_usd && cost != null ? Math.min(1, cost / limits.cost_usd) : null,
    },
    time: {
      used: working,
      limit: limits.minutes ?? null,
      frac: limits.minutes ? Math.min(1, working / (limits.minutes * 60_000)) : null,
    },
  };
}

/** Criteria that have no check yet. */
export function uncovered(criteria, checks) {
  return (criteria || []).filter(
    (k) => String(k.text || "").trim() && !(checks || []).some((c) => c.criterion === k.id),
  );
}

/**
 * What the dialog checks before Start. Returns an error sentence or "" when the goal is fine.
 * Mirrors the server's messages so the dialog says the same thing either way.
 */
export function validateGoal(goal) {
  const text = String(goal?.text || "").trim();
  if (!text || text.length > 500) return "The goal must be between 1 and 500 characters.";
  const criteria = (goal.criteria || []).filter((k) => String(k.text || "").trim());
  if (!criteria.length || criteria.length > 8) return "Add between 1 and 8 done criteria.";
  for (const k of criteria) {
    if (k.text.trim().length > 200) {
      return "Each done criterion must be between 1 and 200 characters.";
    }
  }
  const checks = goal.checks || [];
  if (checks.length > 12) return "A goal can have at most 12 checks.";
  for (const c of checks) {
    const title = String(c.title || "").trim();
    if (!title || title.length > 80) return "Each check needs a title of at most 80 characters.";
    if ((c.kind || "command") === "command" && !String(c.command || "").trim()) {
      return `The check "${title}" needs a command.`;
    }
    const t = Number(c.timeout_s);
    if (!(t >= 5 && t <= 1800)) {
      return `The timeout for "${title}" must be between 5 and 1800 seconds.`;
    }
  }
  const missing = uncovered(criteria, checks);
  if (missing.length) return `"${missing[0].text.trim()}" has no check yet.`;
  return validateLimits(goal.limits);
}

export function validateLimits(limits) {
  const r = Number(limits?.rounds);
  if (!(Number.isInteger(r) && r >= 1 && r <= 10)) return "The round limit must be between 1 and 10.";
  if (limits.cost_usd != null) {
    const c = Number(limits.cost_usd);
    if (!(c >= 0.01 && c <= 1000)) return "The cost limit must be between $0.01 and $1000.";
  }
  if (limits.minutes != null) {
    const m = Number(limits.minutes);
    if (!(Number.isInteger(m) && m >= 1 && m <= 1440)) {
      return "The time limit must be between 1 and 1440 minutes.";
    }
  }
  return "";
}

/** Read a limits form value: empty means no limit. */
export function parseLimit(value, { integer = false } = {}) {
  const t = String(value ?? "").trim();
  if (!t) return null;
  const n = integer ? Number.parseInt(t, 10) : Number.parseFloat(t.replace(/^\$/, ""));
  return Number.isFinite(n) ? n : NaN;
}

/** The next unused id with a prefix, such as `k3` or `c5`. */
export function nextId(prefix, items) {
  let n = 1;
  const used = new Set((items || []).map((i) => i.id));
  while (used.has(`${prefix}${n}`)) n += 1;
  return `${prefix}${n}`;
}

/** Rounds newest first, each with how many command checks passed. */
export function roundSummaries(record) {
  return [...(record?.rounds || [])]
    .map((r) => {
      const results = r.results || [];
      const passed = results.filter((x) => x.passed && !x.tampered).length;
      return {
        round: r.round,
        at: r.at,
        label: r.round === 0 ? "Before start" : `Round ${r.round}`,
        passed,
        total: results.length,
        results,
      };
    })
    .sort((a, b) => b.round - a.round || b.at - a.at);
}

/** Format working milliseconds as minutes. */
export function minutesLabel(ms) {
  const m = Math.floor((ms || 0) / 60_000);
  const s = Math.floor(((ms || 0) % 60_000) / 1000);
  return m ? `${m}m ${String(s).padStart(2, "0")}s` : `${s}s`;
}

/** Checks whose guarded paths changed since they were last accepted. */
export function flaggedChecks(goal) {
  return (goal?.checks || []).filter((c) => c.last?.tampered);
}

/** "3 files changed · +40 −12" for a finish report. */
export function reportFilesLabel(report) {
  const files = report?.files || [];
  const count = files.length + (report?.more_files || 0);
  if (!count) return "No file changes against the base";
  const added = files.reduce((n, f) => n + (f.added || 0), 0);
  const removed = files.reduce((n, f) => n + (f.removed || 0), 0);
  return `${count} file${count === 1 ? "" : "s"} changed · +${added} −${removed}`;
}

/** Rounds to allow when keeping a met goal going: two more, at most 10. */
export function moreRounds(goal) {
  return Math.min(10, (goal?.round || 0) + 2);
}
