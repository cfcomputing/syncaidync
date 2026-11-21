import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  stateLabel,
  blockedLabel,
  agentLabel,
  usageLabel,
  workspaceLabel,
  toolLabel,
} from "../js/lib/labels.js";

describe("stateLabel", () => {
  it("covers every state", () => {
    assert.equal(stateLabel({ state: "launching" }), "Launching");
    assert.equal(stateLabel({ state: "working" }), "Working");
    assert.equal(stateLabel({ state: "blocked" }), "Blocked on you");
    assert.equal(stateLabel({ state: "ready" }), "Ready for you");
    assert.equal(stateLabel({ state: "ended" }), "Ended");
    assert.equal(stateLabel({ state: "ended", exit_code: 1 }), "Ended (1)");
    assert.equal(stateLabel({ state: "ended", exit_code: 0 }), "Ended");
  });
});

describe("blockedLabel", () => {
  it("covers every blocked kind", () => {
    assert.equal(blockedLabel({ kind: "approval", message: "Bash: rm" }), "Needs approval: Bash: rm");
    assert.equal(blockedLabel({ kind: "question", message: "Which?" }), "Has a question: Which?");
    assert.equal(blockedLabel({ kind: "startup", message: "" }), "Needs a look");
    assert.equal(blockedLabel(null), "");
  });
});

describe("agentLabel", () => {
  it("prefers meta label then fallback", () => {
    assert.equal(agentLabel({ agents: [{ id: "claude", label: "Claude Code" }] }, "claude"), "Claude Code");
    assert.equal(agentLabel(null, "shell"), "Shell");
    assert.equal(agentLabel(null, "mystery"), "mystery");
  });
});

describe("usageLabel", () => {
  it("formats cost, tokens, context, or empty", () => {
    assert.equal(usageLabel({ cost_usd: 0.42, context_pct: 31 }), "$0.42 · 31%");
    assert.equal(usageLabel({ cost_usd: 1.5 }), "$1.50");
    assert.equal(usageLabel({ context_pct: 12.2 }), "12%");
    assert.equal(usageLabel({ cost_usd: 0, context_pct: 31 }, { named: true }), "Cost $0.00 · Context 31%");
    assert.equal(usageLabel({ tokens: 1200 }), "1.2k tokens");
    assert.equal(usageLabel({ tokens: 42_000 }, { named: true }), "Tokens 42k");
    assert.equal(usageLabel({ tokens: 1500, cost_usd: 0.5 }), "$0.50");
    assert.equal(usageLabel(null), "");
    assert.equal(usageLabel({}), "");
  });
});

describe("workspaceLabel", () => {
  it("labels the three kinds", () => {
    assert.equal(
      workspaceLabel({ kind: "worktree", branch: "dync/fix", path: "/x/y" }),
      "dync/fix",
    );
    assert.equal(
      workspaceLabel({ kind: "folder", path: "/Users/me/code/shop" }),
      "code/shop",
    );
    assert.equal(
      workspaceLabel({ kind: "scratch", path: "/Users/me/.syncaidync/scratch/abc" }),
      "scratch/abc",
    );
  });
});

describe("toolLabel", () => {
  it("joins name and detail, cuts long details", () => {
    assert.equal(toolLabel({ name: "Bash", detail: "cargo test" }), "Bash · cargo test");
    const long = "x".repeat(200);
    const out = toolLabel({ name: "Bash", detail: long });
    assert.ok(out.startsWith("Bash · "));
    assert.ok(out.endsWith("…"));
    assert.ok(out.length <= "Bash · ".length + 160);
  });
});

describe("stateLabel with a goal", () => {
  it("names what a driving goal is doing instead of Ready for you", () => {
    assert.equal(stateLabel({ state: "ready", goal: { status: "checking" } }), "Checking");
    assert.equal(stateLabel({ state: "ready", goal: { status: "reviewing" } }), "Reviewer reading");
    assert.equal(stateLabel({ state: "ready", goal: { status: "paused" } }), "Ready for you");
    assert.equal(stateLabel({ state: "working", goal: { status: "working" } }), "Working");
  });
});
