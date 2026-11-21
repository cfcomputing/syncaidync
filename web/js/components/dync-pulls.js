import { DyncElement, html, escapeText, raw } from "../lib/element.js";
import { api, ApiError } from "../lib/api.js";
import { store } from "../lib/store.js";
import { icon } from "../lib/icons.js";
import { openMenuBelow } from "../lib/menu.js";
import { showToast } from "../lib/toast.js";
import { askConfirm } from "../lib/prompt.js";
import * as route from "../lib/route.js";
import { stateBadge, progressLabel, sortPulls } from "../lib/pulls.js";

export class DyncPulls extends DyncElement {
  #error = null;
  #opening = false;

  mount() {
    this.classList.add("pulls-page");
    this.#paint();
    this.on(this, "click", (ev) => this.#onClick(ev));
    this.on(this, "keydown", (ev) => {
      if (ev.key === "Enter" && ev.target.matches?.("[data-ref]")) {
        ev.preventDefault();
        this.#open();
      }
    });
    const unsub = store.subscribe(() => this.#paint());
    this.cleanup(unsub);
  }

  #paint() {
    const sorted = sortPulls(store.pulls);
    const active = sorted.filter((p) => p.status !== "done");
    const done = sorted.filter((p) => p.status === "done");
    const refVal = this.querySelector("[data-ref]")?.value ?? "";

    this.render(
      html`<div class="pulls-root">
        <header class="pulls-head">
          <h1>Pull requests</h1>
          <p class="pulls-lead">Review someone else's pull request with the same Changes tools.</p>
        </header>
        <form class="pulls-open" data-open-form>
          <label class="pulls-open-field">
            <span class="visually-hidden">Pull request</span>
            <input
              type="text"
              data-ref
              placeholder="Paste a pull request URL or owner/repo#123"
              value="${escapeText(refVal)}"
              autocomplete="off"
              spellcheck="false"
              ${this.#opening ? "disabled" : ""}
            />
          </label>
          <button type="button" class="primary" data-act="open" ${this.#opening ? "disabled" : ""}>
            ${this.#opening ? "Opening…" : "Open"}
          </button>
        </form>
        ${this.#error
          ? html`<p class="pulls-error" role="alert">${escapeText(this.#error)}</p>`
          : ""}
        ${!sorted.length
          ? html`<p class="pulls-empty">Open a GitHub pull request to read its diff, leave notes, and save a draft review.</p>`
          : html`<div class="pulls-groups">
              ${active.length ? this.#group("Active", active) : ""}
              ${done.length ? this.#group("Done", done) : ""}
            </div>`}
      </div>`,
    );
  }

  #group(title, rows) {
    return html`<section class="pulls-group">
      <h2>${escapeText(title)}</h2>
      <ul class="pulls-list">
        ${raw(rows.map((p) => this.#row(p)).join(""))}
      </ul>
    </section>`;
  }

  #row(p) {
    const badge = stateBadge(p);
    const prog = progressLabel(p.progress);
    return `<li class="pulls-row" data-act="open-row" data-id="${escapeText(p.id)}">
      <div class="pulls-row-main">
        <span class="pulls-ref">${escapeText(p.owner)}/${escapeText(p.repo)} #${escapeText(String(p.number))}</span>
        <span class="pulls-title">${escapeText(p.title || "(no title)")}</span>
        <span class="pulls-meta">
          <span class="pulls-author">${escapeText(p.author || "")}</span>
          <span class="pulls-badge" data-state="${escapeText(badge.toLowerCase())}">${escapeText(badge)}</span>
          <span class="pulls-progress">${escapeText(prog)}</span>
        </span>
      </div>
      <button type="button" class="ghost icon sm" data-act="menu" data-id="${escapeText(p.id)}" title="Menu" aria-label="Menu">${icon("dots")}</button>
    </li>`;
  }

  #onClick(ev) {
    const act = ev.target.closest("[data-act]")?.getAttribute("data-act");
    if (!act) return;
    if (act === "open") {
      ev.preventDefault();
      this.#open();
      return;
    }
    if (act === "open-row") {
      const id = ev.target.closest("[data-id]")?.getAttribute("data-id");
      if (id && !ev.target.closest("[data-act=menu]")) {
        route.go({ name: "pull", id });
      }
      return;
    }
    if (act === "menu") {
      ev.preventDefault();
      ev.stopPropagation();
      const id = ev.target.closest("[data-id]")?.getAttribute("data-id");
      const pull = id ? store.pulls.get(id) : null;
      const btn = ev.target.closest("[data-act=menu]");
      if (!pull || !btn) return;
      const items =
        pull.status === "done"
          ? [
              { id: "active", label: "Mark active" },
              { id: "remove", label: "Remove", danger: true },
            ]
          : [
              { id: "done", label: "Mark done" },
              { id: "remove", label: "Remove", danger: true },
            ];
      openMenuBelow(btn, items, (pick) => this.#menu(pull, pick));
    }
  }

  async #open() {
    if (this.#opening) return;
    const input = this.querySelector("[data-ref]");
    const ref = input?.value?.trim() || "";
    if (!ref) {
      this.#error = "Paste a pull request URL or owner/repo#123.";
      this.#paint();
      return;
    }
    this.#opening = true;
    this.#error = null;
    this.#paint();
    try {
      const pull = await api.post("/pulls/open", { ref });
      store.pulls.set(pull.id, pull);
      route.go({ name: "pull", id: pull.id });
    } catch (err) {
      this.#error = err instanceof ApiError ? err.message : "Could not open that pull request.";
      this.#opening = false;
      this.#paint();
      this.querySelector("[data-ref]")?.focus();
    }
  }

  async #menu(pull, pick) {
    try {
      if (pick === "done" || pick === "active") {
        const updated = await api.patch(`/pulls/${pull.id}`, {
          status: pick === "done" ? "done" : "active",
        });
        store.pulls.set(updated.id, updated);
        this.#paint();
        return;
      }
      if (pick === "remove") {
        if (
          !(await askConfirm({
            title: "Remove pull request",
            message: `Remove ${pull.owner}/${pull.repo} #${pull.number} from the list?`,
            confirmLabel: "Remove",
            danger: true,
          }))
        ) {
          return;
        }
        await api.del(`/pulls/${pull.id}`);
        store.pulls.delete(pull.id);
        this.#paint();
      }
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : "Something went wrong.", {
        kind: "error",
      });
    }
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-pulls")) {
  customElements.define("dync-pulls", DyncPulls);
}
