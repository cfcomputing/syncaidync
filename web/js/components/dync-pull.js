import { DyncElement, html, escapeText, raw } from "../lib/element.js";
import { api, ApiError } from "../lib/api.js";
import { store } from "../lib/store.js";
import { icon } from "../lib/icons.js";
import { fromServerHtml } from "../lib/markdown.js";
import { openDialog, setBusy } from "../lib/dialog.js";
import { showToast } from "../lib/toast.js";
import { unsent } from "../lib/review.js";
import { stateBadge } from "../lib/pulls.js";
import * as route from "../lib/route.js";
import "./dync-changes-view.js";

export class DyncPull extends DyncElement {
  static get observedAttributes() {
    return ["pull-id"];
  }

  #bodyOpen = false;
  #busy = false;
  #review = null;
  #paintedPull = null;
  #paintedAgents = null;
  #paintedSession = false;

  mount() {
    this.classList.add("pull-page");
    this.#paint();
    this.on(this, "click", (ev) => this.#onClick(ev));
    const unsub = store.subscribe(() => {
      const pull = this.#pull;
      const hasSession = !!pull?.session && store.sessions.has(pull.session);
      if (pull !== this.#paintedPull || store.meta?.agents !== this.#paintedAgents ||
          hasSession !== this.#paintedSession) this.#paint();
    });
    this.cleanup(unsub);
    this.on(this, "close", () => this.#paint(), true);
    this.on(this, "dync-review-saved", (ev) => {
      this.#review = ev.detail;
      this.#paint();
    });
    this.#loadReview();
  }

  attributeChangedCallback() {
    if (!this.isConnected) return;
    this.#bodyOpen = false;
    this.#review = null;
    this.#paint();
    this.#loadReview();
  }

  get #id() {
    return this.getAttribute("pull-id") || "";
  }

  get #pull() {
    return store.pulls.get(this.#id) || null;
  }

  async #loadReview() {
    const id = this.#id;
    if (!id) return;
    try {
      const review = await api.get(`/pulls/${id}/review`);
      if (!this.isConnected || this.#id !== id) return;
      this.#review = review;
      this.#paint();
    } catch {
      /* keep last */
    }
  }

  #paint() {
    if (this.querySelector("dialog[open]")) return;
    const pull = this.#pull;
    this.#paintedPull = pull;
    this.#paintedAgents = store.meta?.agents;
    this.#paintedSession = !!pull?.session && store.sessions.has(pull.session);
    if (!pull) {
      this.render(
        html`<div class="pull-missing">
          <p>That pull request is not on the list.</p>
          <button type="button" class="ghost" data-act="back">Back to pull requests</button>
        </div>`,
      );
      return;
    }

    const badge = stateBadge(pull);
    const sessionAlive = this.#paintedSession;
    const notes = unsent(this.#review || { notes: [] });
    const hasOverall = !!(this.#review?.overall || "").trim();
    const canDraft = notes.length > 0 || hasOverall;
    const agents = (store.meta?.agents || []).filter((a) => a.installed && a.id !== "shell");

    if (!this.querySelector(".pull-root")) {
      this.render(html`<div class="pull-root">
        <div class="pull-details"></div>
        <dync-changes-view source-kind="pull"></dync-changes-view>
      </div>`);
    }
    const changes = this.querySelector("dync-changes-view");
    if (changes.getAttribute("source-id") !== pull.id) changes.setAttribute("source-id", pull.id);
    this.querySelector(".pull-details").innerHTML = html`<header class="pull-head">
          <button type="button" class="ghost sm" data-act="back">${raw(icon("back"))} Pull requests</button>
          <div class="pull-head-main">
            <h1>${escapeText(pull.title || "(no title)")}</h1>
            <div class="pull-head-meta">
              <a class="pull-link" href="${escapeText(pull.url)}" target="_blank" rel="noopener noreferrer">
                ${escapeText(pull.owner)}/${escapeText(pull.repo)} #${escapeText(String(pull.number))}
                ${raw(icon("external"))}
              </a>
              <span class="pulls-author">${escapeText(pull.author || "")}</span>
              <span class="pulls-badge" data-state="${escapeText(badge.toLowerCase())}">${escapeText(badge)}</span>
              <span class="pull-branches">${escapeText(pull.base_ref)} ← ${escapeText(pull.head_ref)}</span>
            </div>
          </div>
          <div class="pull-actions">
            <button type="button" class="ghost sm" data-act="refresh" ${this.#busy ? "disabled" : ""}>Refresh</button>
            <button type="button" class="ghost sm" data-act="toggle-status">
              ${pull.status === "done" ? "Mark active" : "Mark done"}
            </button>
            <button type="button" class="primary sm" data-act="session">
              ${sessionAlive ? "Open review session" : "Start review session"}
            </button>
            <button type="button" class="ghost sm" data-act="draft" ${canDraft && !this.#busy ? "" : "disabled"}>
              Save as draft review on GitHub
            </button>
          </div>
        </header>

        <section class="pull-body ${this.#bodyOpen ? "is-open" : ""}">
          <div class="pull-body-html md-body md-body-compact" data-body>${fromServerHtml(pull.body_html || "")}</div>
          <button type="button" class="ghost sm" data-act="toggle-body">
            ${this.#bodyOpen ? "Show less" : "Show more"}
          </button>
        </section>

        <dialog class="pull-session-dialog" data-session-dialog>
          <form method="dialog" class="dialog-body" data-session-form>
            <h2>Start review session</h2>
            <p>Opens a worktree at the pull request head with Changes and Terminal side by side.</p>
            <fieldset>
              <legend>Agent</legend>
              <div class="choice-row">
                ${raw(
                  agents
                    .map(
                      (a, i) =>
                        `<button type="button" class="choice ${i === 0 ? "on" : ""}" data-agent="${escapeText(a.id)}">${escapeText(a.label || a.id)}</button>`,
                    )
                    .join(""),
                )}
              </div>
            </fieldset>
            <label>
              <span>Model</span>
              <input name="model" placeholder="Agent default" autocomplete="off" />
            </label>
            <label class="check">
              <input type="checkbox" name="autonomy" />
              <span>Run without approval prompts</span>
            </label>
            <p class="error" data-session-error hidden></p>
            <div class="dialog-actions">
              <button type="submit" value="cancel" class="ghost">Cancel</button>
              <button type="submit" value="start" class="primary" ${agents.length ? "" : "disabled"}>Start</button>
            </div>
          </form>
        </dialog>

        <dialog class="pull-draft-dialog" data-draft-dialog>
          <form method="dialog" class="dialog-body" data-draft-form>
            <h2>Save as draft review</h2>
            <p data-draft-summary></p>
            <p>The review stays pending on GitHub until you submit it there.</p>
            <div class="dialog-actions">
              <button type="submit" value="cancel" class="ghost">Cancel</button>
              <button type="submit" value="save" class="primary">Save draft</button>
            </div>
          </form>
        </dialog>
      `.value;

    this.#secureBodyLinks();
  }

  #secureBodyLinks() {
    const host = this.querySelector("[data-body]");
    if (!host) return;
    for (const a of host.querySelectorAll("a[href]")) {
      a.target = "_blank";
      a.rel = "noopener noreferrer";
    }
  }

  #onClick(ev) {
    if (!ev.target.closest(".pull-details")) return;
    const agentBtn = ev.target.closest("[data-agent]");
    if (agentBtn) {
      this.querySelectorAll("[data-agent]").forEach((b) => b.classList.remove("on"));
      agentBtn.classList.add("on");
      return;
    }

    const act = ev.target.closest("[data-act]")?.getAttribute("data-act");
    if (!act) return;
    if (act === "back") {
      route.go({ name: "pulls" });
      return;
    }
    if (act === "toggle-body") {
      this.#bodyOpen = !this.#bodyOpen;
      this.#paint();
      return;
    }
    if (act === "refresh") {
      this.#refresh();
      return;
    }
    if (act === "toggle-status") {
      this.#toggleStatus();
      return;
    }
    if (act === "session") {
      this.#session();
      return;
    }
    if (act === "draft") {
      this.#draft();
    }
  }

  async #refresh() {
    const id = this.#id;
    if (!id || this.#busy) return;
    this.#busy = true;
    this.#paint();
    try {
      await api.get(`/pulls/${id}/changes?refresh=1`);
      await this.#loadReview();
      await this.querySelector("dync-changes-view")?.reload();
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : "Refresh failed.", { kind: "error" });
    } finally {
      this.#busy = false;
      this.#paint();
    }
  }

  async #toggleStatus() {
    const pull = this.#pull;
    if (!pull) return;
    try {
      const updated = await api.patch(`/pulls/${pull.id}`, {
        status: pull.status === "done" ? "active" : "done",
      });
      store.pulls.set(updated.id, updated);
      this.#paint();
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : "Could not update status.", {
        kind: "error",
      });
    }
  }

  async #session() {
    const pull = this.#pull;
    if (!pull) return;
    if (pull.session && store.sessions.has(pull.session)) {
      route.go({ name: "session", id: pull.session, tool: "changes" });
      return;
    }
    const dialog = this.querySelector("[data-session-dialog]");
    const form = this.querySelector("[data-session-form]");
    const errEl = this.querySelector("[data-session-error]");
    if (!dialog || !form) return;
    errEl.hidden = true;
    errEl.textContent = "";
    openDialog(dialog);

    form.onsubmit = async (ev) => {
      if (ev.submitter?.value === "cancel") return;
      ev.preventDefault();
      const agent =
        form.querySelector("[data-agent].on")?.getAttribute("data-agent") ||
        form.querySelector("[data-agent]")?.getAttribute("data-agent");
      if (!agent) return;
      const model = form.querySelector('[name="model"]')?.value?.trim() || null;
      const autonomy = !!form.querySelector('[name="autonomy"]')?.checked;
      setBusy(form, true);
      try {
        const session = await api.post(`/pulls/${pull.id}/session`, {
          agent,
          model,
          autonomy,
        });
        store.sessions.set(session.id, session);
        const updated = { ...pull, session: session.id };
        store.pulls.set(pull.id, updated);
        dialog.close();
        route.go({ name: "session", id: session.id, tool: "changes" });
      } catch (err) {
        errEl.hidden = false;
        errEl.textContent =
          err instanceof ApiError ? err.message : "Could not start a session.";
      } finally {
        setBusy(form, false);
      }
    };
  }

  async #draft() {
    const pull = this.#pull;
    if (!pull) return;
    await this.#loadReview();
    const notes = unsent(this.#review || { notes: [] });
    const hasOverall = !!(this.#review?.overall || "").trim();
    if (!notes.length && !hasOverall) return;

    const dialog = this.querySelector("[data-draft-dialog]");
    const form = this.querySelector("[data-draft-form]");
    const summary = this.querySelector("[data-draft-summary]");
    if (!dialog || !form || !summary) return;

    const lineCount = notes.filter((n) => n.start != null || n.end != null).length;
    const bits = [];
    if (lineCount) bits.push(`${lineCount} line comment${lineCount === 1 ? "" : "s"}`);
    if (notes.length - lineCount > 0) {
      bits.push(`${notes.length - lineCount} file note${notes.length - lineCount === 1 ? "" : "s"}`);
    }
    if (hasOverall) bits.push("an overall note");
    summary.textContent = `This will post ${bits.join(" and ") || "your notes"} as a pending review.`;

    openDialog(dialog);
    form.onsubmit = async (ev) => {
      if (ev.submitter?.value === "cancel") return;
      ev.preventDefault();
      setBusy(form, true);
      try {
        const res = await api.post(`/pulls/${pull.id}/draft`, {});
        dialog.close();
        await this.#loadReview();
        await this.querySelector("dync-changes-view")?.reload();
        showToast("Draft review saved.", {
          kind: "ok",
          href: res?.url,
          hrefLabel: "Open on GitHub",
        });
        this.#paint();
      } catch (err) {
        dialog.close();
        showToast(err instanceof ApiError ? err.message : "Could not save the draft.", {
          kind: "error",
        });
      } finally {
        setBusy(form, false);
      }
    };
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-pull")) {
  customElements.define("dync-pull", DyncPull);
}
