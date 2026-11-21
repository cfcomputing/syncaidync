import { DyncElement, html, raw } from "../lib/element.js";
import { brandMark, wordmark } from "../lib/brand.js";
import { icon } from "../lib/icons.js";
import * as themes from "../lib/themes.js";
import * as prefs from "../lib/prefs.js";
import { openNewSession } from "./dync-new-session.js";
import { openImport } from "./dync-import.js";
import { openSettings } from "./dync-settings.js";
import { store } from "../lib/store.js";
import { queueCounts, queueSessions } from "../lib/queue.js";
import { openMenuBelow, closeMenu } from "../lib/menu.js";
import { stateLabel } from "../lib/labels.js";
import * as route from "../lib/route.js";
import { api } from "../lib/api.js";
import { loadLabel } from "../lib/load.js";

const BLOCKED_SHORT = {
  approval: "Needs approval",
  question: "Has a question",
  startup: "Needs a look",
};

/** Menu line for one Your-move session. */
function queueItemLabel(session) {
  const raw = String(session?.title || "").trim() || "Untitled";
  const title = raw.length > 36 ? `${raw.slice(0, 33)}…` : raw;
  let status = stateLabel(session);
  if (session?.state === "blocked") {
    status = BLOCKED_SHORT[session.blocked?.kind] || "Blocked on you";
  }
  return `${title} · ${status}`;
}

export class DyncTopbar extends DyncElement {
  #layout = "rows";
  #load = null;
  #loadTimer = null;

  mount() {
    const board = prefs.get("board", { layout: "rows", collapsed: [], free: {} });
    this.#layout = board?.layout || "rows";

    this.render(
      html`<header class="topbar">
        <a class="brand" href="#/" aria-label="SyncAiDync Board">
          ${raw(brandMark(28))}
          ${raw(wordmark())}
        </a>
        <div class="topbar-session" data-slot="session" hidden></div>
        <div class="topbar-actions">
          <span class="reconnect" hidden data-slot="reconnect">Reconnecting…</span>
          <span class="load-chip" data-slot="load" hidden title=""></span>

          <button type="button" class="queue-pill" data-slot="queue" data-action="queue" hidden
            aria-live="polite" aria-haspopup="menu" aria-expanded="false">
            <span class="dot"></span>
            <span data-queue-label>Your move</span>
          </button>

          <button type="button" class="sm" data-action="new-session">
            ${raw(icon("plus"))} New session
          </button>

          <button type="button" class="ghost sm" data-action="pulls" title="Pull requests">
            ${raw(icon("pull-request"))}
            <span class="label">Pulls</span>
          </button>
          <button type="button" class="ghost sm" data-action="import" data-board-only title="Import history">
            ${raw(icon("import"))}
            <span class="label">Import</span>
          </button>

          <div class="pill layout-pill" role="group" aria-label="Layout" data-board-only>
            <button type="button" class="icon" data-layout="rows" title="Rows" aria-label="Rows">${raw(icon("rows"))}</button>
            <button type="button" class="icon" data-layout="grid" title="Grid" aria-label="Grid">${raw(icon("grid"))}</button>
            <button type="button" class="icon" data-layout="free" title="Free" aria-label="Free">${raw(icon("free"))}</button>
          </div>
          <button type="button" class="ghost sm" data-action="tidy" data-slot="tidy" hidden title="Tidy free layout">
            Tidy
          </button>

          <div class="pill workspace-tools" role="group" aria-label="Workspace tools" data-slot="tools" hidden>
            <button type="button" class="ghost sm" data-workspace-tool="terminal">Terminal</button>
            <button type="button" class="ghost sm" data-workspace-tool="files">Files</button>
            <button type="button" class="ghost sm" data-workspace-tool="changes">Changes</button>
            <button type="button" class="ghost sm" data-workspace-tool="session">Session</button>
          </div>

          <button type="button" class="icon" data-action="settings" title="Settings: appearance, accent theme, alerts" aria-label="Settings">${raw(icon("gear"))}<span class="update-dot" hidden data-update-dot></span></button>
        </div>
      </header>`,
    );

    this.on(this, "click", (ev) => {
      if (ev.target.closest("a.brand")) {
        if (!ev.metaKey && !ev.ctrlKey && !ev.shiftKey && !ev.altKey) {
          ev.preventDefault();
          this.#navigate({ name: "board" });
        }
        return;
      }
      const button = ev.target.closest("button");
      if (!button || !this.contains(button)) return;

      const tool = button.dataset.workspaceTool;
      if (tool) {
        this.dispatchEvent(new CustomEvent("dync-workspace-tool", { bubbles: true, detail: tool }));
        return;
      }

      const action = button.dataset.action;
      if (action === "new-session") {
        ev.preventDefault();
        openNewSession();
        return;
      }
      if (action === "tidy") {
        window.dispatchEvent(new CustomEvent("dync-tidy"));
        return;
      }
      if (action === "queue") {
        this.#openQueueMenu(button);
        return;
      }
      if (action === "pulls") {
        this.#navigate({ name: "pulls" });
        return;
      }
      if (action === "import") {
        openImport();
        return;
      }
      if (action === "settings") {
        openSettings();
        return;
      }

      const layout = button.dataset.layout;
      if (layout) {
        this.#layout = layout;
        this.#syncLayout();
        window.dispatchEvent(new CustomEvent("dync-layout", { detail: { layout } }));
      }
    });

    this.setRoute(route.current());
    this.#syncQueue();
    this.#syncLoad();
    this.#startLoadPoll();
    const unsub = store.subscribe(() => {
      this.#syncQueue();
      this.#syncLoad();
    });
    this.cleanup(unsub);
    this.cleanup(() => this.#stopLoadPoll());
    this.on(window, "dync-queue", () => this.#syncQueue());
    // Day and night use different shades of the accent theme.
    this.on(window, "dync-appearance", () => themes.applyTheme(themes.currentName()));
    this.on(window, "dync-layout-state", (ev) => {
      if (ev.detail?.layout) {
        this.#layout = ev.detail.layout;
        this.#syncLayout();
      }
    });

    const paintUpdate = () => {
      const available = !!store.update?.newer;
      this.querySelector("[data-update-dot]").hidden = !available;
      this.querySelector('[data-action="settings"]').ariaLabel = available
        ? "Settings, update available"
        : "Settings";
    };
    this.on(window, "dync-update", paintUpdate);
    paintUpdate();

    // Alt+N
    this.on(window, "keydown", (ev) => {
      if (ev.altKey && !ev.metaKey && !ev.ctrlKey && (ev.key === "n" || ev.key === "N")) {
        ev.preventDefault();
        openNewSession();
      }
    });
  }

  #navigate(destination, focusId) {
    this.dispatchEvent(new CustomEvent("dync-navigate", {
      bubbles: true,
      detail: { destination, focusId },
    }));
  }

  #openQueueMenu(pill) {
    const queue = queueSessions(store.sessions);
    if (!queue.length) return;
    if (pill.getAttribute("aria-expanded") === "true") {
      closeMenu();
      pill.setAttribute("aria-expanded", "false");
      return;
    }
    const items = queue.map((s) => ({ id: s.id, label: queueItemLabel(s) }));
    const menu = openMenuBelow(pill, items, (id) => this.#pickQueueSession(id));
    menu.classList.add("queue-menu");
    pill.setAttribute("aria-expanded", "true");
    menu._onClose = () => pill.setAttribute("aria-expanded", "false");
  }

  /** Board: jump to the card. Elsewhere: open that session's workspace. */
  #pickQueueSession(id) {
    if (!id) return;
    if (route.current().name === "board") {
      this.#navigate({ name: "board" }, id);
      return;
    }
    this.#navigate({ name: "session", id });
  }

  /** The empty area after the brand that the workspace fills with its session controls. */
  sessionSlot() {
    return this.querySelector('[data-slot="session"]');
  }

  setRoute(current) {
    for (const control of this.querySelectorAll("[data-board-only]")) {
      control.hidden = current.name !== "board";
    }
    this.querySelector('[data-slot="tools"]').hidden = current.name !== "session" || current.solo;
    this.#syncLayout();
  }

  #syncLayout() {
    for (const b of this.querySelectorAll("[data-layout]")) {
      b.classList.toggle("active", b.getAttribute("data-layout") === this.#layout);
    }
    const tidyBtn = this.querySelector('[data-slot="tidy"]');
    if (tidyBtn) tidyBtn.hidden = route.current().name !== "board" || this.#layout !== "free";
  }

  #syncQueue() {
    const pill = this.querySelector('[data-slot="queue"]');
    if (!pill) return;
    const counts = queueCounts(store.sessions);
    const label = pill.querySelector("[data-queue-label]");
    if (counts.total === 0) {
      if (pill.getAttribute("aria-expanded") === "true") closeMenu();
      pill.hidden = true;
      pill.classList.remove("has-count");
      pill.setAttribute("aria-expanded", "false");
      if (label) label.textContent = "Your move";
      pill.title = "Your move";
      return;
    }
    pill.hidden = false;
    pill.classList.add("has-count");
    const text =
      counts.total === 1 ? "Your move · 1" : `Your move · ${counts.total}`;
    if (label) label.textContent = text;
    pill.title =
      counts.blocked && counts.ready
        ? `${counts.blocked} blocked, ${counts.ready} ready`
        : counts.blocked
          ? `${counts.blocked} blocked`
          : `${counts.ready} ready`;
  }

  setConnected(connected) {
    const el = this.querySelector('[data-slot="reconnect"]');
    if (el) el.hidden = !!connected;
    if (connected) this.#startLoadPoll();
    else this.#stopLoadPoll();
  }

  #startLoadPoll() {
    if (this.#loadTimer != null) return;
    const tick = () => {
      this.#refreshLoad();
    };
    tick();
    this.#loadTimer = window.setInterval(tick, 3000);
  }

  #stopLoadPoll() {
    if (this.#loadTimer != null) {
      window.clearInterval(this.#loadTimer);
      this.#loadTimer = null;
    }
  }

  async #refreshLoad() {
    try {
      this.#load = await api.get("/load");
      this.#syncLoad();
    } catch {
      // Keep the last chip; the reconnect banner covers hard failures.
    }
  }

  #syncLoad() {
    const el = this.querySelector('[data-slot="load"]');
    if (!el) return;
    const { text, title } = loadLabel(this.#load, store.sessions || []);
    if (!text) {
      el.hidden = true;
      el.textContent = "";
      el.title = "";
      return;
    }
    el.hidden = false;
    el.textContent = text;
    el.title = title || text;
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-topbar")) {
  customElements.define("dync-topbar", DyncTopbar);
}
