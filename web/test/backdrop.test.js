import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { buildMesh, meshLayers, rng } from "../js/lib/backdrop.js";

describe("backdrop mesh", () => {
  it("is deterministic for a seed and differs between seeds", () => {
    assert.deepEqual(meshLayers(7), meshLayers(7));
    assert.notEqual(meshLayers(7).lines, meshLayers(8).lines);
    const a = rng(1);
    const b = rng(1);
    assert.equal(a(), b());
  });

  it("covers the view with two triangles per cell and no gaps", () => {
    const m = buildMesh(3);
    assert.equal(m.tris.length, m.rows * m.cols * 2);
    // Edges: horizontal + vertical + one diagonal per cell.
    const expected = (m.rows + 1) * m.cols + m.rows * (m.cols + 1) + m.rows * m.cols;
    assert.equal(m.lines.length, expected);
    // The overhang reaches past every edge of the 1600×1000 view.
    const xs = m.pts.flat().map((p) => p[0]);
    const ys = m.pts.flat().map((p) => p[1]);
    assert.ok(Math.min(...xs) < 0 && Math.max(...xs) > 1600);
    assert.ok(Math.min(...ys) < 0 && Math.max(...ys) > 1000);
  });

  it("keeps each point inside its own cell's jitter box, so triangles never fold", () => {
    const m = buildMesh(11);
    m.pts.forEach((row, r) =>
      row.forEach(([x, y], c) => {
        assert.ok(Math.abs(x - (c - 1) * 96) <= 0.38 * 96 + 0.1);
        assert.ok(Math.abs(y - (r - 1) * 96) <= 0.38 * 96 + 0.1);
      }),
    );
  });

  it("never flips a triangle or leaves a sliver, across many seeds", () => {
    for (let seed = 0; seed < 200; seed++) {
      const m = buildMesh(seed);
      for (const [a, b, c] of m.tris) {
        const cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        // Points listed clockwise on screen (y down) give a positive cross product.
        assert.ok(cross > 96 * 96 * 0.05, `seed ${seed}: folded triangle ${JSON.stringify([a, b, c])}`);
      }
    }
  });

  it("picks a few glints and seven sparks", () => {
    const m = buildMesh(5);
    assert.ok(m.glintTris.length > 0 && m.glintTris.length < m.tris.length / 8);
    assert.equal(m.sparks.length, 7);
  });
});
