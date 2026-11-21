import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { titleForCount, shouldRemind } from "../js/lib/alerts.js";

describe("titleForCount", () => {
  it("formats counts 0, 1, 12", () => {
    assert.equal(titleForCount(0), "SyncAiDync");
    assert.equal(titleForCount(1), "(1) SyncAiDync");
    assert.equal(titleForCount(12), "(12) SyncAiDync");
  });
});

describe("shouldRemind", () => {
  it("decides from interval, blocked count, and last remind time", () => {
    assert.equal(
      shouldRemind({ remind_minutes: false, blockedCount: 2, now: 100_000, lastRemindAt: 1 }),
      false,
    );
    assert.equal(
      shouldRemind({ remind_minutes: 5, blockedCount: 0, now: 100_000, lastRemindAt: 1 }),
      false,
    );
    assert.equal(
      shouldRemind({ remind_minutes: 5, blockedCount: 2, now: 100_000, lastRemindAt: 0 }),
      false,
    );
    assert.equal(
      shouldRemind({
        remind_minutes: 5,
        blockedCount: 2,
        now: 5 * 60_000 + 10,
        lastRemindAt: 10,
      }),
      true,
    );
    assert.equal(
      shouldRemind({
        remind_minutes: 5,
        blockedCount: 2,
        now: 5 * 60_000 - 1,
        lastRemindAt: 10,
      }),
      false,
    );
  });
});
