import { DyncElement, html, raw } from "../lib/element.js";
import { brandMark, wordmark } from "../lib/brand.js";
import { store } from "../lib/store.js";

const KNOWN = [
  { id: "claude", label: "Claude Code", cmd: "claude" },
  { id: "codex", label: "Codex", cmd: "codex" },
  { id: "pi", label: "Pi", cmd: "pi" },
  { id: "cursor", label: "Cursor Agent", cmd: "agent" },
  { id: "shell", label: "Shell", cmd: "shell" },
];

export class DyncEmptyBoard extends DyncElement {
  mount() {
    this.classList.add("empty-board");
    this.render(
      html`<div class="empty-board-inner">
        ${raw(brandMark(56))}
        <h1>${raw(wordmark())}</h1>
        <p>One local board for your AI coding agents — see who needs you, jump into any terminal.</p>
        <div class="empty-agents" data-agents></div>
      </div>`,
    );
    const unsub = store.subscribe(() => this.update());
    this.cleanup(unsub);
    this.update();
  }

  update() {
    const host = this.querySelector("[data-agents]");
    if (!host) return;
    const installed = new Map((store.meta?.agents || []).map((a) => [a.id, a]));
    const found = [];
    const missing = [];
    for (const a of KNOWN) {
      if (a.id === "shell" && !installed.has("shell")) continue;
      if (installed.get(a.id)?.installed) found.push(a.label);
      else if (a.id !== "shell") missing.push(`${a.label} (${a.cmd})`);
    }
    const bits = [];
    if (found.length) bits.push(`Found: ${found.join(", ")}`);
    if (missing.length) bits.push(`Not installed: ${missing.join(", ")}`);
    host.textContent = bits.join(" · ") || "No agents detected yet.";
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-empty-board")) {
  customElements.define("dync-empty-board", DyncEmptyBoard);
}
