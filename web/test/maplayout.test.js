import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { layoutMap, providersFirst } from "../js/lib/maplayout.js";

describe("layoutMap", () => {
  it("layers a chain", () => {
    const layout = layoutMap({
      nodes: [
        { path: "a.ts", changed: true },
        { path: "b.ts", changed: true },
        { path: "c.ts", changed: false },
      ],
      // c imports b imports a  => edges from→to means from imports to
      edges: [
        { from: "b.ts", to: "a.ts" },
        { from: "c.ts", to: "b.ts" },
      ],
    });
    const by = Object.fromEntries(layout.nodes.map((n) => [n.path, n.layer]));
    assert.equal(by["a.ts"], 0);
    assert.equal(by["b.ts"], 1);
    assert.equal(by["c.ts"], 2);
  });

  it("handles a diamond and a cycle without hanging", () => {
    const diamond = layoutMap({
      nodes: [
        { path: "a", changed: true },
        { path: "b", changed: true },
        { path: "c", changed: true },
        { path: "d", changed: true },
      ],
      edges: [
        { from: "b", to: "a" },
        { from: "c", to: "a" },
        { from: "d", to: "b" },
        { from: "d", to: "c" },
      ],
    });
    assert.equal(diamond.tooLarge, undefined);
    assert.ok(diamond.nodes.find((n) => n.path === "a").layer === 0);

    const cycle = layoutMap({
      nodes: [
        { path: "x", changed: true },
        { path: "y", changed: true },
      ],
      edges: [
        { from: "x", to: "y" },
        { from: "y", to: "x" },
      ],
    });
    assert.equal(cycle.nodes.length, 2);
  });

  it("lays out large maps, wrapping a tall layer into columns", () => {
    const nodes = Array.from({ length: 240 }, (_, i) => ({
      path: `f${String(i).padStart(3, "0")}.ts`,
      changed: true,
    }));
    const layout = layoutMap({ nodes, edges: [] });
    assert.equal(layout.tooLarge, undefined);
    assert.equal(layout.nodes.length, 240);
    const xs = new Set(layout.nodes.map((n) => n.x));
    const ys = new Set(layout.nodes.map((n) => n.y));
    assert.equal(xs.size, 14); // ceil(240 / 18) columns
    assert.ok(ys.size <= 18);
    const spots = new Set(layout.nodes.map((n) => `${n.x},${n.y}`));
    assert.equal(spots.size, 240); // no overlaps
  });
});

describe("providersFirst", () => {
  it("orders by layer", () => {
    const files = [{ path: "c.ts" }, { path: "a.ts" }, { path: "b.ts" }];
    const ordered = providersFirst(files, {
      nodes: files.map((f) => ({ ...f, changed: true })),
      edges: [
        { from: "b.ts", to: "a.ts" },
        { from: "c.ts", to: "b.ts" },
      ],
    });
    assert.deepEqual(
      ordered.map((f) => f.path),
      ["a.ts", "b.ts", "c.ts"],
    );
  });
});
