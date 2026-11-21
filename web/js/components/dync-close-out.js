import { DyncElement, html, escapeText, raw } from "../lib/element.js";
import { api, ApiError } from "../lib/api.js";
import { store } from "../lib/store.js";
import { openDialog } from "../lib/dialog.js";
import { openRemoveSession } from "./dync-remove-session.js";
import { showToast } from "../lib/toast.js";

const SCOPES = ["project", "everywhere", "note"];

export class DyncCloseOut extends DyncElement {
  #dialog = null;
  #sessionId = null;
  #openGen = 0;
  #step = 1;
  #dest = { everywhere: null, project: null, note: null };
  #takeaways = [];
  #suggesting = false;
  #suggestSeconds = 0;
  #suggestTimer = null;
  #suggestError = null;
  #suggestAgent = "claude";
  #suggestModel = "";
  #savedNote = null;
  #writtenNote = null;
  #busy = false;

  mount() {
    this.render(
      html`<dialog class="close-out-dialog">
        <div class="dialog-body" data-body></div>
      </dialog>`,
    );
    this.#dialog = this.querySelector("dialog");
    this.on(this, "click", (ev) => this.#onClick(ev));
    this.on(this, "change", (ev) => this.#onChange(ev));
    this.on(this, "input", (ev) => this.#onInput(ev));
    this.on(this.#dialog, "close", () => {
      this.#openGen += 1;
      clearInterval(this.#suggestTimer);
      this.#suggestTimer = null;
      this.#suggesting = false;
    });
  }

  disconnectedCallback() {
    this.#openGen += 1;
    clearInterval(this.#suggestTimer);
    super.disconnectedCallback();
  }

  async open(sessionId) {
    const gen = ++this.#openGen;
    this.#sessionId = sessionId;
    this.#step = 1;
    this.#takeaways = [];
    this.#suggestError = null;
    this.#savedNote = null;
    this.#writtenNote = null;
    this.#busy = false;
    clearInterval(this.#suggestTimer);
    this.#suggesting = false;

    const session = store.sessions.get(sessionId);
    if (session?.agent && session.agent !== "shell") {
      this.#suggestAgent = session.agent;
    }
    if (session?.model) this.#suggestModel = session.model;

    try {
      this.#dest = await api.get(`/sessions/${encodeURIComponent(sessionId)}/takeaways/destinations`);
    } catch {
      this.#dest = { everywhere: null, project: null, note: null };
    }
    if (gen !== this.#openGen) return;

    this.#paint();
    openDialog(this.#dialog);
  }

  #agents() {
    return (store.meta?.agents || []).filter((a) => a.installed && a.id !== "shell");
  }

  #paint() {
    const body = this.querySelector("[data-body]");
    if (!body) return;
    if (this.#step === 1) body.innerHTML = this.#step1().value;
    else if (this.#step === 2) body.innerHTML = this.#step2().value;
    else body.innerHTML = this.#step3().value;
  }

  #step1() {
    const agents = this.#agents();
    const options = agents
      .map(
        (a) =>
          `<option value="${escapeText(a.id)}" ${a.id === this.#suggestAgent ? "selected" : ""}>${escapeText(a.label || a.id)}</option>`,
      )
      .join("");
    const suggesting = this.#suggesting
      ? html`<p class="co-progress">Suggesting… ${this.#suggestSeconds}s — This can take a minute</p>`
      : "";
    const err = this.#suggestError
      ? html`<p class="error">${this.#suggestError}
          <button type="button" class="ghost sm" data-act="suggest">Try again</button></p>`
      : "";
    return html`<h2>Any takeaways?</h2>
      <p>Keep lessons from this session for the next one.</p>
      ${suggesting}
      ${err}
      <div class="co-suggest-row">
        <select data-act="agent" ${this.#suggesting || !agents.length ? "disabled" : ""}>${raw(options)}</select>
        <input type="text" data-act="model" placeholder="model" value="${this.#suggestModel}"
          ${this.#suggesting ? "disabled" : ""} />
        <button type="button" class="primary" data-act="suggest"
          ${this.#suggesting || !agents.length ? "disabled" : ""}>Suggest takeaways</button>
      </div>
      <div class="dialog-actions">
        <button type="button" class="ghost" data-act="add">Add one myself</button>
        <button type="button" class="ghost" data-act="skip">Skip</button>
      </div>`;
  }

  #step2() {
    const rows = this.#takeaways
      .map((t, i) => {
        const scopeOpts = SCOPES.map((s) => {
          const path = this.#dest[s];
          const disabled = s === "project" && !path ? "disabled" : "";
          const label = path || (s === "project" ? "project (no repo)" : s);
          return `<option value="${s}" ${t.scope === s ? "selected" : ""} ${disabled}>${escapeText(s)} → ${escapeText(label)}</option>`;
        }).join("");
        return `<div class="co-item" data-i="${i}">
          <input type="text" data-field="title" maxlength="80" value="${escapeText(t.title)}" placeholder="Title" />
          <textarea data-field="body" maxlength="600" rows="3" placeholder="One paragraph">${escapeText(t.body)}</textarea>
          <div class="co-item-meta">
            <select data-field="scope">${scopeOpts}</select>
            <button type="button" class="ghost sm danger-text" data-act="remove" data-i="${i}">Remove</button>
          </div>
        </div>`;
      })
      .join("");
    const saved = this.#savedNote
      ? html`<p class="co-saved">${this.#savedNote}</p>`
      : "";
    return html`<h2>Takeaways</h2>
      <p>Edit, drop, or change the file each one will be written to.</p>
      <div class="co-list">${raw(rows || `<p class="muted">No takeaways yet.</p>`)}</div>
      ${saved}
      <div class="dialog-actions">
        <button type="button" class="ghost" data-act="add">Add one</button>
        <button type="button" class="ghost" data-act="skip-save">Skip save</button>
        <button type="button" class="primary" data-act="save"
          ${this.#busy || !this.#takeaways.length ? "disabled" : ""}>Save takeaways</button>
      </div>`;
  }

  #step3() {
    const session = store.sessions.get(this.#sessionId);
    const live = !!session?.live;
    const written = this.#writtenNote
      ? html`<p class="co-saved">${this.#writtenNote}</p>`
      : "";
    return html`<h2>What next?</h2>
      <p>Takeaways are set. Choose what to do with the session.</p>
      ${written}
      <div class="dialog-actions co-next">
        <button type="button" class="ghost" data-act="leave">Leave it running</button>
        <button type="button" class="ghost" data-act="stop" ${live ? "" : "disabled"}>Stop the session</button>
        <button type="button" class="danger" data-act="remove">Remove the session</button>
      </div>`;
  }

  #onClick(ev) {
    const btn = ev.target.closest("[data-act]");
    if (!btn || btn.disabled) return;
    const act = btn.getAttribute("data-act");
    if (act === "suggest") this.#suggest();
    else if (act === "add") this.#addBlank();
    else if (act === "skip") {
      this.#step = 3;
      this.#paint();
    } else if (act === "skip-save") {
      this.#step = 3;
      this.#paint();
    } else if (act === "save") this.#save();
    else if (act === "remove" && btn.hasAttribute("data-i")) {
      const i = Number(btn.getAttribute("data-i"));
      this.#takeaways.splice(i, 1);
      this.#paint();
    } else if (act === "leave") {
      this.#dialog.close();
    } else if (act === "stop") this.#stop();
    else if (act === "remove") this.#remove();
  }

  #onChange(ev) {
    if (ev.target.matches?.("select[data-act=agent]")) {
      this.#suggestAgent = ev.target.value;
    }
    if (ev.target.matches?.("[data-field=scope]")) {
      const item = ev.target.closest(".co-item");
      const i = Number(item?.getAttribute("data-i"));
      if (this.#takeaways[i]) this.#takeaways[i].scope = ev.target.value;
    }
  }

  #onInput(ev) {
    if (ev.target.matches?.("input[data-act=model]")) {
      this.#suggestModel = ev.target.value;
      return;
    }
    const item = ev.target.closest?.(".co-item");
    if (!item) return;
    const i = Number(item.getAttribute("data-i"));
    const t = this.#takeaways[i];
    if (!t) return;
    const field = ev.target.getAttribute("data-field");
    if (field === "title") t.title = ev.target.value;
    if (field === "body") t.body = ev.target.value;
  }

  #addBlank() {
    const scope = this.#dest.project ? "project" : this.#dest.everywhere ? "everywhere" : "note";
    this.#takeaways.push({ title: "", body: "", scope });
    this.#step = 2;
    this.#paint();
  }

  async #suggest() {
    if (this.#suggesting || !this.#sessionId) return;
    const gen = this.#openGen;
    const sessionId = this.#sessionId;
    this.#suggesting = true;
    this.#suggestError = null;
    this.#suggestSeconds = 0;
    clearInterval(this.#suggestTimer);
    this.#suggestTimer = setInterval(() => {
      if (gen !== this.#openGen) return;
      this.#suggestSeconds += 1;
      this.#paint();
    }, 1000);
    this.#paint();
    try {
      const res = await api.post(
        `/sessions/${encodeURIComponent(sessionId)}/takeaways/suggest`,
        { agent: this.#suggestAgent, model: this.#suggestModel || null },
      );
      if (gen !== this.#openGen || this.#sessionId !== sessionId) return;
      this.#takeaways = (res.takeaways || []).map((t) => {
        let scope = SCOPES.includes(t.scope) ? t.scope : "project";
        if (scope === "project" && !this.#dest.project) {
          scope = this.#dest.everywhere ? "everywhere" : "note";
        }
        return { title: t.title || "", body: t.body || "", scope };
      });
      if (!this.#takeaways.length) {
        this.#suggestError = "No takeaways came back. You can add one yourself.";
      } else {
        this.#step = 2;
      }
    } catch (err) {
      if (gen !== this.#openGen || this.#sessionId !== sessionId) return;
      this.#suggestError = err instanceof ApiError ? err.message : "Suggesting failed.";
    } finally {
      if (gen === this.#openGen) {
        clearInterval(this.#suggestTimer);
        this.#suggestTimer = null;
        this.#suggesting = false;
        this.#paint();
      }
    }
  }

  async #save() {
    if (this.#busy || !this.#sessionId) return;
    const gen = this.#openGen;
    const cleaned = this.#takeaways
      .map((t) => ({
        title: String(t.title || "").trim(),
        body: String(t.body || "").trim(),
        scope: t.scope || "project",
      }))
      .filter((t) => t.title && t.body);
    if (!cleaned.length) {
      this.#savedNote = "Each takeaway needs a title and a body.";
      this.#paint();
      return;
    }
    this.#busy = true;
    this.#paint();
    try {
      const res = await api.post(`/sessions/${encodeURIComponent(this.#sessionId)}/takeaways/save`, {
        takeaways: cleaned,
      });
      if (gen !== this.#openGen) return;
      const parts = (res.written || []).map((w) => `${w.count} → ${w.path}`);
      this.#savedNote = parts.length ? `Saved: ${parts.join("; ")}` : "Saved.";
      this.#writtenNote = this.#savedNote;
      showToast(this.#savedNote, { kind: "ok" });
      this.#step = 3;
    } catch (err) {
      if (gen !== this.#openGen) return;
      this.#savedNote = err instanceof ApiError ? err.message : "Could not save takeaways.";
    } finally {
      if (gen === this.#openGen) {
        this.#busy = false;
        this.#paint();
      }
    }
  }

  async #stop() {
    if (!this.#sessionId) return;
    try {
      await api.post(`/sessions/${encodeURIComponent(this.#sessionId)}/stop`);
      this.#dialog.close();
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : "Could not stop the session.", {
        kind: "error",
      });
    }
  }

  #remove() {
    const id = this.#sessionId;
    this.#dialog.close();
    if (id) openRemoveSession(id);
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-close-out")) {
  customElements.define("dync-close-out", DyncCloseOut);
}

export function openCloseOut(sessionId) {
  let el = document.querySelector("dync-close-out");
  if (!el) {
    el = document.createElement("dync-close-out");
    document.body.appendChild(el);
  }
  el.open(sessionId);
}
