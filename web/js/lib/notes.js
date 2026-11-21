/** Compose a sendable review message from notes + overall. */

const MAX_QUOTE_LINES = 6;

/**
 * @param {object[]} notes
 * @param {string} [overall]
 */
export function composeMessage(notes, overall) {
  const list = notes || [];
  const parts = [];
  const count = list.length;
  if (count > 0) {
    parts.push(`Review notes on your changes (${count}):`);
    parts.push("");
    list.forEach((note, i) => {
      parts.push(`${i + 1}. ${formatTarget(note)}`);
      const quote = quoteBlock(note.quote);
      for (const q of quote) parts.push(`   > ${q}`);
      if (note.text) {
        for (const line of String(note.text).split("\n")) {
          parts.push(`   ${line}`);
        }
      }
      parts.push("");
    });
  }
  const overallText = (overall || "").trim();
  if (overallText) {
    parts.push(`Overall: ${overallText}`);
  }
  return parts.join("\n").replace(/\n+$/, "");
}

function formatTarget(note) {
  const path = note.path || "file";
  const start = note.start;
  const end = note.end;
  if (start == null || end == null) {
    return `${path} (whole file)`;
  }
  const side = note.side === "old" ? " (before)" : "";
  if (start === end) {
    return `${path}, line ${start}${side}`;
  }
  return `${path}, lines ${start}–${end}${side}`;
}

function quoteBlock(quote) {
  if (!quote) return [];
  const lines = String(quote).split("\n");
  if (lines.length <= MAX_QUOTE_LINES) return lines;
  const kept = lines.slice(0, MAX_QUOTE_LINES);
  kept.push(`… (${lines.length - MAX_QUOTE_LINES} more lines)`);
  return kept;
}
