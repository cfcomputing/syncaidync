import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { stateBadge, progressLabel, sortPulls } from "../js/lib/pulls.js";

describe("stateBadge", () => {
  it("covers open, draft, merged, closed", () => {
    assert.equal(stateBadge({ state: "open", draft: false }), "Open");
    assert.equal(stateBadge({ state: "open", draft: true }), "Draft");
    assert.equal(stateBadge({ state: "merged" }), "Merged");
    assert.equal(stateBadge({ state: "closed", draft: true }), "Closed");
  });
});

describe("progressLabel", () => {
  it("formats reviewed of files", () => {
    assert.equal(progressLabel({ files: 12, reviewed: 4 }), "4 of 12 files");
    assert.equal(progressLabel(null), "0 of 0 files");
  });
});

describe("sortPulls", () => {
  it("orders active first, then most recently added", () => {
    const sorted = sortPulls([
      { id: "1", status: "done", added_at: 30 },
      { id: "2", status: "active", added_at: 10 },
      { id: "3", status: "active", added_at: 40 },
      { id: "4", status: "done", added_at: 50 },
    ]);
    assert.deepEqual(
      sorted.map((p) => p.id),
      ["3", "2", "4", "1"],
    );
  });

  it("accepts a Map", () => {
    const m = new Map([
      ["a", { id: "a", status: "done", added_at: 1 }],
      ["b", { id: "b", status: "active", added_at: 2 }],
    ]);
    assert.equal(sortPulls(m)[0].id, "b");
  });
});
