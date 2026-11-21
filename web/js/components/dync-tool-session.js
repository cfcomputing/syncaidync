import { DyncElement, html, escapeText, raw } from "../lib/element.js";
import { api, ApiError } from "../lib/api.js";
import { store } from "../lib/store.js";
import { agentLabel, formatTokens, stateLabel, toolLabel } from "../lib/labels.js";
import { relativeTime, absoluteTime } from "../lib/time.js";
import { sessionLinks } from "../lib/session-links.js";
import { showToast } from "../lib/toast.js";
import { openCloseOut } from "./dync-close-out.js";
import "./dync-goal-panel.js";

const HISTORY_MAX = 8;
const TOOLS_MAX = 20;

export class DyncToolSession extends DyncElement {
  static get observedAttributes() {
    return ["session"];
  }

  #history = [];
  #links = [];
  #linkRequest = 0;
  #lastState = null;
  #sendText = "";
  #sending = false;
  #paintKey = "";
  #goalPanel = null;

  mount() {
    this.classList.add("tool", "tool-session");
    this.#paint(true);
    this.#loadLinks();
    const unsub = store.subscribe(() => this.#onStore());
    this.cleanup(unsub);
    this.on(this, "click", (ev) => this.#onClick(ev));
    this.on(this, "keydown", (ev) => this.#onKey(ev));
    this.on(this, "input", (ev) => {
      if (ev.target.matches?.("[data-el=send]")) this.#sendText = ev.target.value;
    });
  }

  attributeChangedCallback() {
    this.#history = [];
    this.#lastState = null;
    this.#sendText = "";
    this.#links = [];
    this.#paintKey = "";
    this.#paint(true);
    if (this.isConnected) this.#loadLinks();
  }

  async #loadLinks() {
    const request = ++this.#linkRequest;
    const id = this.#sessionId();
    if (!id) return;
    const links = await sessionLinks(id);
    if (request !== this.#linkRequest || !this.isConnected) return;
    this.#links = links;
    this.#paint(true);
  }

  #sessionId() {
    return this.getAttribute("session");
  }

  #session() {
    const id = this.#sessionId();
    return id ? store.sessions.get(id) : null;
  }

  #snapshotKey(s) {
    if (!s) return "gone";
    const tools = (s.tools || [])
      .map((t) => `${t.at}:${t.name}:${t.detail}`)
      .join("|");
    const usage = s.usage || {};
    return [
      s.id,
      s.state,
      s.blocked?.kind || "",
      s.snoozed ? 1 : 0,
      s.exit_code ?? "",
      s.live ? 1 : 0,
      s.model || "",
      s.autonomy ? 1 : 0,
      s.workspace?.path || "",
      s.workspace?.branch || "",
      s.workspace?.base || "",
      s.workspace?.base_commit || "",
      s.goal ? 1 : 0,
      usage.cost_usd ?? "",
      usage.context_pct ?? "",
      usage.tokens ?? "",
      tools,
      this.#links.map((l) => l.url).join("|"),
      this.#history.map((h) => `${h.at}:${h.label}`).join("|"),
      this.#sending ? 1 : 0,
    ].join("\n");
  }

  #onStore() {
    const s = this.#session();
    if (!s) {
      this.#paint(true);
      return;
    }
    const key = `${s.state}|${s.blocked?.kind || ""}|${s.snoozed ? 1 : 0}|${s.exit_code ?? ""}`;
    if (this.#lastState !== key) {
      this.#lastState = key;
      this.#history = [
        { at: Date.now(), label: stateLabel(s) },
        ...this.#history,
      ].slice(0, HISTORY_MAX);
    }
    this.#paint(false);
  }

  #paint(force) {
    const s = this.#session();
    const nextKey = this.#snapshotKey(s);
    if (!force && nextKey === this.#paintKey) return;

    const send = this.querySelector("[data-el=send]");
    const keepFocus = send && document.activeElement === send;
    const selStart = keepFocus ? send.selectionStart : null;
    const selEnd = keepFocus ? send.selectionEnd : null;
    if (keepFocus && !force) {
      // Prefer leaving the caret alone when only background data changed.
      // Still refresh if the paint key changed (live/usage/tools/etc.).
    }

    this.#paintKey = nextKey;
    const toolsScroll = this.querySelector("[data-el=tools]")?.scrollTop || 0;

    if (!s) {
      this.render(
        html`<div class="empty tool-placeholder">
          <h2>Session</h2>
          <p>This session is gone.</p>
        </div>`,
      );
      return;
    }

    const ws = s.workspace || {};
    const facts = [
      ["Agent", agentLabel(store.meta, s.agent)],
      ["Model", s.model || "Agent default"],
      ["Approval prompts", s.autonomy ? "Off" : "On"],
      ["Folder", ws.path || "—"],
      ["Branch", ws.branch || "—"],
      ["Base", ws.base || (ws.base_commit ? ws.base_commit.slice(0, 7) : "—")],
      ["Created", absoluteTime(s.created_at)],
      ["Session id", s.id],
    ];

    const factRows = facts
      .map(([k, v]) => {
        if (k === "Folder" && ws.path) {
          return `<div class="sess-fact"><dt>${escapeText(k)}</dt><dd>
            <code class="sess-path">${escapeText(ws.path)}</code>
            <button type="button" class="ghost sm" data-act="copy-path">Copy path</button>
          </dd></div>`;
        }
        return `<div class="sess-fact"><dt>${escapeText(k)}</dt><dd>${escapeText(v)}</dd></div>`;
      })
      .join("");

    const hist = this.#history
      .map(
        (h) =>
          `<li><time>${escapeText(relativeTime(h.at))}</time> ${escapeText(h.label)}</li>`,
      )
      .join("");

    const tools = [...(s.tools || [])]
      .slice()
      .sort((a, b) => (b.at || 0) - (a.at || 0))
      .slice(0, TOOLS_MAX)
      .map(
        (t) =>
          `<li><time>${escapeText(relativeTime(t.at))}</time> ${escapeText(toolLabel(t))}</li>`,
      )
      .join("");

    const usage = s.usage || {};
    const cost = typeof usage.cost_usd === "number" ? usage.cost_usd : null;
    const ctx = typeof usage.context_pct === "number" ? usage.context_pct : null;
    const tokens =
      typeof usage.tokens === "number" && Number.isFinite(usage.tokens) && usage.tokens > 0
        ? usage.tokens
        : null;
    const costPct = cost == null ? 0 : Math.min(100, Math.round((cost / 5) * 100));
    const ctxPct = ctx == null ? 0 : Math.min(100, Math.round(ctx));
    const ctxWarn = ctxPct > 80 ? " data-warn" : "";
    const tokenLabel = tokens == null ? "—" : formatTokens(tokens);

    const live = !!s.live;
    const sendDisabled = !live || this.#sending ? "disabled" : "";

    this.render(
      html`<div class="sess">
        <header class="sess-head">
          <h2>Session</h2>
          <button type="button" class="primary sm" data-act="close-out">Close out</button>
        </header>

        ${s.goal ? html`<section class="sess-section sess-goal">
          <h3>Goal</h3>
          <div data-goal-slot></div>
        </section>` : ""}

        <section class="sess-section">
          <h3>Facts</h3>
          <dl class="sess-facts">${raw(factRows)}</dl>
        </section>

        ${this.#links.length ? html`<section class="sess-section" data-links>
          <h3>Links</h3>
          <ul>${this.#links.map(({ label, url }) =>
            html`<li><a href="${url}" target="_blank" rel="noopener noreferrer">${label}</a></li>`
          )}</ul>
        </section>` : ""}

        <section class="sess-section">
          <h3>State history</h3>
          ${hist
            ? html`<ul class="sess-list">${raw(hist)}</ul>`
            : html`<p class="muted sm">No state changes yet on this page.</p>`}
        </section>

        <section class="sess-section">
          <h3>Activity</h3>
          ${tools
            ? html`<ul class="sess-list sess-list-scroll" data-el="tools">${raw(tools)}</ul>`
            : html`<p class="muted sm">No tools used yet.</p>`}
        </section>

        <section class="sess-section">
          <h3>Usage</h3>
          <div class="sess-meters">
            <div class="sess-meter">
              <div class="sess-meter-label">
                <span>Cost</span>
                <span>${cost == null ? "—" : `$${cost.toFixed(2)}`}</span>
              </div>
              <div class="sess-meter-bar" role="meter" aria-valuenow="${costPct}" aria-valuemin="0" aria-valuemax="100">
                <span style="width:${costPct}%"></span>
              </div>
            </div>
            <div class="sess-meter">
              <div class="sess-meter-label">
                <span>Tokens</span>
                <span>${tokenLabel}</span>
              </div>
            </div>
            <div class="sess-meter"${raw(ctxWarn)}>
              <div class="sess-meter-label">
                <span>Context</span>
                <span>${ctx == null ? "—" : `${ctxPct}%`}</span>
              </div>
              <div class="sess-meter-bar" role="meter" aria-valuenow="${ctxPct}" aria-valuemin="0" aria-valuemax="100">
                <span style="width:${ctxPct}%"></span>
              </div>
            </div>
          </div>
        </section>

        <section class="sess-section sess-send">
          <h3>Send a message</h3>
          <textarea data-el="send" rows="3" placeholder="Type into the agent’s terminal"
            ${live ? "" : "disabled"}>${this.#sendText}</textarea>
          <div class="sess-send-actions">
            <button type="button" class="primary sm" data-act="send" ${sendDisabled}>Send</button>
            <span class="muted sm">Cmd/Ctrl+Enter</span>
          </div>
        </section>
      </div>`,
    );

    // The goal panel keeps its own state (open outputs, Keep going form) across repaints.
    const slot = this.querySelector("[data-goal-slot]");
    if (slot) {
      if (!this.#goalPanel) this.#goalPanel = document.createElement("dync-goal-panel");
      if (this.#goalPanel.getAttribute("session") !== s.id) {
        this.#goalPanel.setAttribute("session", s.id);
      }
      slot.replaceWith(this.#goalPanel);
    }

    const toolsList = this.querySelector("[data-el=tools]");
    if (toolsList && toolsScroll) toolsList.scrollTop = toolsScroll;

    if (keepFocus) {
      const again = this.querySelector("[data-el=send]");
      if (again) {
        again.focus();
        if (selStart != null && selEnd != null) {
          try {
            again.setSelectionRange(selStart, selEnd);
          } catch {
            /* ignore */
          }
        }
      }
    }
  }

  #onClick(ev) {
    const act = ev.target.closest?.("[data-act]")?.getAttribute("data-act");
    if (!act) return;
    if (act === "copy-path") this.#copyPath();
    else if (act === "close-out") {
      const id = this.#sessionId();
      if (id) openCloseOut(id);
    } else if (act === "send") this.#send();
  }

  #onKey(ev) {
    if (ev.target.matches?.("[data-el=send]") && (ev.metaKey || ev.ctrlKey) && ev.key === "Enter") {
      ev.preventDefault();
      this.#send();
    }
  }

  async #copyPath() {
    const path = this.#session()?.workspace?.path;
    if (!path) return;
    try {
      await navigator.clipboard.writeText(path);
      showToast("Path copied.", { kind: "ok" });
    } catch {
      showToast("Could not copy the path.", { kind: "error" });
    }
  }

  async #send() {
    const id = this.#sessionId();
    const text = String(this.#sendText || "").trim();
    if (!id || !text || this.#sending) return;
    this.#sending = true;
    this.#paint(true);
    try {
      await api.post(`/sessions/${encodeURIComponent(id)}/send`, { text, submit: true });
      this.#sendText = "";
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : "Could not send.", { kind: "error" });
    } finally {
      this.#sending = false;
      this.#paint(true);
    }
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-tool-session")) {
  customElements.define("dync-tool-session", DyncToolSession);
}
