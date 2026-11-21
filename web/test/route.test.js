import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { parse, format } from "../js/lib/route.js";

const cases = [
  { hash: "#/", route: { name: "board", id: null, tool: null, solo: false } },
  {
    hash: "#/s/abc",
    route: { name: "session", id: "abc", tool: null, solo: false },
  },
  {
    hash: "#/s/abc/terminal?solo=1",
    route: { name: "session", id: "abc", tool: "terminal", solo: true },
  },
  {
    hash: "#/s/abc/files",
    route: { name: "session", id: "abc", tool: "files", solo: false },
  },
  { hash: "#/pulls", route: { name: "pulls", id: null, tool: null, solo: false } },
  {
    hash: "#/p/42",
    route: { name: "pull", id: "42", tool: null, solo: false },
  },
];

describe("route.parse / format", () => {
  for (const { hash, route } of cases) {
    it(`parses and formats ${hash}`, () => {
      assert.deepEqual(parse(hash), route);
      assert.equal(format(route), hash);
      assert.deepEqual(parse(format(route)), route);
    });
  }

  it("empty and bare hashes parse as the board", () => {
    const board = { name: "board", id: null, tool: null, solo: false };
    assert.deepEqual(parse(""), board);
    assert.deepEqual(parse("#"), board);
    assert.equal(format(board), "#/");
  });

  it("junk hashes parse as the board", () => {
    const board = { name: "board", id: null, tool: null, solo: false };
    assert.deepEqual(parse("#/nope"), board);
    assert.deepEqual(parse("#/s"), board);
    assert.deepEqual(parse("#/weird/path"), board);
  });
});
