import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  pillText,
  pillTone,
  stages,
  resultLabel,
  uncovered,
  validateGoal,
  validateLimits,
  parseLimit,
  nextId,
  limitMeters,
  roundSummaries,
  flaggedChecks,
  reportFilesLabel,
  moreRounds,
} from "../js/lib/goal.js";
import { queueSessions, diffQueue } from "../js/lib/queue.js";

function goal(partial = {}) {
  return {
    text: "Fix login",
    criteria: [{ id: "k1", text: "Redirects once" }],
    checks: [
      { id: "c1", criterion: "k1", title: "Unit", kind: "command", stage: "quick", command: "t", timeout_s: 300, last: null },
      { id: "c2", criterion: "k1", title: "E2E", kind: "command", stage: "full", command: "e", timeout_s: 300, last: null },
    ],
    limits: { rounds: 3, cost_usd: null, minutes: null },
    reviewer: { on: false },
    status: "working",
    paused: null,
    round: 0,
    running: null,
    working_ms: 0,
    cost_at_start: 0,
    ...partial,
  };
}

const pass = { passed: true, tampered: false, skipped: false };
const fail = { passed: false, tampered: false, skipped: false };

describe("pillText", () => {
  it("names every status and pause reason", () => {
    assert.equal(pillText(goal()), "Goal · 0/2");
    const g = goal({ round: 2 });
    g.checks[0].last = pass;
    assert.equal(pillText(g), "Goal · 1/2 · round 2/3");
    assert.equal(pillText(goal({ status: "baseline" })), "Checking before start…");
    assert.equal(pillText(goal({ status: "checking", running: "c2" })), "Checking · E2E");
    assert.equal(pillText(goal({ status: "checking" })), "Checking…");
    assert.equal(pillText(goal({ status: "reviewing" })), "Reviewer reading…");
    assert.equal(pillText(goal({ status: "met" })), "Goal met");
    assert.equal(pillText(goal({ status: "dropped" })), "Goal dropped");
    const reasons = {
      rounds: "Paused: round limit",
      cost: "Paused: cost limit",
      time: "Paused: time limit",
      guard: "Paused: checks changed",
      manual: "Paused: your check",
      ended: "Paused: session ended",
      review: "Paused: reviewer couldn’t run",
    };
    for (const [reason, text] of Object.entries(reasons)) {
      assert.equal(pillText(goal({ status: "paused", paused: { reason, message: "" } })), text);
    }
    assert.equal(pillText(null), "");
  });

  it("picks a tone", () => {
    assert.equal(pillTone(goal({ status: "paused", paused: { reason: "cost" } })), "warn");
    assert.equal(pillTone(goal({ status: "met" })), "ok");
    assert.equal(pillTone(goal({ status: "checking" })), "active");
  });
});

describe("stages", () => {
  it("lights the running stage and marks failures", () => {
    const g = goal({ status: "checking", running: "c1", round: 1 });
    const st = Object.fromEntries(stages(g).map((s) => [s.id, s.state]));
    assert.equal(st.quick, "now");
    assert.equal(st.full, "wait");
    assert.equal(st.reviewer, undefined, "reviewer off is hidden");
    g.status = "working";
    g.running = null;
    g.checks[0].last = fail;
    g.checks[1].last = { ...fail, skipped: true };
    const st2 = Object.fromEntries(stages(g).map((s) => [s.id, s.state]));
    assert.equal(st2.agent, "now");
    assert.equal(st2.quick, "fail");
    assert.equal(st2.full, "wait");
  });

  it("shows the reviewer and the end state", () => {
    const g = goal({
      status: "met",
      round: 2,
      reviewer: { on: true, agent: "claude", model: "fable" },
      verdict: { approved: true, concerns: [], round: 2 },
    });
    g.checks[0].last = pass;
    g.checks[1].last = pass;
    const st = Object.fromEntries(stages(g).map((s) => [s.id, s.state]));
    assert.equal(st.reviewer, "done");
    assert.equal(st.met, "done");
    const p = goal({ status: "paused", paused: { reason: "rounds" } });
    const last = stages(p).at(-1);
    assert.equal(last.label, "Paused");
    assert.equal(last.detail, "round limit");
  });
});

describe("validateGoal", () => {
  it("accepts a complete goal", () => {
    assert.equal(validateGoal(goal()), "");
  });

  it("matches the server's sentences", () => {
    assert.equal(validateGoal(goal({ text: " " })), "The goal must be between 1 and 500 characters.");
    assert.equal(validateGoal(goal({ criteria: [] })), "Add between 1 and 8 done criteria.");
    const noCheck = goal({ criteria: [{ id: "k1", text: "A" }, { id: "k2", text: "B" }] });
    assert.equal(validateGoal(noCheck), '"B" has no check yet.');
    const g = goal();
    g.checks[0].command = "";
    assert.equal(validateGoal(g), 'The check "Unit" needs a command.');
    const t = goal();
    t.checks[0].timeout_s = 2;
    assert.match(validateGoal(t), /between 5 and 1800 seconds/);
    assert.equal(validateLimits({ rounds: 0 }), "The round limit must be between 1 and 10.");
    assert.equal(validateLimits({ rounds: 3, cost_usd: 0 }), "The cost limit must be between $0.01 and $1000.");
    assert.equal(validateLimits({ rounds: 3, minutes: 1.5 }), "The time limit must be between 1 and 1440 minutes.");
  });

  it("finds criteria with no check, ignoring blank ones", () => {
    const crit = [{ id: "k1", text: "A" }, { id: "k2", text: "B" }, { id: "k3", text: " " }];
    assert.deepEqual(uncovered(crit, [{ criterion: "k1" }]).map((k) => k.id), ["k2"]);
  });
});

describe("small helpers", () => {
  it("parses limits", () => {
    assert.equal(parseLimit(""), null);
    assert.equal(parseLimit("$2.50"), 2.5);
    assert.equal(parseLimit("45", { integer: true }), 45);
    assert.ok(Number.isNaN(parseLimit("abc")));
  });

  it("makes ids", () => {
    assert.equal(nextId("k", [{ id: "k1" }, { id: "k3" }]), "k2");
    assert.equal(nextId("c", []), "c1");
  });

  it("labels results", () => {
    assert.equal(resultLabel({ kind: "command", last: null }), "Not run yet");
    assert.equal(resultLabel({ kind: "command", last: { ...pass, tampered: true } }), "Changed");
    assert.equal(resultLabel({ kind: "command", last: { ...fail, timed_out: true } }), "Timed out");
    assert.equal(resultLabel({ kind: "manual", done: false }), "Waiting on you");
  });

  it("measures limits from where the goal started", () => {
    const g = goal({ limits: { rounds: 4, cost_usd: 2, minutes: 10 }, round: 1, cost_at_start: 0.5, working_ms: 60_000 });
    const m = limitMeters(g, { state: "working", state_since: 1000, usage: { cost_usd: 1.5 } }, 61_000);
    assert.equal(m.rounds.frac, 0.25);
    assert.equal(m.cost.used, 1);
    assert.equal(m.cost.frac, 0.5);
    assert.equal(m.time.used, 120_000);
    assert.equal(m.time.frac, 0.2);
  });

  it("summarises rounds newest first", () => {
    const rows = roundSummaries({
      rounds: [
        { round: 0, at: 1, results: [{ check: "c1", passed: false }] },
        { round: 1, at: 2, results: [{ check: "c1", passed: true }, { check: "c2", passed: false }] },
      ],
    });
    assert.deepEqual(rows.map((r) => r.label), ["Round 1", "Before start"]);
    assert.equal(rows[0].passed, 1);
    assert.equal(rows[0].total, 2);
  });
});

describe("queue and goals", () => {
  const base = { agent: "claude", snoozed: false, state_since: 1, blocked: null };
  it("keeps a session the goal is still driving out of Your move", () => {
    const driving = { ...base, id: "a", state: "ready", goal: { status: "checking" } };
    const paused = { ...base, id: "b", state: "ready", goal: { status: "paused" } };
    const met = { ...base, id: "c", state: "ready", goal: { status: "met" } };
    assert.deepEqual(queueSessions([driving, paused, met]).map((s) => s.id), ["b", "c"]);
  });

  it("alerts when a goal stops driving", () => {
    const prev = [{ ...base, id: "a", state: "ready", goal: { status: "checking" } }];
    const next = [{ ...base, id: "a", state: "ready", goal: { status: "met" } }];
    assert.deepEqual(diffQueue(prev, next), [{ id: "a", kind: "ready" }]);
    const turn = [{ ...base, id: "a", state: "working", goal: { status: "working" } }];
    const checking = [{ ...base, id: "a", state: "ready", goal: { status: "checking" } }];
    assert.deepEqual(diffQueue(turn, checking), []);
  });
});

describe("finish helpers", () => {
  it("says when a met goal was committed", () => {
    assert.equal(pillText(goal({ status: "met" })), "Goal met");
    assert.equal(pillText(goal({ status: "met", report: { committed: "abc1234" } })), "Goal met · committed");
  });

  it("lists flagged checks", () => {
    const g = goal();
    g.checks[1].last = { ...pass, tampered: true };
    assert.deepEqual(flaggedChecks(g).map((c) => c.id), ["c2"]);
    assert.deepEqual(flaggedChecks(null), []);
  });

  it("labels report files", () => {
    assert.equal(reportFilesLabel({ files: [] }), "No file changes against the base");
    assert.equal(
      reportFilesLabel({ files: [{ added: 3, removed: 1 }, { added: 2, removed: 0 }], more_files: 1 }),
      "3 files changed · +5 −1",
    );
    assert.equal(reportFilesLabel({ files: [{ added: 1, removed: 1 }] }), "1 file changed · +1 −1");
  });

  it("offers two more rounds, at most 10", () => {
    assert.equal(moreRounds(goal({ round: 2 })), 4);
    assert.equal(moreRounds(goal({ round: 9 })), 10);
  });
});
