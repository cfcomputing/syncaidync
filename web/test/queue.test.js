import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { queueSessions, queueCounts, diffQueue } from "../js/lib/queue.js";

function s(partial) {
  return {
    id: partial.id,
    agent: partial.agent ?? "claude",
    state: partial.state,
    snoozed: partial.snoozed ?? false,
    state_since: partial.state_since ?? 0,
    blocked: partial.blocked ?? null,
  };
}

describe("queueSessions", () => {
  it("orders blocked before ready, longest wait first, excludes snoozed", () => {
    const sessions = [
      s({ id: "r-new", state: "ready", state_since: 200 }),
      s({ id: "b-old", state: "blocked", state_since: 50 }),
      s({ id: "working", state: "working", state_since: 10 }),
      s({ id: "r-old", state: "ready", state_since: 100 }),
      s({ id: "b-new", state: "blocked", state_since: 150 }),
      s({ id: "snoozed", state: "blocked", snoozed: true, state_since: 1 }),
      s({ id: "ended", state: "ended", state_since: 1 }),
    ];
    assert.deepEqual(
      queueSessions(sessions).map((x) => x.id),
      ["b-old", "b-new", "r-old", "r-new"],
    );
  });

  it("leaves out a shell at its prompt but keeps a shell asking something", () => {
    const sessions = [
      s({ id: "idle-shell", agent: "shell", state: "ready", state_since: 1 }),
      s({ id: "asking-shell", agent: "shell", state: "blocked", state_since: 2 }),
      s({ id: "agent", state: "ready", state_since: 3 }),
    ];
    assert.deepEqual(
      queueSessions(sessions).map((x) => x.id),
      ["asking-shell", "agent"],
    );
    assert.deepEqual(queueCounts(sessions), { total: 2, blocked: 1, ready: 1 });
    const prev = sessions.map((x) => ({ ...x, state: "working" }));
    assert.deepEqual(
      diffQueue(prev, sessions).map((d) => d.id).sort(),
      ["agent", "asking-shell"],
    );
  });

  it("accepts a Map", () => {
    const map = new Map([
      ["a", s({ id: "a", state: "ready", state_since: 2 })],
      ["b", s({ id: "b", state: "blocked", state_since: 1 })],
    ]);
    assert.deepEqual(
      queueSessions(map).map((x) => x.id),
      ["b", "a"],
    );
  });
});

describe("queueCounts", () => {
  it("counts blocked and ready", () => {
    const sessions = [
      s({ id: "1", state: "blocked" }),
      s({ id: "2", state: "blocked" }),
      s({ id: "3", state: "ready" }),
      s({ id: "4", state: "working" }),
      s({ id: "5", state: "ready", snoozed: true }),
    ];
    assert.deepEqual(queueCounts(sessions), { total: 3, blocked: 2, ready: 1 });
  });
});

describe("diffQueue", () => {
  it("yields nothing on the first snapshot", () => {
    assert.deepEqual(
      diffQueue(null, [s({ id: "a", state: "blocked" })]),
      [],
    );
  });

  it("alerts blocked from any state, ready only from working", () => {
    const prev = [
      s({ id: "a", state: "working" }),
      s({ id: "b", state: "launching" }),
      s({ id: "c", state: "ready" }),
      s({ id: "d", state: "ended" }),
      s({ id: "e", state: "blocked", blocked: { kind: "approval" } }),
    ];
    const next = [
      s({ id: "a", state: "ready" }),
      s({ id: "b", state: "ready" }),
      s({ id: "c", state: "ready" }),
      s({ id: "d", state: "blocked", blocked: { kind: "question" } }),
      s({ id: "e", state: "blocked", blocked: { kind: "approval" } }),
    ];
    assert.deepEqual(diffQueue(prev, next), [
      { id: "a", kind: "ready" },
      { id: "d", kind: "blocked" },
    ]);
  });

  it("never alerts snoozed sessions", () => {
    const prev = [s({ id: "a", state: "working", snoozed: true })];
    const next = [s({ id: "a", state: "blocked", snoozed: true })];
    assert.deepEqual(diffQueue(prev, next), []);
  });

  it("alerts again when blocked kind changes", () => {
    const prev = [
      s({ id: "a", state: "blocked", blocked: { kind: "approval", message: "rm" } }),
    ];
    const next = [
      s({ id: "a", state: "blocked", blocked: { kind: "question", message: "?" } }),
    ];
    assert.deepEqual(diffQueue(prev, next), [{ id: "a", kind: "blocked" }]);
  });

  it("covers every state pair into blocked or ready", () => {
    const states = ["launching", "working", "blocked", "ready", "ended"];
    for (const from of states) {
      for (const to of ["blocked", "ready"]) {
        const prev = [s({ id: "x", state: from, blocked: { kind: "approval" } })];
        const next = [s({ id: "x", state: to, blocked: { kind: "approval" } })];
        const diffs = diffQueue(prev, next);
        if (to === "blocked") {
          if (from === "blocked") assert.deepEqual(diffs, []);
          else assert.deepEqual(diffs, [{ id: "x", kind: "blocked" }]);
        } else if (to === "ready") {
          if (from === "working") assert.deepEqual(diffs, [{ id: "x", kind: "ready" }]);
          else assert.deepEqual(diffs, []);
        }
      }
    }
  });
});
