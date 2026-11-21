import { DyncElement, html, raw } from "../lib/element.js";
import { icon } from "../lib/icons.js";
import { formatDuration } from "../lib/time.js";
import { stateLabel } from "../lib/labels.js";
import { needsYou } from "../lib/queue.js";
import { pillText } from "../lib/goal.js";

export class DyncSessionStrip extends DyncElement {
  #session = null;
  #built = false;

  get session() {
    return this.#session;
  }
  set session(v) {
    this.#session = v;
    this.update();
  }

  get collapsed() {
    return this.hasAttribute("collapsed");
  }
  set collapsed(v) {
    this.toggleAttribute("collapsed", !!v);
  }

  static get observedAttributes() {
    return ["collapsed"];
  }

  attributeChangedCallback() {
    this.update();
  }

  mount() {
    this.classList.add("session-strip");
    this.render(
      html`<span class="sc-dot" data-el="dot" aria-hidden="true"></span>
        <span class="strip-title" data-el="title"></span>
        <span class="strip-sep">·</span>
        <span class="strip-state" data-el="state"></span>
        <span class="strip-sep">·</span>
        <span class="strip-time" data-el="time"></span>
        <button type="button" class="icon strip-expand" data-action="expand" title="Expand" aria-label="Expand"><span data-strip-action></span>${raw(icon("chevron-right"))}</button>`,
    );
    this.#built = true;
    this.on(this, "click", (ev) => {
      const action = ev.target.closest("button")?.dataset.action;
      if (action === "expand") {
        this.#emit("dync-collapse", { collapsed: false });
        return;
      }
      if (action === "wake") {
        this.#emit("dync-snooze", { snoozed: false });
        return;
      }
      this.#emit("dync-focus");
      this.#emit("dync-open");
    });
    this.every(10_000, () => this.update());
    this.update();
  }

  update() {
    if (!this.#built || !this.#session) return;
    const s = this.#session;
    this.dataset.id = s.id;
    this.dataset.state = s.state || "";
    this.classList.toggle("is-blocked", s.state === "blocked");
    this.classList.toggle("is-ready", s.state === "ready" && needsYou(s));
    this.classList.toggle("is-snoozed", !!s.snoozed);
    this.hidden = !this.collapsed;
    const expand = this.querySelector(".strip-expand");
    const action = s.snoozed ? "Wake" : "Expand";
    expand.dataset.action = action.toLowerCase();
    expand.title = action;
    expand.setAttribute("aria-label", action);
    expand.querySelector("[data-strip-action]").textContent = action;

    this.querySelector('[data-el="dot"]').dataset.state = s.state || "";
    this.querySelector('[data-el="title"]').textContent = s.title || "Untitled session";
    this.querySelector('[data-el="title"]').title = s.title || "Untitled session";
    this.querySelector('[data-el="state"]').textContent = s.goal
      ? `${stateLabel(s)} · ${pillText(s.goal)}`
      : stateLabel(s);
    this.querySelector('[data-el="time"]').textContent = formatDuration(
      Date.now() - (s.state_since ?? s.updated_at ?? Date.now()),
    );
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

if (typeof customElements !== "undefined" && !customElements.get("dync-session-strip")) {
  customElements.define("dync-session-strip", DyncSessionStrip);
}
