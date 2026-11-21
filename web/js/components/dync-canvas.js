import { DyncElement, html } from "../lib/element.js";
import { startMove, startResize } from "../lib/drag.js";
import { clampRect } from "../lib/layout.js";

const EDGES = ["n", "s", "e", "w", "ne", "nw", "se", "sw"];

/**
 * Absolute canvas host. Children with [data-id] are placed from rect lists.
 * When interactive, header [data-drag] moves and edge handles resize.
 */
export class DyncCanvas extends DyncElement {
  #rects = new Map(); // id -> rect
  #interactive = false;
  #busy = false;
  #active = null; // { id, kind }
  #ro = null;

  get viewport() {
    return this.querySelector(".canvas-viewport");
  }

  get surface() {
    return this.querySelector(".canvas-surface");
  }

  get interactive() {
    return this.#interactive;
  }
  set interactive(v) {
    this.#interactive = !!v;
    this.classList.toggle("is-interactive", this.#interactive);
    this.#syncHandles();
  }

  mount() {
    this.render(
      html`<div class="canvas-viewport" data-viewport>
        <div class="canvas-surface" data-surface></div>
      </div>`,
    );

    this.on(this, "pointerdown", (ev) => this.#onPointerDown(ev));
    this.on(this, "keydown", (ev) => this.#onKey(ev));

    // Observe host size for consumers.
    if (typeof ResizeObserver !== "undefined") {
      this.#ro = new ResizeObserver(() => {
        this.dispatchEvent(
          new CustomEvent("dync-canvas-resize", {
            bubbles: true,
            composed: true,
            detail: this.size(),
          }),
        );
      });
      this.#ro.observe(this);
      this.cleanup(() => this.#ro?.disconnect());
    }
  }

  size() {
    const vp = this.viewport;
    return {
      width: vp?.clientWidth || this.clientWidth || 0,
      height: vp?.clientHeight || this.clientHeight || 0,
    };
  }

  /** Append a placed item wrapper; returns the wrapper. */
  ensureItem(id) {
    const surface = this.surface;
    let el = surface.querySelector(`[data-id="${cssEscape(id)}"]`);
    if (el) return el;
    el = document.createElement("div");
    el.className = "canvas-item";
    el.dataset.id = id;
    el.tabIndex = -1;
    surface.appendChild(el);
    return el;
  }

  item(id) {
    return this.surface?.querySelector(`[data-id="${cssEscape(id)}"]`) || null;
  }

  removeItem(id) {
    this.item(id)?.remove();
    this.#rects.delete(id);
  }

  /**
   * @param {{ items: Array<{id,x,y,w,h,z}>, canvas?: {w:number,h:number}, animate?: boolean }} layout
   */
  applyLayout(layout) {
    const surface = this.surface;
    if (!surface) return;
    const items = layout.items || [];
    const seen = new Set();
    for (const r of items) {
      seen.add(r.id);
      this.#rects.set(r.id, { ...r });
      const el = this.ensureItem(r.id);
      this.#paintItem(el, r, !!layout.animate && !this.#busy);
    }
    for (const el of [...surface.querySelectorAll(".canvas-item")]) {
      if (!seen.has(el.dataset.id)) el.remove();
    }
    const cw = Math.max(layout.canvas?.w || 0, this.size().width);
    const ch = Math.max(layout.canvas?.h || 0, this.size().height);
    surface.style.width = `${cw}px`;
    surface.style.height = `${ch}px`;
    this.#syncHandles();
  }

  /** Bring an item to the front, unless it already is. Emits `dync-rect-raise` when it moves. */
  raise(id) {
    const rect = this.#rects.get(id);
    const item = this.item(id);
    if (!rect || !item || this.#busy) return;
    const top = Math.max(0, ...[...this.#rects].filter(([rid]) => rid !== id).map(([, r]) => r.z || 1));
    if ((rect.z || 1) > top) return;
    const raised = { ...rect, z: this.#nextZ(id) };
    this.#rects.set(id, raised);
    this.#paintItem(item, raised, false);
    this.#emit("dync-rect-raise", { id, rect: raised, kind: "click" });
  }

  scrollIntoViewId(id) {
    const el = this.item(id);
    el?.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "smooth" });
  }

  #paintItem(el, r, _animate) {
    el.style.setProperty("--cx", `${Math.round(r.x)}px`);
    el.style.setProperty("--cy", `${Math.round(r.y)}px`);
    el.style.setProperty("--cw", `${Math.round(r.w)}px`);
    el.style.setProperty("--ch", `${Math.round(r.h)}px`);
    el.style.setProperty("--cz", String(r.z || 1));
  }

  #syncHandles() {
    for (const el of this.surface?.querySelectorAll(".canvas-item") || []) {
      const existing = el.querySelector(".canvas-handles");
      if (!this.#interactive) {
        existing?.remove();
        el.querySelector(".canvas-size-chip")?.remove();
        continue;
      }
      if (existing) continue;
      const handles = document.createElement("div");
      handles.className = "canvas-handles";
      handles.setAttribute("aria-hidden", "true");
      for (const edge of EDGES) {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "canvas-handle";
        btn.dataset.edge = edge;
        btn.tabIndex = -1;
        btn.setAttribute("aria-label", `Resize ${edge}`);
        handles.appendChild(btn);
      }
      el.appendChild(handles);
      const chip = document.createElement("div");
      chip.className = "canvas-size-chip";
      chip.hidden = true;
      el.appendChild(chip);
    }
  }

  #onPointerDown(ev) {
    if (!this.#interactive || this.#busy) return;
    if (ev.button !== 0) return;

    const handle = ev.target.closest?.(".canvas-handle");
    if (handle) {
      ev.preventDefault();
      ev.stopPropagation();
      const item = handle.closest(".canvas-item");
      if (!item) return;
      this.#beginResize(ev, item, handle.dataset.edge);
      return;
    }

    const drag = ev.target.closest?.("[data-drag]");
    if (!drag) return;
    // Ignore interactive controls inside the drag area.
    if (ev.target.closest("button, input, textarea, select, a, [contenteditable]")) return;
    const item = drag.closest(".canvas-item");
    if (!item) return;
    ev.preventDefault();
    this.#beginMove(ev, item);
  }

  #beginMove(ev, item) {
    const id = item.dataset.id;
    const rect = this.#rects.get(id);
    if (!rect) return;
    const raised = { ...rect, z: this.#nextZ(id) };
    this.#rects.set(id, raised);
    this.#paintItem(item, raised, false);
    this.#emit("dync-rect-raise", { id, rect: raised, kind: "move" });

    this.#busy = true;
    this.classList.add("is-busy");
    item.classList.add("is-dragging");
    this.#active = { id, kind: "move" };

    try {
      item.setPointerCapture?.(ev.pointerId);
    } catch {
      /* ignore */
    }

    startMove({
      pointerId: ev.pointerId,
      startX: ev.clientX,
      startY: ev.clientY,
      rect: raised,
      scrollEl: this.viewport,
      clampRect,
      onFrame: (r) => {
        const next = { ...raised, ...r, id, z: raised.z };
        this.#rects.set(id, next);
        this.#paintItem(item, next, false);
      },
      onCommit: (r) => {
        const next = clampRect({ ...raised, ...r, id, z: raised.z });
        this.#rects.set(id, next);
        this.#paintItem(item, next, false);
        this.#endBusy(item);
        this.#emit("dync-rect-commit", { id, rect: next });
      },
      onCancel: () => {
        this.#rects.set(id, rect);
        this.#paintItem(item, rect, false);
        this.#endBusy(item);
      },
    });
  }

  #beginResize(ev, item, edge) {
    const id = item.dataset.id;
    const rect = this.#rects.get(id);
    if (!rect) return;
    const raised = { ...rect, z: this.#nextZ(id) };
    this.#rects.set(id, raised);
    this.#paintItem(item, raised, false);
    this.#emit("dync-rect-raise", { id, rect: raised, kind: "resize" });

    this.#busy = true;
    this.classList.add("is-busy");
    item.classList.add("is-resizing");
    item.querySelector(`.canvas-handle[data-edge="${edge}"]`)?.classList.add("is-grabbed");
    this.#active = { id, kind: "resize" };
    const chip = item.querySelector(".canvas-size-chip");
    if (chip) {
      chip.hidden = false;
      chip.textContent = `${raised.w} × ${raised.h}`;
    }

    startResize({
      pointerId: ev.pointerId,
      edge,
      startX: ev.clientX,
      startY: ev.clientY,
      rect: raised,
      clampRect,
      onFrame: (r) => {
        const next = { ...raised, ...r, id, z: raised.z };
        this.#rects.set(id, next);
        this.#paintItem(item, next, false);
        if (chip) chip.textContent = `${next.w} × ${next.h}`;
      },
      onCommit: (r) => {
        const next = clampRect({ ...raised, ...r, id, z: raised.z });
        this.#rects.set(id, next);
        this.#paintItem(item, next, false);
        if (chip) chip.hidden = true;
        this.#endBusy(item);
        this.#emit("dync-rect-commit", { id, rect: next });
      },
      onCancel: () => {
        this.#rects.set(id, rect);
        this.#paintItem(item, rect, false);
        if (chip) chip.hidden = true;
        this.#endBusy(item);
      },
    });
  }

  #onKey(ev) {
    if (!this.#interactive || this.#busy) return;
    const item = ev.target.closest?.(".canvas-item");
    if (!item || !this.contains(item)) return;
    // Only when focus is on the item / header, not inside a terminal/input.
    if (ev.target.closest("dync-terminal, input, textarea, select")) return;
    const id = item.dataset.id;
    const rect = this.#rects.get(id);
    if (!rect) return;

    const arrows = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"];
    if (!arrows.includes(ev.key)) return;
    ev.preventDefault();
    ev.stopPropagation();

    const step = ev.shiftKey ? 1 : 16;
    let next = { ...rect };
    if (ev.altKey) {
      if (ev.key === "ArrowLeft") next.w = rect.w - step;
      if (ev.key === "ArrowRight") next.w = rect.w + step;
      if (ev.key === "ArrowUp") next.h = rect.h - step;
      if (ev.key === "ArrowDown") next.h = rect.h + step;
    } else {
      if (ev.key === "ArrowLeft") next.x = rect.x - step;
      if (ev.key === "ArrowRight") next.x = rect.x + step;
      if (ev.key === "ArrowUp") next.y = rect.y - step;
      if (ev.key === "ArrowDown") next.y = rect.y + step;
    }
    next = clampRect({ ...next, id, z: rect.z });
    this.#rects.set(id, next);
    this.#paintItem(item, next, false);
    this.#emit("dync-rect-commit", { id, rect: next });
  }

  #nextZ(id) {
    let max = 0;
    for (const [rid, r] of this.#rects) {
      if (rid === id) continue;
      max = Math.max(max, r.z || 1);
    }
    let z = max + 1;
    if (z > 10000) {
      const ordered = [...this.#rects.entries()].sort((a, b) => (a[1].z || 0) - (b[1].z || 0));
      ordered.forEach(([rid, r], i) => {
        const nr = { ...r, z: i + 1 };
        this.#rects.set(rid, nr);
        const el = this.item(rid);
        if (el) this.#paintItem(el, nr, false);
      });
      z = this.#rects.size;
    }
    return z;
  }

  #endBusy(item) {
    this.#busy = false;
    this.#active = null;
    this.classList.remove("is-busy");
    item.classList.remove("is-dragging", "is-resizing");
    item.querySelector(".canvas-handle.is-grabbed")?.classList.remove("is-grabbed");
  }

  #emit(name, detail) {
    this.dispatchEvent(new CustomEvent(name, { bubbles: true, composed: true, detail }));
  }
}

function cssEscape(id) {
  if (typeof CSS !== "undefined" && CSS.escape) return CSS.escape(id);
  return String(id).replace(/"/g, '\\"');
}

if (typeof customElements !== "undefined" && !customElements.get("dync-canvas")) {
  customElements.define("dync-canvas", DyncCanvas);
}
