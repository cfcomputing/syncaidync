import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { formatDuration } from "../js/lib/time.js";

describe("formatDuration", () => {
  it("formats common spans", () => {
    assert.equal(formatDuration(45_000), "45s");
    assert.equal(formatDuration(3 * 60_000), "3m");
    assert.equal(formatDuration((1 * 60 + 12) * 60_000), "1h 12m");
    assert.equal(formatDuration((2 * 24 + 4) * 60 * 60_000), "2d 4h");
  });

  it("treats zero or negative as 0s", () => {
    assert.equal(formatDuration(0), "0s");
    assert.equal(formatDuration(-10), "0s");
    assert.equal(formatDuration(NaN), "0s");
  });
});
