import { DyncElement, html, raw } from "../lib/element.js";
import { icon } from "../lib/icons.js";
import { chipDrag } from "../lib/drag.js";
import { openMenuBelow, openPopoverBelow } from "../lib/menu.js";
import * as popout from "../lib/popout.js";
import "./dync-tool-terminal.js";
import "./dync-tool-files.js";
import "./dync-tool-changes.js";
import "./dync-tool-session.js";

const TOOL_LABEL = {
  terminal: "Terminal",
  files: "Files",
  changes: "Changes",
  session: "Session",
};

const TOOL_TAG = {
  terminal: "dync-tool-terminal",
  files: "dync-tool-files",
  changes: "dync-tool-changes",
  session: "dync-tool-session",
};

export class DyncPanel extends DyncElement {
  #group = null;
  #sessionId = null;
  #tools = new Map();
  #built = false;
  #popped = new Set();

  get group() {
    return this.#group;
  }
  set group(v) {
    this.#group = v;
    this.update();
  }

  get sessionId() {
    return this.#sessionId;
  }
  set sessionId(v) {
    this.#sessionId = v;
    this.#syncSessionAttr();
  }

  mount() {
    this.classList.add("panel", "card");
    this.render(
      html`<div class="panel-header" data-drag>
        <div class="panel-tabs" data-tabs role="tablist"></div>
        <button type="button" class="icon" data-action="menu" title="Panel menu" aria-label="Panel menu">${raw(icon("dots"))}</button>
      </div>
      <div class="panel-body" data-body>
        <div class="panel-popped" data-popped hidden>
          <p>Open in another window</p>
          <button type="button" class="sm" data-action="bring-back">Bring back</button>
        </div>
      </div>`,
    );
    this.#built = true;

    this.on(this, "click", (ev) => {
      const action = ev.target.closest("[data-action]")?.getAttribute("data-action");
      if (action === "menu") {
        this.#openMenu(ev.target.closest("[data-action]"));
        return;
      }
      if (action === "bring-back") {
        const tool = this.#group?.active;
        if (tool && this.#sessionId) {
          popout.closePopout(this.#sessionId, tool);
          this.#popped.delete(tool);
          this.#emit("dync-panel-bring-back", { tool });
          this.update();
        }
        return;
      }
      const tab = ev.target.closest(".panel-tab[data-tool]");
      if (tab && !tab.classList.contains("is-dragging-chip")) {
        this.#emit("dync-panel-activate", { tool: tab.getAttribute("data-tool") });
      }
    });

    this.on(this, "pointerdown", (ev) => {
      const tab = ev.target.closest(".panel-tab[data-tool]");
      if (!tab || ev.button !== 0) return;
      if (ev.target.closest("[data-action]")) return;
      const tool = tab.getAttribute("data-tool");
      ev.preventDefault();
      chipDrag({
        pointerId: ev.pointerId,
        startX: ev.clientX,
        startY: ev.clientY,
        onStart: ({ x, y }) => {
          tab.classList.add("is-dragging-chip");
          this.#showChip(tool, x, y);
          this.#emit("dync-tab-drag-start", { tool, groupId: this.#group?.id });
        },
        onMove: ({ x, y }) => {
          this.#moveChip(x, y);
          this.#emit("dync-tab-drag-move", { tool, x, y, groupId: this.#group?.id });
        },
        onDrop: ({ x, y }) => {
          tab.classList.remove("is-dragging-chip");
          this.#hideChip();
          this.#emit("dync-tab-drag-end", { tool, x, y, groupId: this.#group?.id });
        },
        onCancel: () => {
          tab.classList.remove("is-dragging-chip");
          this.#hideChip();
          this.#emit("dync-tab-drag-cancel", { tool });
        },
      });
    });

    this.update();
  }

  setPopped(tool, on) {
    if (on) this.#popped.add(tool);
    else this.#popped.delete(tool);
    this.update();
  }

  update() {
    if (!this.#built || !this.#group) return;
    const g = this.#group;
    this.dataset.group = g.id;
    this.style.setProperty("--panel-opacity", String(g.opacity ?? 1));
    this.style.setProperty("--panel-idle-opacity", String(g.idleOpacity ?? 1));

    const tabs = this.querySelector("[data-tabs]");
    tabs.innerHTML = "";
    for (const tool of g.tabs) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "panel-tab";
      btn.dataset.tool = tool;
      btn.setAttribute("role", "tab");
      btn.setAttribute("aria-selected", tool === g.active ? "true" : "false");
      if (tool === g.active) btn.classList.add("active");
      btn.textContent = TOOL_LABEL[tool] || tool;
      tabs.appendChild(btn);
    }

    const body = this.querySelector("[data-body]");
    const poppedEl = this.querySelector("[data-popped]");
    const active = g.active;
    const isPopped = this.#popped.has(active);
    poppedEl.hidden = !isPopped;

    for (const tool of g.tabs) {
      let el = this.#tools.get(tool);
      if (!el) {
        el = document.createElement(TOOL_TAG[tool]);
        el.dataset.tool = tool;
        body.appendChild(el);
        this.#tools.set(tool, el);
      }
      if (this.#sessionId) el.setAttribute("session", this.#sessionId);
      const show = tool === active && !isPopped;
      el.hidden = !show;
    }

    // Hide tools no longer in this group (moved away) — keep mounted off-panel? Spec: keep while panel exists.
    for (const [tool, el] of this.#tools) {
      if (!g.tabs.includes(tool)) {
        el.remove();
        this.#tools.delete(tool);
      }
    }
  }

  #syncSessionAttr() {
    for (const el of this.#tools.values()) {
      if (this.#sessionId) el.setAttribute("session", this.#sessionId);
    }
  }

  #openMenu(anchor) {
    const opacity = Math.round((this.#group?.opacity ?? 1) * 100);
    const idle = Math.round((this.#group?.idleOpacity ?? 1) * 100);
    openMenuBelow(
      anchor,
      [
        { id: "opacity", label: idle === opacity ? `Opacity ${opacity}%` : `Opacity ${opacity}% · inactive ${idle}%` },
        { id: "popout", label: "Pop out" },
      ],
      (id) => {
        if (id === "opacity") this.#pickOpacity(anchor);
        if (id === "popout") {
          const tool = this.#group?.active;
          if (tool && this.#sessionId) {
            popout.openPopout(this.#sessionId, tool);
            this.#popped.add(tool);
            this.#emit("dync-panel-popout", { tool });
            this.update();
          }
        }
      },
    );
  }

  /** Two sliders: opacity while this panel is active, and while another panel is. */
  #pickOpacity(anchor) {
    const pct = (v) => Math.round((v ?? 1) * 100);
    const box = document.createElement("div");
    box.className = "opacity-pop";
    box.innerHTML = `
      <label class="opacity-row">
        <span>Active</span>
        <input type="range" min="40" max="100" step="1" data-k="active" value="${pct(this.#group?.opacity)}">
        <output data-out="active">${pct(this.#group?.opacity)}%</output>
      </label>
      <label class="opacity-row">
        <span>Inactive</span>
        <input type="range" min="20" max="100" step="1" data-k="idle" value="${pct(this.#group?.idleOpacity)}">
        <output data-out="idle">${pct(this.#group?.idleOpacity)}%</output>
      </label>
      <p class="opacity-hint">Inactive applies while you are working in another panel.</p>`;
    const preview = (idle) => this.classList.toggle("is-preview-idle", idle);
    box.addEventListener("input", (ev) => {
      const input = ev.target.closest("input[data-k]");
      if (!input) return;
      const k = input.dataset.k;
      const v = Number(input.value) / 100;
      box.querySelector(`[data-out=${k}]`).textContent = `${input.value}%`;
      // While you drag Inactive, show this panel as it will look when inactive.
      preview(k === "idle");
      this.#emit(
        "dync-panel-opacity",
        k === "active" ? { opacity: v } : { idleOpacity: v },
      );
    });
    box.addEventListener("pointerup", () => preview(false));
    box.addEventListener("change", () => preview(false));
    openPopoverBelow(anchor, box, () => preview(false));
  }

  #chip = null;
  #showChip(tool, x, y) {
    this.#hideChip();
    const chip = document.createElement("div");
    chip.className = "tab-drag-chip";
    chip.textContent = TOOL_LABEL[tool] || tool;
    document.body.appendChild(chip);
    this.#chip = chip;
    this.#moveChip(x, y);
  }
  #moveChip(x, y) {
    if (!this.#chip) return;
    this.#chip.style.transform = `translate(${x + 12}px, ${y + 12}px)`;
  }
  #hideChip() {
    this.#chip?.remove();
    this.#chip = null;
  }

  #emit(name, detail = {}) {
    this.dispatchEvent(
      new CustomEvent(name, {
        bubbles: true,
        composed: true,
        detail: { groupId: this.#group?.id, ...detail },
      }),
    );
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-panel")) {
  customElements.define("dync-panel", DyncPanel);
}
