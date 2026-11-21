import { DyncElement, html, raw } from "../lib/element.js";
import { icon } from "../lib/icons.js";
import { formatDuration } from "../lib/time.js";
import {
  stateLabel,
  blockedLabel,
  agentLabel,
  usageLabel,
  workspaceLabel,
  toolLabel,
  inQueueOrSnoozed,
} from "../lib/labels.js";
import { needsYou } from "../lib/queue.js";
import { sessionLinks } from "../lib/session-links.js";
import { openMenuBelow } from "../lib/menu.js";
import "./dync-terminal.js";
import "./dync-goal-panel.js";

export class DyncSessionCard extends DyncElement {
  #session = null;
  #meta = null;
  #focused = false;
  #renaming = false;
  #built = false;
  #linksSession = null;

  get session() {
    return this.#session;
  }
  set session(v) {
    this.#session = v;
    this.update();
  }

  get meta() {
    return this.#meta;
  }
  set meta(v) {
    this.#meta = v;
    this.update();
  }

  get collapsed() {
    return this.hasAttribute("collapsed");
  }
  set collapsed(v) {
    this.toggleAttribute("collapsed", !!v);
  }

  get focused() {
    return this.#focused;
  }
  set focused(v) {
    this.#focused = !!v;
    this.classList.toggle("is-focused", this.#focused);
  }

  static get observedAttributes() {
    return ["collapsed"];
  }

  attributeChangedCallback() {
    this.update();
  }

  mount() {
    this.classList.add("session-card", "card");
    this.setAttribute("tabindex", "0");
    this.#renderFrame();
    this.#built = true;

    this.on(this, "click", (ev) => {
      const action = ev.target.closest("[data-action]")?.getAttribute("data-action");
      if (!action) {
        this.#emit("dync-focus");
        return;
      }
      ev.stopPropagation();
      if (action === "collapse") {
        this.#emit("dync-collapse", { collapsed: true });
      } else if (action === "menu") {
        this.#openMenu(ev.target.closest("[data-action]"));
      } else if (action === "open") {
        this.#emit("dync-open");
      } else if (action === "snooze") {
        this.#emit("dync-snooze", { snoozed: !this.#session?.snoozed });
      } else if (action === "stop") {
        this.#emit("dync-stop");
      } else if (action === "title") {
        this.#beginRename();
      }
    });

    this.on(this, "keydown", (ev) => {
      if (this.#renaming) return;
      if (ev.key === "F2") {
        ev.preventDefault();
        this.#beginRename();
      }
    });

    this.every(10_000, () => this.#paintTime());
    this.update();
  }

  unmount() {
    this.#linksSession = null;
  }

  #renderFrame() {
    this.render(
      html`<div class="sc-header" data-drag>
        <span class="sc-dot" data-el="dot" aria-hidden="true"></span>
        <span class="sc-state" data-el="state"></span>
        <button type="button" class="sc-title ghost" data-action="title" data-el="title"></button>
        <div class="sc-header-controls">
          <span class="sc-time" data-el="time"></span>
          <button type="button" class="icon" data-action="collapse" title="Collapse" aria-label="Collapse">${raw(icon("chevron-down"))}</button>
          <button type="button" class="icon" data-action="menu" title="More" aria-label="More">${raw(icon("dots"))}</button>
        </div>
        <div class="sc-details">
          <div class="sc-sub">
            <span data-el="agent"></span>
            <span data-el="usage"></span>
          </div>
          <div class="sc-header-actions">
            <div class="sc-header-links" data-el="links"></div>
            <button type="button" class="icon" data-action="snooze" data-el="snooze" hidden></button>
            <button type="button" class="icon" data-action="stop" data-el="stop" title="Stop" aria-label="Stop" hidden>${raw(icon("stop"))}</button>
          </div>
        </div>
      </div>
      <div class="sc-blocked" data-el="blocked" hidden></div>
      <dync-goal-panel compact data-el="goal" hidden></dync-goal-panel>
      <dync-terminal mode="view" archive-action data-el="term"></dync-terminal>
      <div class="sc-footer">
        <span class="sc-workspace" data-el="workspace"></span>
        <span class="sc-tool" data-el="tool"></span>
        <button type="button" class="ghost sm" data-action="open">Open Session</button>
      </div>`,
    );
  }

  update() {
    if (!this.#built || !this.#session) return;
    const s = this.#session;
    this.dataset.id = s.id;
    this.dataset.state = s.state || "";
    this.classList.toggle("is-blocked", s.state === "blocked");
    this.classList.toggle("is-ready", s.state === "ready" && needsYou(s));
    this.classList.toggle("is-ended", s.state === "ended");
    this.classList.toggle("is-snoozed", !!s.snoozed);
    this.hidden = !!this.collapsed;

    const approximate =
      this.#meta?.agents?.find((a) => a.id === s.agent)?.observes === "activity";

    const stateEl = this.querySelector('[data-el="state"]');
    stateEl.textContent = stateLabel(s);
    stateEl.title = approximate ? "Approximate — based on terminal activity" : "";

    const dot = this.querySelector('[data-el="dot"]');
    dot.dataset.state = s.state || "";

    if (!this.#renaming) {
      const titleBtn = this.querySelector('[data-el="title"]');
      titleBtn.textContent = s.title || "Untitled session";
      titleBtn.title = s.title || "Untitled session";
    }

    this.#paintTime();

    this.querySelector('[data-el="agent"]').textContent = [
      agentLabel(this.#meta, s.agent),
      s.model,
    ]
      .filter(Boolean)
      .join(" · ");

    const usage = usageLabel(s.usage, { named: true });
    const usageEl = this.querySelector('[data-el="usage"]');
    usageEl.textContent = usage;
    usageEl.hidden = !usage;

    const blocked = this.querySelector('[data-el="blocked"]');
    if (s.state === "blocked" && s.blocked) {
      blocked.hidden = false;
      blocked.textContent = blockedLabel(s.blocked);
      blocked.title = blockedLabel(s.blocked);
    } else {
      blocked.hidden = true;
      blocked.textContent = "";
    }

    const goalEl = this.querySelector('[data-el="goal"]');
    if (s.goal) {
      if (goalEl.getAttribute("session") !== s.id) goalEl.setAttribute("session", s.id);
    } else {
      goalEl.hidden = true;
    }

    const term = this.querySelector("dync-terminal");
    term.toggleAttribute("fit-preview", !!s.live);
    if (term.getAttribute("session") !== s.id) term.setAttribute("session", s.id);

    this.querySelector('[data-el="workspace"]').textContent = workspaceLabel(s.workspace);
    this.querySelector('[data-el="workspace"]').title = s.workspace?.path || "";
    const tools = s.tools || [];
    const last = tools[tools.length - 1];
    this.querySelector('[data-el="tool"]').textContent = toolLabel(last);

    const showSnooze = inQueueOrSnoozed(s);
    const snoozeBtn = this.querySelector('[data-el="snooze"]');
    snoozeBtn.hidden = !showSnooze;
    const snoozeLabel = s.snoozed ? "Wake" : "Snooze";
    if (snoozeBtn.title !== snoozeLabel) {
      snoozeBtn.title = snoozeLabel;
      snoozeBtn.setAttribute("aria-label", snoozeLabel);
      snoozeBtn.innerHTML = icon(s.snoozed ? "bell" : "snooze");
    }

    const live = !!s.live;
    this.querySelector('[data-el="stop"]').hidden = !live;
    if (this.#linksSession !== s.id) this.#loadLinks(s.id);
  }

  async #loadLinks(id) {
    this.#linksSession = id;
    const headerLinks = this.querySelector('[data-el="links"]');
    headerLinks.replaceChildren();
    const links = await sessionLinks(id);
    if (!this.isConnected || this.#session?.id !== id || !this.contains(headerLinks)) return;

    for (const { label, url, iconName } of links) {
      const link = document.createElement("a");
      link.href = url;
      link.target = "_blank";
      link.rel = "noopener";
      link.className = "icon";
      link.title = label;
      link.setAttribute("aria-label", label);
      link.innerHTML = icon(iconName);
      headerLinks.append(link);
    }
  }

  #paintTime() {
    const s = this.#session;
    if (!s) return;
    const el = this.querySelector('[data-el="time"]');
    if (!el) return;
    el.textContent = formatDuration(Date.now() - (s.state_since ?? s.updated_at ?? Date.now()));
  }

  #beginRename() {
    if (this.#renaming || !this.#session) return;
    const titleBtn = this.querySelector('[data-el="title"]');
    const input = document.createElement("input");
    input.className = "sc-title-input";
    input.value = this.#session.title || "";
    input.setAttribute("aria-label", "Session title");
    this.#renaming = true;
    titleBtn.replaceWith(input);
    input.focus();
    input.select();

    const finish = (save) => {
      if (!this.#renaming) return;
      this.#renaming = false;
      const next = input.value.trim();
      input.replaceWith(titleBtn);
      if (save && next && next !== (this.#session.title || "")) {
        this.#emit("dync-rename", { title: next });
      } else {
        this.update();
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

  #openMenu(anchor) {
    const s = this.#session;
    if (!s) return;
    const items = [
      ...(s.live ? [{ id: "stop", label: "Stop" }] : []),
      ...(!s.live && !s.archived ? [{ id: "resume", label: "Resume" }] : []),
      ...(!s.live && s.state === "ended"
        ? [{ id: "archive", label: s.archived ? "Restore" : "Archive" }]
        : []),
      { id: "remove", label: "Remove", danger: true },
    ];
    openMenuBelow(anchor, items, (id) => {
      if (id === "stop") this.#emit("dync-stop");
      else if (id === "resume") this.#emit("dync-resume");
      else if (id === "archive") this.#emit("dync-archive", { archived: !s.archived });
      else if (id === "remove") this.#emit("dync-remove");
    });
  }

  #emit(name, detail = {}) {
    this.dispatchEvent(
      new CustomEvent(name, {
        bubbles: true,
        composed: true,
        detail: { id: this.#session?.id, ...detail },
      }),
    );
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-session-card")) {
  customElements.define("dync-session-card", DyncSessionCard);
}
