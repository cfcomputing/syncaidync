import { DyncElement, html } from "../lib/element.js";
import { store } from "../lib/store.js";
import { api, ApiError } from "../lib/api.js";
import * as prefs from "../lib/prefs.js";
import { showToast } from "../lib/toast.js";
import { openRemoveSession } from "./dync-remove-session.js";
import * as route from "../lib/route.js";
import {
  orderSessions,
  rowsLayout,
  gridLayout,
  freeLayout,
  tidy,
  clampRect,
} from "../lib/layout.js";
import "./dync-session-card.js";
import "./dync-session-strip.js";
import "./dync-empty-board.js";
import "./dync-canvas.js";

const DEFAULT_BOARD = { layout: "rows", collapsed: [], free: {} };

export class DyncBoard extends DyncElement {
  #cards = new Map(); // id -> { wrap, card, strip }
  #collapsed = new Set();
  #free = {};
  #layout = "rows";
  #focusedId = null;
  #width = 0;
  #height = 0;

  mount() {
    const boardPrefs = prefs.get("board", DEFAULT_BOARD) || DEFAULT_BOARD;
    this.#layout = boardPrefs.layout || "rows";
    this.#collapsed = new Set(boardPrefs.collapsed || []);
    this.#free = { ...(boardPrefs.free || {}) };

    this.render(
      html`<div class="board">
        <div class="board-toolbar">
          <h2>Sessions</h2>
        </div>
        <dync-canvas data-canvas></dync-canvas>
        <dync-empty-board data-empty hidden></dync-empty-board>
      </div>`,
    );

    const canvas = this.#canvas();
    this.on(this, "dync-terminal-focus", (ev) => {
      const id = ev.detail?.session;
      if (id) this.#focusTerminal(id);
    });

    this.on(this, "dync-focus", (ev) => {
      this.#focusedId = ev.detail?.id || null;
      this.#syncFocus();
    });

    this.on(this, "dync-open", (ev) => {
      const id = ev.detail?.id;
      if (id) route.go({ name: "session", id });
    });

    this.on(this, "dync-collapse", (ev) => {
      const { id, collapsed } = ev.detail;
      if (!id) return;
      if (collapsed) this.#collapsed.add(id);
      else this.#collapsed.delete(id);
      this.#persist();
      this.#relayout();

      const entry = this.#cards.get(id);
      const control = collapsed
        ? entry?.strip.querySelector('[data-action="expand"], [data-action="wake"]')
        : entry?.card.querySelector('[data-action="collapse"]');
      control?.focus({ preventScroll: true });
    });

    this.on(this, "dync-snooze", (ev) => this.#api(ev, () =>
      api.post(`/sessions/${ev.detail.id}/snooze`, { snoozed: !!ev.detail.snoozed }),
    ));
    this.on(this, "dync-stop", (ev) => this.#api(ev, () =>
      api.post(`/sessions/${ev.detail.id}/stop`),
    ));
    this.on(this, "dync-resume", (ev) => this.#api(ev, () =>
      api.post(`/sessions/${ev.detail.id}/resume`),
    ));
    this.on(this, "dync-archive", (ev) => this.#api(ev, () =>
      api.post(`/sessions/${ev.detail.id}/archive`, { archived: ev.detail.archived }),
    ));
    this.on(this, "dync-rename", (ev) => this.#api(ev, () =>
      api.post(`/sessions/${ev.detail.id}/title`, { title: ev.detail.title }),
    ));
    this.on(this, "dync-remove", (ev) => {
      if (ev.detail?.id) openRemoveSession(ev.detail.id);
    });

    this.on(this, "dync-rect-commit", (ev) => {
      const { id, rect } = ev.detail || {};
      if (!id || !rect) return;
      const prev = this.#free[id] || {};
      this.#free[id] = clampRect({
        ...prev,
        ...rect,
        id,
        savedH: rect.collapsed ? (prev.savedH ?? rect.savedH) : rect.h,
      });
      this.#persist();
      this.#relayout();
    });

    this.on(this, "dync-rect-raise", (ev) => {
      const { id, rect, kind } = ev.detail || {};
      if (!id || !rect) return;
      // Only the edit terminal may resize the PTY, so resizing a card focuses it;
      // its terminal then refits and reflows live while the card is dragged.
      if (kind === "resize") this.#focusTerminal(id);
      const prev = this.#free[id] || {};
      this.#free[id] = { ...prev, ...rect, id };
      this.#persist();
    });

    this.on(window, "dync-layout", (ev) => {
      const next = ev.detail?.layout;
      if (next !== "rows" && next !== "grid" && next !== "free") return;
      this.#layout = next;
      this.#persist();
      this.#relayout(true);
      this.#announceLayout();
    });

    this.on(window, "dync-show-archived-sessions", () => this.update());

    this.on(window, "dync-focus-session", (ev) => {
      const id = ev.detail?.id;
      if (!id || !this.#cards.has(id)) return;
      this.#focusedId = id;
      this.#syncFocus();
      this.#canvas()?.scrollIntoViewId(id);
      const entry = this.#cards.get(id);
      entry?.card?.focus?.();
    });

    this.on(window, "dync-tidy", () => {
      if (this.#layout !== "free") return;
      const sessions = this.#visibleSessions();
      const result = tidy({
        width: this.#width,
        viewportHeight: this.#height,
        sessions,
        collapsed: this.#collapsed,
        free: this.#free,
      });
      for (const item of result.items) {
        this.#free[item.id] = {
          x: item.x,
          y: item.y,
          w: item.w,
          h: item.collapsed ? (item.savedH ?? 380) : item.h,
          z: item.z,
          savedH: item.savedH ?? item.h,
        };
      }
      this.#persist();
      this.#relayout(true);
    });

    // Alt shortcuts — capture so terminals do not eat them.
    this.on(
      window,
      "keydown",
      (ev) => {
        if (!ev.altKey || ev.metaKey || ev.ctrlKey) return;
        const key = ev.key;
        if (key === "]" || key === "[") {
          ev.preventDefault();
          ev.stopPropagation();
          this.#cycleFocus(key === "]" ? 1 : -1);
          return;
        }
        if (key === "Enter") {
          if (!this.#focusedId) return;
          ev.preventDefault();
          ev.stopPropagation();
          route.go({ name: "session", id: this.#focusedId });
          return;
        }
        if (key === "z" || key === "Z") {
          if (!this.#focusedId) return;
          const s = store.sessions.get(this.#focusedId);
          if (!s) return;
          ev.preventDefault();
          ev.stopPropagation();
          this.#api(null, () =>
            api.post(`/sessions/${s.id}/snooze`, { snoozed: !s.snoozed }),
          );
        }
      },
      true,
    );

    this.on(this, "dync-canvas-resize", () => {
      const size = canvas.size();
      if (size.width === this.#width && size.height === this.#height) return;
      this.#width = size.width;
      this.#height = size.height;
      this.#relayout();
    });

    const unsub = store.subscribe(() => this.update());
    this.cleanup(unsub);

    // Initial size after layout.
    requestAnimationFrame(() => {
      const size = canvas.size();
      this.#width = size.width;
      this.#height = size.height;
      this.update();
      this.#announceLayout();
    });
  }

  #focusTerminal(id) {
    this.#focusedId = id;
    this.#syncFocus();
    for (const term of this.querySelectorAll("dync-terminal")) {
      if (term.getAttribute("session") === id) term.setAttribute("mode", "edit");
      else if (term.getAttribute("mode") === "edit") term.setAttribute("mode", "view");
    }
  }

  async #api(_ev, fn) {
    try {
      await fn();
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : "Something went wrong.", {
        kind: "error",
      });
    }
  }

  #canvas() {
    return this.querySelector("[data-canvas]");
  }

  #persist() {
    this.#pruneStalePrefs();
    prefs.set("board", {
      layout: this.#layout,
      collapsed: [...this.#collapsed],
      free: this.#free,
    });
  }

  #pruneStalePrefs() {
    // Before the first snapshot, an empty store does not mean sessions were removed.
    if (!store.hasSessionSnapshot) return;
    for (const id of [...this.#collapsed]) {
      if (!store.sessions.has(id)) this.#collapsed.delete(id);
    }
    for (const id of Object.keys(this.#free)) {
      if (!store.sessions.has(id)) delete this.#free[id];
    }
  }

  #announceLayout() {
    window.dispatchEvent(
      new CustomEvent("dync-layout-state", {
        detail: { layout: this.#layout },
      }),
    );
  }

  #syncFocus() {
    for (const { card, wrap } of this.#cards.values()) {
      const on = card.session?.id === this.#focusedId;
      card.focused = on;
      wrap?.classList.toggle("is-focused-item", on);
    }
  }

  #cycleFocus(dir) {
    const ordered = orderSessions(this.#visibleSessions());
    if (!ordered.length) return;
    let idx = ordered.findIndex((s) => s.id === this.#focusedId);
    if (idx < 0) idx = dir > 0 ? -1 : 0;
    idx = (idx + dir + ordered.length) % ordered.length;
    this.#focusedId = ordered[idx].id;
    this.#syncFocus();
    this.#canvas()?.scrollIntoViewId(this.#focusedId);
    const entry = this.#cards.get(this.#focusedId);
    entry?.card?.focus?.();
  }

  #visibleSessions() {
    const showArchived = prefs.get("show-archived-sessions", false);
    return [...store.sessions.values()].filter((session) => showArchived || !session.archived);
  }

  #computeLayout() {
    const sessions = this.#visibleSessions();
    const input = {
      width: this.#width || this.clientWidth || 800,
      viewportHeight: this.#height || window.innerHeight || 800,
      sessions,
      collapsed: this.#collapsed,
      free: this.#free,
    };
    if (this.#layout === "grid") return gridLayout(input);
    if (this.#layout === "free") {
      const result = freeLayout(input);
      // Persist newly assigned slots
      let changed = false;
      for (const item of result.items) {
        if (!this.#free[item.id]) {
          this.#free[item.id] = {
            x: item.x,
            y: item.y,
            w: item.w,
            h: item.collapsed ? (item.savedH ?? 380) : item.h,
            z: item.z,
            savedH: item.savedH ?? item.h,
          };
          changed = true;
        } else if (item.savedH != null) {
          this.#free[item.id] = {
            ...this.#free[item.id],
            savedH: item.savedH,
          };
        }
      }
      if (changed) this.#persist();
      return result;
    }
    return rowsLayout(input);
  }

  #relayout(animate = false) {
    const canvas = this.#canvas();
    if (!canvas) return;
    canvas.interactive = this.#layout === "free";
    const layout = this.#computeLayout();
    canvas.applyLayout({ ...layout, animate });
    this.#syncCardVisibility(layout.items);
    this.#syncSnoozeLabel(layout.items);
  }

  #syncSnoozeLabel(items) {
    const surface = this.#canvas()?.surface;
    if (!surface) return;
    let label = surface.querySelector('[data-group="snoozed"]');
    const firstSnoozed = items.find((it) => {
      const s = store.sessions.get(it.id);
      return s?.snoozed;
    });
    if (!firstSnoozed) {
      label?.remove();
      return;
    }
    if (!label) {
      label = document.createElement("div");
      label.className = "board-group-label";
      label.dataset.group = "snoozed";
      label.textContent = "Snoozed";
      surface.appendChild(label);
    }
    const y = Math.max(0, firstSnoozed.y - 28);
    label.style.transform = `translate(${firstSnoozed.x}px, ${y}px)`;
    label.style.width = `${firstSnoozed.w}px`;
  }

  #syncCardVisibility(items) {
    for (const item of items) {
      const entry = this.#cards.get(item.id);
      if (!entry) continue;
      const showStrip = !!item.collapsed;
      entry.wrap.classList.toggle("is-strip", showStrip);
      entry.card.collapsed = showStrip;
      entry.strip.collapsed = showStrip;
    }
  }

  update() {
    const canvas = this.#canvas();
    const empty = this.querySelector("[data-empty]");
    const sessions = this.#visibleSessions();

    this.#pruneStalePrefs();

    if (empty) empty.hidden = sessions.length > 0;
    if (canvas) canvas.hidden = sessions.length === 0;

    const seen = new Set();
    for (const s of sessions) {
      seen.add(s.id);
      let entry = this.#cards.get(s.id);
      if (!entry) {
        const wrap = canvas.ensureItem(s.id);
        const card = document.createElement("dync-session-card");
        const strip = document.createElement("dync-session-strip");
        wrap.appendChild(card);
        wrap.appendChild(strip);
        entry = { wrap, card, strip };
        this.#cards.set(s.id, entry);
      }
      entry.card.meta = store.meta;
      entry.card.session = s;
      entry.card.focused = s.id === this.#focusedId;
      entry.strip.session = s;
    }

    for (const id of this.#cards.keys()) {
      if (!seen.has(id)) {
        canvas.removeItem(id);
        this.#cards.delete(id);
        if (this.#focusedId === id) this.#focusedId = null;
      }
    }

    if (sessions.length > 0) {
      const size = canvas.size();
      if (size.width) this.#width = size.width;
      if (size.height) this.#height = size.height;
      this.#relayout();
    }
    this.#syncFocus();
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-board")) {
  customElements.define("dync-board", DyncBoard);
}
