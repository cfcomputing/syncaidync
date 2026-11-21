import { DyncElement, html, raw } from "../lib/element.js";
import { store } from "../lib/store.js";
import { api, ApiError } from "../lib/api.js";
import { icon } from "../lib/icons.js";
import * as prefs from "../lib/prefs.js";
import * as route from "../lib/route.js";
import { showToast } from "../lib/toast.js";
import { askConfirm } from "../lib/prompt.js";
import {
  stateLabel,
  agentLabel,
  usageLabel,
} from "../lib/labels.js";
import {
  defaultLayout,
  normalize,
  moveTab,
  tearOut,
  setActive,
  setRect,
  setOpacity,
  TOOLS,
} from "../lib/tabgroups.js";
import * as popout from "../lib/popout.js";
import "./dync-canvas.js";
import "./dync-panel.js";
import "./dync-tool-terminal.js";
import "./dync-tool-files.js";
import "./dync-tool-changes.js";
import "./dync-tool-session.js";

export class DyncWorkspace extends DyncElement {
  #sessionId = null;
  #solo = false;
  #soloTool = null;
  #layout = null;
  #panels = new Map();
  #activeGroup = null; // the panel you last clicked or typed in
  #dragTool = null;
  #width = 0;
  #height = 0;
  #renaming = false;
  #header = null; // the top bar's session slot while this workspace owns it

  mount() {
    const r = route.current();
    this.#sessionId = r.id;
    this.#solo = !!r.solo && !!r.tool;
    this.#soloTool = r.tool;

    if (this.#solo) {
      this.#mountSolo();
      return;
    }

    this.render(
      html`<div class="workspace">
        <dync-canvas data-canvas class="ws-canvas"></dync-canvas>
        <div class="ws-gone" data-gone hidden>
          <p>That session is gone.</p>
          <button type="button" class="sm" data-action="back">Back to the board</button>
        </div>
      </div>`,
    );

    const onAction = async (ev) => {
      const action = ev.target.closest("[data-action]")?.getAttribute("data-action");
      if (action === "back") {
        if (!(await this.confirmLeave())) return;
        route.go({ name: "board" });
        return;
      }
      if (action === "reset") {
        this.#layout = defaultLayout(this.#width || 1200, this.#height || 700);
        this.#save();
        this.#paintLayout(true);
        return;
      }
      if (action === "stop") {
        this.#api(() => api.post(`/sessions/${this.#sessionId}/stop`));
        return;
      }
      if (action === "archive") {
        const session = store.sessions.get(this.#sessionId);
        if (session) {
          this.#api(() => api.post(`/sessions/${session.id}/archive`, { archived: !session.archived }));
        }
        return;
      }
      if (action === "resume") {
        this.#api(() => api.post(`/sessions/${this.#sessionId}/resume`));
        return;
      }
      if (action === "title") {
        this.#beginRename();
      }
    };
    this.on(this, "click", onAction);
    this.#mountTopbarSession(onAction);

    // Pressing anywhere in a panel (or tabbing into it) makes it the active panel and brings it
    // to the front, not only grabbing its header.
    const markActive = (ev) => {
      const id = ev.target.closest?.("dync-panel")?.dataset.group;
      if (!id) return;
      this.#setActiveGroup(id);
      this.#canvas()?.raise?.(id);
    };
    this.on(this, "pointerdown", markActive, true);
    this.on(this, "focusin", markActive);

    this.on(this, "dync-panel-activate", (ev) => {
      this.#layout = setActive(this.#layout, ev.detail.groupId, ev.detail.tool);
      this.#save();
      this.#paintLayout();
    });
    this.on(this, "dync-panel-opacity", (ev) => {
      const { groupId, opacity, idleOpacity } = ev.detail;
      this.#layout = setOpacity(this.#layout, groupId, opacity, idleOpacity);
      this.#save();
      this.#paintLayout();
    });
    this.on(this, "dync-panel-popout", () => this.#paintLayout());
    this.on(this, "dync-panel-bring-back", () => this.#paintLayout());

    this.on(this, "dync-rect-commit", (ev) => {
      const { id, rect } = ev.detail || {};
      if (!id || !rect) return;
      this.#layout = setRect(this.#layout, id, rect);
      this.#save();
    });
    this.on(this, "dync-rect-raise", (ev) => {
      const { id, rect } = ev.detail || {};
      if (!id || !rect) return;
      this.#layout = setRect(this.#layout, id, rect);
      this.#save();
    });

    this.on(this, "dync-tab-drag-start", (ev) => {
      this.#dragTool = ev.detail.tool;
    });
    this.on(this, "dync-tab-drag-move", (ev) => this.#onTabDragMove(ev.detail));
    this.on(this, "dync-tab-drag-end", (ev) => this.#onTabDragEnd(ev.detail));
    this.on(this, "dync-tab-drag-cancel", () => {
      this.#dragTool = null;
      this.#clearDropHints();
    });

    const unsubPop = popout.subscribe((msg) => {
      if (msg.sessionId !== this.#sessionId) return;
      if (msg.type === "open" || msg.type === "closed") {
        for (const panel of this.#panels.values()) {
          panel.setPopped(msg.tool, msg.type === "open");
        }
      }
    });
    this.cleanup(unsubPop);

    this.on(
      window,
      "keydown",
      (ev) => {
        if (!ev.altKey || ev.metaKey || ev.ctrlKey) return;
        if (ev.key === "0") {
          ev.preventDefault();
          ev.stopPropagation();
          void this.confirmLeave().then((ok) => {
            if (ok) route.go({ name: "board" });
          });
          return;
        }
        const n = Number(ev.key);
        if (n >= 1 && n <= 4) {
          ev.preventDefault();
          ev.stopPropagation();
          this.focusTool(TOOLS[n - 1]);
        }
      },
      true,
    );

    this.on(this, "dync-canvas-resize", () => {
      const canvas = this.#canvas();
      const size = canvas.size();
      if (size.width === this.#width && size.height === this.#height) return;
      this.#width = size.width;
      this.#height = size.height;
    });

    const unsub = store.subscribe(() => this.#paintHeader());
    this.cleanup(unsub);

    const canvas = this.#canvas();
    canvas.interactive = true;
    const size = canvas.size();
    this.#width = size.width || 1200;
    this.#height = size.height || 700;
    this.#loadLayout();
    this.#paintHeader();
    this.#paintLayout(true);
  }

  /**
   * The session's state, title, usage and actions live in the top bar's session slot (there is
   * no separate workspace header). The slot is emptied again when this workspace goes away.
   */
  #mountTopbarSession(onAction) {
    const slot = document.querySelector("dync-topbar")?.sessionSlot?.();
    if (!slot) return;
    slot._owner = this;
    slot.innerHTML = html`<button type="button" class="icon ghost topbar-back" data-action="back"
        title="Back to the Board" aria-label="Back to the Board">${raw(icon("back"))}</button>
      <span class="sc-dot" data-el="dot" aria-hidden="true"></span>
      <span class="ws-state" data-el="state"></span>
      <button type="button" class="ws-title ghost" data-action="title" data-el="title" title="Rename"></button>
      <span class="ws-meta" data-el="meta"></span>
      <span class="ws-usage" data-el="usage"></span>
      <span class="topbar-session-actions">
        <button type="button" class="ghost sm" data-action="stop" data-el="stop">Stop</button>
        <button type="button" class="ghost sm" data-action="archive" data-el="archive" hidden>Archive</button>
        <button type="button" class="ghost sm" data-action="resume" data-el="resume" hidden>Resume</button>
        <button type="button" class="ghost sm" data-action="reset" title="Reset layout">Reset layout</button>
      </span>`.value;
    slot.hidden = false;
    this.#header = slot;
    this.on(slot, "click", onAction);
    this.cleanup(() => {
      if (slot._owner !== this) return;
      slot._owner = null;
      slot.innerHTML = "";
      slot.hidden = true;
    });
  }

  /** Find a header control: in the top bar's session slot, or in the solo header. */
  #hq(sel) {
    return this.#header?.querySelector(sel) || this.querySelector(sel);
  }

  #mountSolo() {
    const tool = this.#soloTool;
    const tag =
      tool === "terminal"
        ? "dync-tool-terminal"
        : tool === "files"
          ? "dync-tool-files"
          : tool === "changes"
            ? "dync-tool-changes"
            : "dync-tool-session";

    this.render(
      html`<div class="workspace workspace-solo">
        <header class="ws-header ws-solo-header">
          <span class="sc-dot" data-el="dot" aria-hidden="true"></span>
          <span class="ws-state" data-el="state"></span>
          <span class="ws-title" data-el="title"></span>
          <button type="button" class="ghost sm" data-action="close-solo">${raw(icon("close"))} Close</button>
        </header>
        <div class="ws-solo-body" data-solo></div>
      </div>`,
    );

    const host = this.querySelector("[data-solo]");
    const el = document.createElement(tag);
    el.setAttribute("session", this.#sessionId);
    host.appendChild(el);

    popout.announce("open", { sessionId: this.#sessionId, tool });
    this.on(window, "pagehide", () => {
      popout.announce("closed", { sessionId: this.#sessionId, tool });
    });
    this.on(this, "click", (ev) => {
      if (ev.target.closest('[data-action="close-solo"]')) window.close();
    });

    const unsub = store.subscribe(() => this.#paintHeader());
    this.cleanup(unsub);
    this.#paintHeader();
  }

  #canvas() {
    return this.querySelector("[data-canvas]");
  }

  #prefsKey() {
    return `workspace:${this.#sessionId}`;
  }

  /**
   * Each session keeps its own layout. A session with none yet starts from the last
   * arranged layout (`workspace-seed`); if that is missing too, the product default.
   * Once copied, later seed updates from other sessions do not change this one.
   */
  #loadLayout() {
    const session = store.sessions.get(this.#sessionId);
    if (session?.pull && route.current().tool === "changes") {
      this.#layout = defaultLayout(this.#width, this.#height, { review: true });
      return;
    }
    const raw = prefs.get(this.#prefsKey(), null);
    if (raw) {
      this.#layout = normalize(raw, this.#width, this.#height);
      return;
    }
    const seed = prefs.get("workspace-seed", null);
    this.#layout = normalize(seed, this.#width, this.#height);
    // Own a copy so this session stays independent of later seed changes.
    prefs.set(this.#prefsKey(), this.#layout);
  }

  #save() {
    prefs.set(this.#prefsKey(), this.#layout);
    // Next new session starts from whatever was last arranged (including Reset layout).
    prefs.set("workspace-seed", this.#layout);
  }

  async confirmLeave() {
    const files = this.querySelector("dync-tool-files");
    if (files && typeof files.hasUnsaved === "function" && files.hasUnsaved()) {
      return askConfirm({
        title: "Unsaved edits",
        message: "Leave without saving?",
        confirmLabel: "Leave",
        danger: true,
      });
    }
    return true;
  }

  async #api(fn) {
    try {
      await fn();
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : "Something went wrong.", { kind: "error" });
    }
  }

  #paintHeader() {
    const s = store.sessions.get(this.#sessionId);
    const gone = this.querySelector("[data-gone]");
    const canvas = this.#canvas();
    if (!s) {
      if (gone) gone.hidden = false;
      if (canvas) canvas.hidden = true;
      const header = this.#header;
      if (header && !this.#solo) {
        for (const el of header.querySelectorAll("[data-el], [data-action='reset']")) {
          el.hidden = true;
        }
      }
      return;
    }
    if (gone) gone.hidden = true;
    if (canvas) canvas.hidden = false;
    if (this.#header && !this.#solo) {
      for (const el of this.#header.querySelectorAll(
        "[data-el=dot], [data-el=state], [data-el=title], [data-el=meta], [data-action='reset']",
      )) {
        el.hidden = false;
      }
    }

    const dot = this.#hq('[data-el="dot"]');
    if (dot) dot.dataset.state = s.state || "";
    const state = this.#hq('[data-el="state"]');
    if (state) state.textContent = stateLabel(s);
    const title = this.#hq('[data-el="title"]');
    if (title && !this.#renaming) title.textContent = s.title || "Untitled session";

    const meta = this.#hq('[data-el="meta"]');
    if (meta) {
      meta.textContent = [agentLabel(store.meta, s.agent), s.model].filter(Boolean).join(" · ");
    }
    const usage = this.#hq('[data-el="usage"]');
    if (usage) {
      const u = usageLabel(s.usage);
      usage.textContent = u;
      usage.hidden = !u;
    }
    const stop = this.#hq('[data-el="stop"]');
    const resume = this.#hq('[data-el="resume"]');
    if (stop) stop.hidden = !s.live;
    if (resume) resume.hidden = !!s.live || s.archived || s.state !== "ended";
    const archive = this.#hq('[data-el="archive"]');
    if (archive) {
      archive.hidden = !!s.live || s.state !== "ended";
      archive.textContent = s.archived ? "Restore" : "Archive";
    }
  }

  #paintLayout(animate = false) {
    const canvas = this.#canvas();
    if (!canvas || !this.#layout) return;
    const items = this.#layout.groups.map((g) => ({
      id: g.id,
      x: g.rect.x,
      y: g.rect.y,
      w: g.rect.w,
      h: g.rect.h,
      z: g.rect.z,
    }));
    let maxX = 0;
    let maxY = 0;
    for (const r of items) {
      maxX = Math.max(maxX, r.x + r.w);
      maxY = Math.max(maxY, r.y + r.h);
    }
    canvas.applyLayout({
      items,
      canvas: { w: maxX + 400, h: maxY + 400 },
      animate,
    });

    const seen = new Set();
    for (const g of this.#layout.groups) {
      seen.add(g.id);
      let panel = this.#panels.get(g.id);
      if (!panel) {
        const wrap = canvas.ensureItem(g.id);
        panel = document.createElement("dync-panel");
        wrap.appendChild(panel);
        this.#panels.set(g.id, panel);
      }
      panel.sessionId = this.#sessionId;
      panel.group = g;
      panel.classList.toggle("is-active", g.id === this.#activeGroup);
      for (const tool of g.tabs) {
        if (popout.isPoppedOut(this.#sessionId, tool)) panel.setPopped(tool, true);
      }
    }
    for (const [id, panel] of this.#panels) {
      if (!seen.has(id)) {
        panel.remove();
        canvas.removeItem(id);
        this.#panels.delete(id);
      }
    }
  }

  #setActiveGroup(id) {
    if (this.#activeGroup === id) return;
    this.#activeGroup = id;
    for (const [gid, panel] of this.#panels) panel.classList.toggle("is-active", gid === id);
    this.classList.toggle("has-active-panel", !!id);
  }

  focusTool(tool) {
    if (!this.#layout) return;
    for (const g of this.#layout.groups) {
      if (g.tabs.includes(tool)) {
        this.#layout = setActive(this.#layout, g.id, tool);
        this.#activeGroup = g.id;
        this.classList.add("has-active-panel");
        this.#save();
        this.#paintLayout();
        this.#canvas()?.scrollIntoViewId(g.id);
        return;
      }
    }
  }

  #onTabDragMove({ x, y }) {
    this.#clearDropHints();
    const hit = this.#hitTest(x, y);
    if (!hit) return;
    if (hit.kind === "tabs") {
      hit.el.classList.add("drop-target");
      if (hit.before) hit.before.classList.add("drop-before");
    } else if (hit.kind === "body") {
      hit.el.classList.add("drop-body");
    }
  }

  #onTabDragEnd({ tool, x, y }) {
    this.#clearDropHints();
    this.#dragTool = null;
    const hit = this.#hitTest(x, y);
    if (!hit) {
      // Tear out at pointer, mapped into canvas coords
      const canvas = this.#canvas();
      const vp = canvas?.viewport;
      const bounds = vp?.getBoundingClientRect();
      const cx = bounds ? x - bounds.left + vp.scrollLeft : 40;
      const cy = bounds ? y - bounds.top + vp.scrollTop : 40;
      this.#layout = tearOut(this.#layout, tool, { x: cx, y: cy, w: 520, h: 360 });
      this.#save();
      this.#paintLayout(true);
      return;
    }
    if (hit.kind === "tabs" || hit.kind === "body") {
      this.#layout = moveTab(this.#layout, tool, hit.groupId, hit.index ?? 999);
      this.#save();
      this.#paintLayout(true);
    }
  }

  #hitTest(x, y) {
    const el = document.elementFromPoint(x, y);
    if (!el || !this.contains(el)) return null;
    const panel = el.closest("dync-panel");
    if (!panel) return null;
    const groupId = panel.dataset.group;
    const tabs = panel.querySelector("[data-tabs]");
    const body = panel.querySelector("[data-body]");
    if (tabs && (el === tabs || tabs.contains(el))) {
      const tabEls = [...tabs.querySelectorAll("[data-tool]")];
      let index = tabEls.length;
      let before = null;
      for (let i = 0; i < tabEls.length; i++) {
        const r = tabEls[i].getBoundingClientRect();
        if (x < r.left + r.width / 2) {
          index = i;
          before = tabEls[i];
          break;
        }
      }
      return { kind: "tabs", groupId, index, el: tabs, before };
    }
    if (body && (el === body || body.contains(el))) {
      return { kind: "body", groupId, index: 999, el: body };
    }
    return null;
  }

  #clearDropHints() {
    for (const el of this.querySelectorAll(".drop-target, .drop-before, .drop-body")) {
      el.classList.remove("drop-target", "drop-before", "drop-body");
    }
  }

  #beginRename() {
    const s = store.sessions.get(this.#sessionId);
    if (!s || this.#renaming) return;
    const btn = this.#hq('[data-el="title"]');
    if (!btn) return;
    const input = document.createElement("input");
    input.className = "ws-title-input";
    input.value = s.title || "";
    this.#renaming = true;
    btn.replaceWith(input);
    input.focus();
    input.select();
    const finish = (save) => {
      if (!this.#renaming) return;
      this.#renaming = false;
      const next = input.value.trim();
      input.replaceWith(btn);
      if (save && next && next !== (s.title || "")) {
        this.#api(() => api.post(`/sessions/${s.id}/title`, { title: next }));
      } else {
        this.#paintHeader();
      }
    };
    input.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter") {
        ev.preventDefault();
        finish(true);
      } else if (ev.key === "Escape") {
        ev.preventDefault();
        finish(false);
      }
    });
    input.addEventListener("blur", () => finish(true));
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-workspace")) {
  customElements.define("dync-workspace", DyncWorkspace);
}
