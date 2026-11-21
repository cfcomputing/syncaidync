import { describe, it, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";

function withStorage(impl, fn) {
  const prev = globalThis.localStorage;
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: impl,
  });
  return Promise.resolve()
    .then(fn)
    .finally(() => {
      if (prev === undefined) delete globalThis.localStorage;
      else
        Object.defineProperty(globalThis, "localStorage", {
          configurable: true,
          value: prev,
        });
    });
}

describe("prefs", () => {
  it("falls back when storage throws", async () => {
    await withStorage(
      {
        getItem() {
          throw new Error("blocked");
        },
        setItem() {
          throw new Error("full");
        },
        removeItem() {
          throw new Error("blocked");
        },
      },
      async () => {
        const prefs = await import("../js/lib/prefs.js");
        assert.equal(prefs.get("appearance", "auto"), "auto");
        assert.equal(prefs.set("appearance", "day"), false);
      },
    );
  });

  it("falls back on bad JSON and wrong shape", async () => {
    const map = new Map();
    await withStorage(
      {
        getItem(k) {
          return map.has(k) ? map.get(k) : null;
        },
        setItem(k, v) {
          map.set(k, String(v));
        },
        removeItem(k) {
          map.delete(k);
        },
      },
      async () => {
        // Fresh import is not required; module is cached but uses localStorage at call time.
        const prefs = await import("../js/lib/prefs.js");
        map.set("syncaidync:appearance", "{not json");
        assert.equal(prefs.get("appearance", "night"), "night");
        map.set("syncaidync:appearance", JSON.stringify("neon"));
        assert.equal(prefs.get("appearance", "night"), "night");
        assert.equal(prefs.set("appearance", "day"), true);
        assert.equal(prefs.get("appearance", "night"), "day");
        map.set("syncaidync:board", JSON.stringify({ layout: "nope" }));
        assert.deepEqual(prefs.get("board", { layout: "rows", collapsed: [], free: {} }), {
          layout: "rows",
          collapsed: [],
          free: {},
        });
        map.set("syncaidync:workspace-seed", JSON.stringify({ nope: true }));
        assert.equal(prefs.get("workspace-seed", null), null);
        const seed = { groups: [{ id: "g1", tabs: ["terminal"], active: "terminal" }] };
        assert.equal(prefs.set("workspace-seed", seed), true);
        assert.deepEqual(prefs.get("workspace-seed", null), seed);
      },
    );
  });
});
