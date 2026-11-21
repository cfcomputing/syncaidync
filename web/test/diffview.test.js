import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { unifiedRows, splitRows, lineKey } from "../js/lib/diffview.js";

describe("unifiedRows", () => {
  it("keeps signs and numbers", () => {
    const rows = unifiedRows({
      lines: [
        { kind: "context", old: 1, new: 1, text: "a" },
        { kind: "remove", old: 2, new: null, text: "b" },
        { kind: "add", old: null, new: 2, text: "c" },
      ],
    });
    assert.equal(rows[1].sign, "−");
    assert.equal(rows[2].sign, "+");
    assert.equal(rows[2].new, 2);
  });
});

describe("splitRows", () => {
  it("pairs unequal remove/add runs", () => {
    const rows = splitRows({
      lines: [
        { kind: "remove", old: 1, text: "a" },
        { kind: "remove", old: 2, text: "b" },
        { kind: "add", new: 1, text: "c" },
      ],
    });
    assert.equal(rows.length, 2);
    assert.equal(rows[0].left.text, "a");
    assert.equal(rows[0].right.text, "c");
    assert.equal(rows[1].left.text, "b");
    assert.equal(rows[1].right, null);
  });
});

describe("lineKey", () => {
  it("joins side and number", () => {
    assert.equal(lineKey("new", 11), "new:11");
  });
});
