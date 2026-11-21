import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  byStartLine,
  quoteLines,
  relocateMargin,
  relocateAll,
} from "../js/lib/margins.js";

describe("byStartLine", () => {
  it("groups by start", () => {
    const map = byStartLine([
      { id: "a", start: 3 },
      { id: "b", start: 3 },
      { id: "c", start: 10 },
    ]);
    assert.equal(map.get(3).length, 2);
    assert.equal(map.get(10).length, 1);
  });
});

describe("quoteLines", () => {
  it("takes inclusive 1-based range capped at 6 lines", () => {
    const text = ["a", "b", "c", "d", "e", "f", "g", "h"].join("\n");
    assert.equal(quoteLines(text, 2, 4), "b\nc\nd");
    assert.equal(quoteLines(text, 1, 20).split("\n").length, 6);
  });
});

describe("relocateMargin", () => {
  const text = ["keep", "fn foo() {", "  return 1;", "}", "end"].join("\n");

  it("stays ok when quote still at the same lines", () => {
    const r = relocateMargin(text, {
      start: 2,
      end: 3,
      quote: "fn foo() {\n  return 1;",
    });
    assert.equal(r.status, "ok");
    assert.equal(r.start, 2);
  });

  it("moves when the quote slid down", () => {
    const moved = ["intro", "keep", "fn foo() {", "  return 1;", "}", "end"].join(
      "\n",
    );
    const r = relocateMargin(moved, {
      start: 2,
      end: 3,
      quote: "fn foo() {\n  return 1;",
    });
    assert.equal(r.status, "moved");
    assert.equal(r.start, 3);
    assert.equal(r.end, 4);
  });

  it("marks outdated when the quote is gone", () => {
    const r = relocateMargin("nothing here\n", {
      start: 2,
      end: 3,
      quote: "fn foo() {\n  return 1;",
    });
    assert.equal(r.status, "outdated");
  });

  it("moves when only whitespace drifted", () => {
    const spaced = ["keep", "fn foo() {", "\treturn 1;", "}", "end"].join("\n");
    const r = relocateMargin(spaced, {
      start: 2,
      end: 3,
      quote: "fn foo() {\n  return 1;",
    });
    assert.equal(r.status, "ok");
    assert.equal(r.start, 2);
  });

  it("fuzzy-moves when a line was lightly edited", () => {
    const edited = ["keep", "fn foo() {", "  return 2;", "}", "end"].join("\n");
    const r = relocateMargin(edited, {
      start: 2,
      end: 3,
      quote: "fn foo() {\n  return 1;",
    });
    assert.equal(r.status, "moved");
    assert.equal(r.start, 2);
    assert.equal(r.end, 3);
  });

  it("stays outdated when two fuzzy hits tie", () => {
    const dup = [
      "fn foo() {",
      "  return 1;",
      "}",
      "fn foo() {",
      "  return 1;",
      "}",
    ].join("\n");
    const r = relocateMargin(dup, {
      start: 10,
      end: 11,
      quote: "fn foo() {\n  return 9;",
    });
    assert.equal(r.status, "outdated");
  });
});

describe("relocateAll", () => {
  it("collects moved entries", () => {
    const text = ["x", "target", "y"].join("\n");
    const { margins, moved } = relocateAll(text, [
      { id: "a", start: 1, end: 1, quote: "target" },
    ]);
    assert.equal(moved.length, 1);
    assert.equal(margins[0].start, 2);
    assert.equal(margins[0].status, "moved");
  });
});
