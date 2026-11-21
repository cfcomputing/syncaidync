/** Pure helpers for pull request list and badges. */

/** @param {{ state?: string, draft?: boolean }} pull */
export function stateBadge(pull) {
  if (!pull) return "Open";
  const state = String(pull.state || "open").toLowerCase();
  if (state === "merged") return "Merged";
  if (state === "closed") return "Closed";
  if (pull.draft) return "Draft";
  return "Open";
}

/** @param {{ files?: number, reviewed?: number } | null | undefined} progress */
export function progressLabel(progress) {
  const files = Number(progress?.files) || 0;
  const reviewed = Number(progress?.reviewed) || 0;
  return `${reviewed} of ${files} files`;
}

/**
 * Active first, then done; within a group, most recently added first.
 * @param {Iterable<object> | Map<string, object> | object[]} pulls
 */
export function sortPulls(pulls) {
  const list = pulls instanceof Map ? [...pulls.values()] : [...(pulls || [])];
  return list.sort((a, b) => {
    const aDone = a?.status === "done" ? 1 : 0;
    const bDone = b?.status === "done" ? 1 : 0;
    if (aDone !== bDone) return aDone - bDone;
    return (b?.added_at || 0) - (a?.added_at || 0);
  });
}
