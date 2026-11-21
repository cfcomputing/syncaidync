import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { composeMessage } from "../js/lib/notes.js";

describe("composeMessage", () => {
  it("formats single line, range, old side, file note, overall, quote cut", () => {
    const msg = composeMessage(
      [
        {
          path: "src/auth/redirect.ts",
          side: "new",
          start: 11,
          end: 11,
          quote: "  return go(safe(next));",
          text: "What does safe() do with an absolute URL?",
        },
        {
          path: "src/x.ts",
          side: "new",
          start: 11,
          end: 14,
          quote: "a\nb\nc\nd\ne\nf\ng\nh",
          text: "range",
        },
        {
          path: "src/old.ts",
          side: "old",
          start: 11,
          end: 11,
          quote: "before",
          text: "was this",
        },
        {
          path: "src/pages/login.tsx",
          side: "new",
          start: null,
          end: null,
          quote: "",
          text: "This still calls the old helper.",
        },
      ],
      "Please add a test for the absolute URL case.",
    );
    assert.match(msg, /Review notes on your changes \(4\):/);
    assert.match(msg, /1\. src\/auth\/redirect\.ts, line 11/);
    assert.match(msg, /2\. src\/x\.ts, lines 11–14/);
    assert.match(msg, /\(2 more lines\)/);
    assert.match(msg, /3\. src\/old\.ts, line 11 \(before\)/);
    assert.match(msg, /4\. src\/pages\/login\.tsx \(whole file\)/);
    assert.match(msg, /Overall: Please add a test/);
  });

  it("overall only", () => {
    assert.equal(composeMessage([], "Just this"), "Overall: Just this");
  });
});
