import test from "node:test";
import assert from "node:assert/strict";
import { formatBytes, formatCpu, loadLabel } from "../js/lib/load.js";

test("formatCpu", () => {
  assert.equal(formatCpu(1.25), "1.3%");
  assert.equal(formatCpu(12.4), "12%");
  assert.equal(formatCpu(null), "—");
});

test("formatBytes", () => {
  assert.equal(formatBytes(512), "512 B");
  assert.equal(formatBytes(2048), "2 KB");
  assert.equal(formatBytes(5.5 * 1024 * 1024), "5.5 MB");
  assert.equal(formatBytes(48 * 1024 * 1024), "48 MB");
  assert.equal(formatBytes(1.25 * 1024 * 1024 * 1024), "1.3 GB");
});

test("loadLabel combines host, agents, and cost", () => {
  const { text, title } = loadLabel(
    {
      host: { cpu_pct: 2.1, rss_bytes: 64 * 1024 * 1024 },
      agents: { cpu_pct: 40, rss_bytes: 1.2 * 1024 * 1024 * 1024, sessions: 2 },
    },
    [{ usage: { cost_usd: 0.2 } }, { usage: { cost_usd: 0.22 } }],
  );
  assert.match(text, /Sync 2\.1% · 64 MB/);
  assert.match(text, /Agents 40% · 1\.2 GB/);
  assert.match(text, /\$0\.42/);
  assert.match(title, /2 sessions/);
});

test("loadLabel hides agents when none live", () => {
  const { text } = loadLabel(
    {
      host: { cpu_pct: 1, rss_bytes: 1024 * 1024 },
      agents: { cpu_pct: 0, rss_bytes: 0, sessions: 0 },
    },
    [],
  );
  assert.equal(text, "Sync 1.0% · 1.0 MB");
});
