import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  defaultLayout,
  moveTab,
  tearOut,
  tidy,
  normalize,
  setActive,
  setOpacity,
  TOOLS,
} from "../js/lib/tabgroups.js";

describe("defaultLayout", () => {
  it("gives the review diff more room while keeping Terminal visible", () => {
    const { groups: [terminal, changes] } = defaultLayout(1200, 800, { review: true });
    assert.equal(terminal.active, "terminal");
    assert.equal(changes.active, "changes");
    assert.ok(changes.rect.w > terminal.rect.w);
    assert.ok(terminal.rect.x + terminal.rect.w < changes.rect.x);
  });
  it("uses ~58% left for terminal and Changes active on the right", () => {
    const layout = defaultLayout(1200, 800);
    assert.equal(layout.groups.length, 2);
    const [left, right] = layout.groups;
    assert.deepEqual(left.tabs, ["terminal"]);
    assert.equal(left.active, "terminal");
    assert.deepEqual(right.tabs, ["files", "changes", "session"]);
    assert.equal(right.active, "changes");
    const inner = 1200 - 32;
    const expectedLeft = Math.round(inner * 0.58);
    assert.equal(left.rect.w, expectedLeft);
    assert.ok(Math.abs(left.rect.w / (left.rect.w + 16 + right.rect.w) - 0.58) < 0.02);
  });
});

describe("moveTab", () => {
  it("reorders within a group and moves across groups", () => {
    let layout = defaultLayout(1200, 800);
    layout = moveTab(layout, "session", "g2", 0);
    assert.deepEqual(layout.groups.find((g) => g.id === "g2").tabs, [
      "session",
      "files",
      "changes",
    ]);
    assert.equal(layout.groups.find((g) => g.id === "g2").active, "session");

    layout = moveTab(layout, "files", "g1", 1);
    const g1 = layout.groups.find((g) => g.id === "g1");
    const g2 = layout.groups.find((g) => g.id === "g2");
    assert.deepEqual(g1.tabs, ["terminal", "files"]);
    assert.deepEqual(g2.tabs, ["session", "changes"]);
  });
});

describe("tearOut", () => {
  it("creates a new group and tidy drops emptied groups", () => {
    let layout = defaultLayout(1200, 800);
    layout = tearOut(layout, "changes", { x: 100, y: 80, w: 520, h: 360 });
    assert.equal(layout.groups.length, 3);
    const torn = layout.groups.find((g) => g.tabs.length === 1 && g.tabs[0] === "changes");
    assert.ok(torn);
    assert.equal(torn.rect.x, 100);
    assert.equal(torn.active, "changes");

    // Move last tab out of a group → empty group dropped
    layout = moveTab(layout, "changes", "g1", 1);
    assert.ok(!layout.groups.find((g) => g.tabs.length === 0));
    assert.ok(layout.groups.every((g) => g.tabs.includes(g.active)));
  });
});

describe("normalize", () => {
  it("repairs junk input into a valid layout", () => {
    assert.equal(normalize(null).groups.length, 2);
    assert.equal(normalize(42).groups.length, 2);
    assert.equal(normalize({}).groups.length, 2);

    const fixed = normalize({
      groups: [
        {
          id: "a",
          tabs: ["terminal", "terminal", "bogus", "files"],
          active: "nope",
          rect: { x: -50, y: 99999, w: 10, h: 10, z: 1 },
          opacity: 9,
        },
        { id: "b", tabs: [], active: "files" },
      ],
    });
    const tools = fixed.groups.flatMap((g) => g.tabs).sort();
    assert.deepEqual(tools, [...TOOLS].sort());
    assert.ok(fixed.groups.every((g) => g.tabs.includes(g.active)));
    const a = fixed.groups.find((g) => g.id === "a");
    assert.equal(a.rect.x, 0);
    assert.equal(a.rect.y, 5000);
    assert.equal(a.rect.w, 320);
    assert.equal(a.rect.h, 240);
    assert.equal(a.opacity, 1);
    // missing changes + session appended to last group
    assert.ok(fixed.groups.some((g) => g.tabs.includes("changes")));
    assert.ok(fixed.groups.some((g) => g.tabs.includes("session")));
  });
});

describe("setActive / tidy", () => {
  it("repairs active when tabs change", () => {
    let layout = defaultLayout(1200, 800);
    layout = setActive(layout, "g2", "files");
    assert.equal(layout.groups.find((g) => g.id === "g2").active, "files");
    layout = tidy({
      groups: [
        { id: "g1", tabs: ["terminal"], active: "files", rect: { x: 0, y: 0, w: 400, h: 300, z: 1 }, opacity: 1 },
      ],
    });
    assert.equal(layout.groups[0].active, "terminal");
  });
});

describe("setOpacity", () => {
  it("sets active and inactive opacity separately, clamped, and keeps them through normalize", () => {
    let layout = defaultLayout(1200, 800);
    assert.equal(layout.groups[0].idleOpacity, 1);
    layout = setOpacity(layout, "g1", 0.8);
    layout = setOpacity(layout, "g1", undefined, 0.05);
    const g = layout.groups.find((x) => x.id === "g1");
    assert.equal(g.opacity, 0.8);
    assert.equal(g.idleOpacity, 0.2);
    const again = normalize(JSON.parse(JSON.stringify(layout)));
    assert.equal(again.groups.find((x) => x.id === "g1").idleOpacity, 0.2);
  });
});
