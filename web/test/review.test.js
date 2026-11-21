import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  isReviewed,
  progress,
  toggleReviewed,
  addNote,
  removeNote,
  unsent,
  markSent,
} from "../js/lib/review.js";

const file = { path: "a.ts", digest: "abc" };

describe("reviewed + progress", () => {
  it("clears when digest changes", () => {
    let review = { reviewed: { "a.ts": "abc" }, notes: [], overall: "" };
    assert.equal(isReviewed(review, file), true);
    assert.equal(isReviewed(review, { path: "a.ts", digest: "zzz" }), false);
    review = toggleReviewed(review, file);
    assert.equal(isReviewed(review, file), false);
    review = toggleReviewed(review, file);
    assert.equal(isReviewed(review, file), true);
    const p = progress(review, [file, { path: "b.ts", digest: "1" }]);
    assert.deepEqual(p, { reviewed: 1, total: 2 });
  });
});

describe("notes helpers", () => {
  it("add remove unsent markSent", () => {
    let review = { reviewed: {}, notes: [], overall: "" };
    review = addNote(review, { id: "1", path: "a.ts", text: "hi", sent: false });
    review = addNote(review, { id: "2", path: "a.ts", text: "yo", sent: false });
    assert.equal(unsent(review).length, 2);
    review = markSent(review, ["1"]);
    assert.equal(unsent(review).length, 1);
    review = removeNote(review, "2");
    assert.equal(review.notes.length, 1);
  });
});
