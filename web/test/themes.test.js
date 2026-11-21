import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  parseHex,
  contrast,
  mix,
  pickOnAccent,
  readableInk,
} from "../js/lib/themes.js";

describe("parseHex", () => {
  it("accepts 3 and 6 digit hex", () => {
    assert.deepEqual(parseHex("#abc"), [170, 187, 204]);
    assert.deepEqual(parseHex("#a78bfa"), [167, 139, 250]);
  });
  it("rejects bad input", () => {
    assert.equal(parseHex("a78bfa"), null);
    assert.equal(parseHex("#gg0000"), null);
    assert.equal(parseHex("#ab"), null);
  });
});

describe("contrast", () => {
  it("black/white is 21", () => {
    assert.ok(Math.abs(contrast("#000000", "#ffffff") - 21) < 0.05);
  });
});

describe("pickOnAccent", () => {
  it("picks dark ink on light accent and light ink on dark accent", () => {
    assert.equal(pickOnAccent("#f5e6ff"), "#0b0716");
    assert.equal(pickOnAccent("#3b1d6e"), "#ffffff");
  });
});

describe("readableInk", () => {
  it("reaches 4.5 or stops after 10 steps", () => {
    const ink = readableInk("#a78bfa", "#06060b", "#e8e8ed");
    assert.ok(contrast(ink, "#06060b") >= 4.5 || ink !== "#a78bfa");
    // Very low-contrast yellow on white needs mixing toward text.
    const mixed = readableInk("#f5f5a0", "#ffffff", "#17152a");
    assert.ok(contrast(mixed, "#ffffff") >= 4.5 || mixed === mix("#f5f5a0", "#17152a", 1));
  });
});
