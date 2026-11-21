/** Pure labels for session cards. Words match docs/plan/00-ground-rules.md §5. */

import { needsYou } from "./queue.js";

const STATE_LABELS = {
  launching: "Launching",
  working: "Working",
  blocked: "Blocked on you",
  ready: "Ready for you",
  ended: "Ended",
};

const GOAL_DRIVING_LABELS = {
  baseline: "Checking before start",
  working: "Working",
  checking: "Checking",
  reviewing: "Reviewer reading",
};

const BLOCKED_LABELS = {
  approval: "Needs approval",
  question: "Has a question",
  startup: "Needs a look",
};

const AGENT_FALLBACK = {
  claude: "Claude Code",
  codex: "Codex",
  pi: "Pi",
  cursor: "Cursor Agent",
  shell: "Shell",
};

export function stateLabel(session) {
  if (!session) return "";
  if (session.archived) return "Archived";
  // A ready session the goal is still driving is not waiting on you: say what the goal is doing.
  const goalWord = GOAL_DRIVING_LABELS[session.goal?.status];
  if (session.state === "ready" && goalWord) return goalWord;
  let label = STATE_LABELS[session.state] || session.state || "";
  if (session.state === "ended" && session.exit_code != null && session.exit_code !== 0) {
    label = `Ended (${session.exit_code})`;
  }
  return label;
}

export function blockedLabel(blocked) {
  if (!blocked) return "";
  const kind = BLOCKED_LABELS[blocked.kind] || "Needs a look";
  const msg = String(blocked.message || "").trim();
  if (!msg) return kind;
  return `${kind}: ${msg}`;
}

export function agentLabel(meta, id) {
  if (!id) return "";
  const fromMeta = meta?.agents?.find((a) => a.id === id);
  if (fromMeta?.label) return fromMeta.label;
  return AGENT_FALLBACK[id] || id;
}

/** Compact token count for usage labels (`1.2k`, `12k`, `1.5M`). */
export function formatTokens(n) {
  const v = Number(n);
  if (!Number.isFinite(v) || v < 0) return "";
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (v >= 10_000) return `${Math.round(v / 1000)}k`;
  if (v >= 1_000) return `${(v / 1000).toFixed(1)}k`;
  return String(Math.round(v));
}

export function usageLabel(usage, { named = false } = {}) {
  if (!usage || typeof usage !== "object") return "";
  const parts = [];
  if (typeof usage.cost_usd === "number" && Number.isFinite(usage.cost_usd)) {
    parts.push(`${named ? "Cost " : ""}$${usage.cost_usd.toFixed(2)}`);
  } else if (typeof usage.tokens === "number" && Number.isFinite(usage.tokens) && usage.tokens > 0) {
    const t = formatTokens(usage.tokens);
    parts.push(named ? `Tokens ${t}` : `${t} tokens`);
  }
  if (typeof usage.context_pct === "number" && Number.isFinite(usage.context_pct)) {
    parts.push(`${named ? "Context " : ""}${Math.round(usage.context_pct)}%`);
  }
  return parts.join(" · ");
}

export function workspaceLabel(workspace) {
  if (!workspace) return "";
  if (workspace.kind === "worktree" && workspace.branch) {
    return workspace.branch;
  }
  const path = String(workspace.path || "").replace(/\/+$/, "");
  if (!path) return workspace.kind === "scratch" ? "Scratch" : "";
  const parts = path.split("/").filter(Boolean);
  if (parts.length >= 2) return `${parts[parts.length - 2]}/${parts[parts.length - 1]}`;
  return parts[parts.length - 1] || path;
}

export function toolLabel(tool) {
  if (!tool) return "";
  const name = String(tool.name || "").trim();
  let detail = String(tool.detail || "").trim();
  if (detail.length > 160) detail = `${detail.slice(0, 157)}…`;
  if (name && detail) return `${name} · ${detail}`;
  return name || detail;
}

/** True when the session is in Your move or snoozed out of it. */
export function inQueueOrSnoozed(session) {
  if (!session) return false;
  if (session.snoozed) return true;
  return needsYou(session);
}
