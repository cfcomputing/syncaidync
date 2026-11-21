/** Format and combine live process load with session cost for the top bar. */

/**
 * @param {{ host?: { cpu_pct?: number, rss_bytes?: number }, agents?: { cpu_pct?: number, rss_bytes?: number, sessions?: number } } | null} load
 * @param {{ usage?: { cost_usd?: number } }[]} sessions
 * @returns {{ text: string, title: string }}
 */
export function loadLabel(load, sessions = []) {
  const parts = [];
  const title = [];

  const host = load?.host;
  if (host && typeof host.rss_bytes === "number") {
    const cpu = formatCpu(host.cpu_pct);
    const mem = formatBytes(host.rss_bytes);
    parts.push(`Sync ${cpu} · ${mem}`);
    title.push(`SyncAiDync ${cpu} CPU, ${mem}`);
  }

  const agents = load?.agents;
  const agentSessions = Number(agents?.sessions) || 0;
  if (agents && agentSessions > 0 && typeof agents.rss_bytes === "number") {
    const cpu = formatCpu(agents.cpu_pct);
    const mem = formatBytes(agents.rss_bytes);
    const n = agentSessions === 1 ? "1 session" : `${agentSessions} sessions`;
    parts.push(`Agents ${cpu} · ${mem}`);
    title.push(`Agents ${cpu} CPU, ${mem} across ${n}`);
  }

  let cost = 0;
  let hasCost = false;
  for (const s of sessions) {
    const c = s?.usage?.cost_usd;
    if (typeof c === "number" && Number.isFinite(c)) {
      cost += c;
      hasCost = true;
    }
  }
  if (hasCost) {
    const dollar = `$${cost.toFixed(2)}`;
    parts.push(dollar);
    title.push(`Cost ${dollar}`);
  }

  return { text: parts.join("  ·  "), title: title.join(" · ") };
}

/** @param {number | null | undefined} pct */
export function formatCpu(pct) {
  if (typeof pct !== "number" || !Number.isFinite(pct)) return "—";
  if (pct < 10) return `${pct.toFixed(1)}%`;
  return `${Math.round(pct)}%`;
}

/** @param {number | null | undefined} bytes */
export function formatBytes(bytes) {
  if (typeof bytes !== "number" || !Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${Math.round(kb)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return mb < 10 ? `${mb.toFixed(1)} MB` : `${Math.round(mb)} MB`;
  const gb = mb / 1024;
  return gb < 10 ? `${gb.toFixed(1)} GB` : `${Math.round(gb)} GB`;
}
