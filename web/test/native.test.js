import test from "node:test";
import assert from "node:assert/strict";
import { isNative, notify, ready, setBadge, startNative } from "../js/lib/native.js";

test("native message shapes and session routing", () => {
  assert.equal(isNative(), false);
  const messages = [];
  const opened = [];
  const window = new EventTarget();
  window.__syncaidyncNative = true;
  window.webkit = { messageHandlers: { syncaidync: { postMessage: (msg) => messages.push(msg) } } };
  globalThis.window = window;
  try {
    startNative({ openSession: (id) => opened.push(id), liveCount: () => 2 });
    setBadge(3);
    notify({ id: "s1", title: "Session", body: "Ready for you" });
    ready();
    window.dispatchEvent(new CustomEvent("syncaidync:open", { detail: { id: "s1" } }));
    assert.equal(window.__syncaidyncLiveCount(), 2);
    assert.deepEqual(opened, ["s1"]);
    assert.deepEqual(messages, [
      { type: "badge", count: 3 },
      { type: "notify", id: "s1", title: "Session", body: "Ready for you" },
      { type: "ready" },
    ]);
  } finally {
    delete globalThis.window;
  }
});
