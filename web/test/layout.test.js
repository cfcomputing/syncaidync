import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  orderSessions,
  boardOrder,
  rowsLayout,
  gridLayout,
  freeLayout,
  nextFreeSlot,
  clampRect,
  raise,
  tidy,
  STRIP_H,
} from "../js/lib/layout.js";

function s(partial) {
  return {
    id: partial.id,
    state: partial.state ?? "working",
    snoozed: partial.snoozed ?? false,
    state_since: partial.state_since ?? 0,
    created_at: partial.created_at ?? 0,
  };
}

describe("orderSessions", () => {
  it("orders blocked, ready, working, launching, ended, snoozed last; ties by age", () => {
    const sessions = [
      s({ id: "ended", state: "ended", created_at: 1 }),
      s({ id: "ready-new", state: "ready", state_since: 200, created_at: 9 }),
      s({ id: "blocked-old", state: "blocked", state_since: 50, created_at: 5 }),
      s({ id: "working", state: "working", created_at: 3 }),
      s({ id: "launching", state: "launching", created_at: 4 }),
      s({ id: "ready-old", state: "ready", state_since: 100, created_at: 8 }),
      s({ id: "snoozed", state: "blocked", snoozed: true, created_at: 2 }),
      s({ id: "blocked-new", state: "blocked", state_since: 150, created_at: 6 }),
    ];
    assert.deepEqual(
      orderSessions(sessions).map((x) => x.id),
      [
        "blocked-old",
        "blocked-new",
        "ready-old",
        "ready-new",
        "working",
        "launching",
        "ended",
        "snoozed",
      ],
    );
  });
});

describe("boardOrder", () => {
  it("keeps cards in age order whatever their state; snoozed last", () => {
    const sessions = [
      s({ id: "ended", state: "ended", created_at: 1 }),
      s({ id: "ready", state: "ready", created_at: 9 }),
      s({ id: "blocked", state: "blocked", created_at: 5 }),
      s({ id: "working", state: "working", created_at: 3 }),
      s({ id: "snoozed", state: "blocked", snoozed: true, created_at: 2 }),
    ];
    assert.deepEqual(
      boardOrder(sessions).map((x) => x.id),
      ["ended", "working", "blocked", "ready", "snoozed"],
    );
  });
});

describe("rowsLayout", () => {
  it("orders, uses strip height, clamps card height", () => {
    const sessions = [
      s({ id: "a", state: "working", created_at: 1 }),
      s({ id: "b", state: "blocked", state_since: 1, created_at: 2 }),
      s({ id: "c", state: "ready", snoozed: true }),
    ];
    const small = rowsLayout({
      width: 800,
      viewportHeight: 400,
      sessions,
      collapsed: ["b"],
    });
    assert.equal(small.items[0].id, "a");
    assert.equal(small.items[0].w, 800);
    assert.equal(small.items[0].h, 260); // clamp low
    assert.equal(small.items[1].id, "b");
    assert.equal(small.items[1].h, STRIP_H);
    assert.equal(small.items[1].collapsed, true);
    assert.equal(small.items[2].h, STRIP_H);

    const mid = rowsLayout({ width: 800, viewportHeight: 800, sessions: [s({ id: "x" })] });
    assert.equal(mid.items[0].h, Math.round(800 * 0.42));

    const large = rowsLayout({ width: 800, viewportHeight: 2000, sessions: [s({ id: "x" })] });
    assert.equal(large.items[0].h, 380);
  });
});

describe("gridLayout", () => {
  it("picks columns at key widths and shortest-column placement", () => {
    const many = Array.from({ length: 6 }, (_, i) =>
      s({ id: `s${i}`, state: "working", created_at: i }),
    );
    const at = (width) =>
      gridLayout({ width, viewportHeight: 900, sessions: many, collapsed: [] }).items;

    assert.equal(new Set(at(300).map((i) => i.x)).size, 1);
    assert.ok(new Set(at(900).map((i) => i.x)).size >= 1);
    assert.ok(new Set(at(1400).map((i) => i.x)).size >= 2);
    assert.ok(new Set(at(2600).map((i) => i.x)).size >= 3);

    const few = gridLayout({
      width: 1400,
      viewportHeight: 900,
      sessions: [s({ id: "only" })],
    });
    assert.equal(few.items.length, 1);
    assert.ok(few.items[0].w >= 320 && few.items[0].w <= 960);

    const mixed = gridLayout({
      width: 1000,
      viewportHeight: 900,
      sessions: [
        s({ id: "a", created_at: 1 }),
        s({ id: "b", created_at: 2 }),
        s({ id: "c", snoozed: true, created_at: 3 }),
      ],
      collapsed: [],
    });
    const strip = mixed.items.find((i) => i.id === "c");
    assert.equal(strip.h, STRIP_H);
  });
});

describe("freeLayout", () => {
  it("default slots, clamp, raise, tidy, collapsed keeps saved height", () => {
    const sessions = [
      s({ id: "a", created_at: 1 }),
      s({ id: "b", created_at: 2 }),
      s({ id: "c", created_at: 3 }),
    ];
    const first = freeLayout({ width: 800, sessions, free: {} });
    assert.equal(first.items[0].x, 24);
    assert.equal(first.items[0].y, 24);
    assert.equal(first.items[0].w, 560);
    assert.equal(first.items[1].x, 24 + 560 + 16);

    const clamped = clampRect({ id: "z", x: -10, y: 99999, w: 10, h: 10, z: 1 });
    assert.equal(clamped.x, 0);
    assert.equal(clamped.y, 5000);
    assert.equal(clamped.w, 320);
    assert.equal(clamped.h, 240);

    let rects = [
      { id: "a", x: 0, y: 0, w: 400, h: 300, z: 1 },
      { id: "b", x: 10, y: 10, w: 400, h: 300, z: 2 },
    ];
    rects = raise(rects, "a");
    assert.equal(rects.find((r) => r.id === "a").z, 3);

    rects = [
      { id: "a", x: 0, y: 0, w: 400, h: 300, z: 10001 },
      { id: "b", x: 10, y: 10, w: 400, h: 300, z: 10002 },
    ];
    rects = raise(rects, "a");
    assert.ok(rects.every((r) => r.z <= 10000));
    assert.equal(rects.find((r) => r.id === "a").z, 2);

    const slot = nextFreeSlot([], 800);
    assert.deepEqual(slot, { x: 24, y: 24, w: 560, h: 380, z: 1 });

    const withSaved = freeLayout({
      width: 1200,
      sessions: [s({ id: "a" })],
      collapsed: ["a"],
      free: { a: { x: 40, y: 50, w: 500, h: 400, z: 2, savedH: 400 } },
    });
    assert.equal(withSaved.items[0].x, 40);
    assert.equal(withSaved.items[0].y, 50);
    assert.equal(withSaved.items[0].w, 500);
    assert.equal(withSaved.items[0].h, STRIP_H);
    assert.equal(withSaved.items[0].savedH, 400);

    const tidied = tidy({
      width: 1200,
      viewportHeight: 900,
      sessions,
      free: {
        a: { x: 900, y: 900, w: 500, h: 400, z: 1 },
        b: { x: 10, y: 10, w: 500, h: 400, z: 2 },
        c: { x: 20, y: 20, w: 500, h: 400, z: 3 },
      },
    });
    const grid = gridLayout({
      width: 1200,
      viewportHeight: 900,
      sessions,
    });
    assert.equal(tidied.items.length, grid.items.length);
    // tidy positions come from grid
    for (const g of grid.items) {
      const t = tidied.items.find((i) => i.id === g.id);
      assert.equal(t.x, g.x);
      assert.equal(t.y, g.y);
    }
  });
});
