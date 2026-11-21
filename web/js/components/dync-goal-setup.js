import { DyncElement, html } from "../lib/element.js";
import { api, ApiError } from "../lib/api.js";
import { store } from "../lib/store.js";
import * as prefs from "../lib/prefs.js";
import { nextId, parseLimit, uncovered, validateGoal } from "../lib/goal.js";

const AGENTS = [
  { id: "claude", label: "Claude Code" },
  { id: "codex", label: "Codex" },
  { id: "pi", label: "Pi" },
  { id: "cursor", label: "Cursor Agent" },
];

const DEFAULTS = {
  rounds: 3,
  cost_usd: "",
  minutes: "",
  baseline_on: true,
  reviewer_on: false,
  reviewer_agent: "claude",
  reviewer_model: "fable",
};

/**
 * The "Set a goal" part of the New session dialog: goal, done criteria, checks (drafted by a
 * model or written by hand), questions from the draft, limits and the reviewer.
 *
 * The dialog sets `context` to a function returning `{ agent, model, workspace }` so a draft
 * can read the chosen repository.
 */
export class DyncGoalSetup extends DyncElement {
  context = null;
  #on = false;
  #text = "";
  #criteria = [{ id: "k1", text: "" }];
  #checks = [];
  #questions = [];
  #limits = { rounds: 3, cost_usd: "", minutes: "" };
  #baseline = true;
  #reviewer = { on: false, agent: "claude", model: "fable" };
  #draftAgent = "";
  #draftModel = "";
  #drafting = false;
  #draftStarted = 0;
  #draftError = "";
  #draftNote = "";
  #missingGuards = new Set();
  #workspaceNote = "";

  mount() {
    this.classList.add("goal-setup");
    this.reset();
    this.on(this, "input", (ev) => this.#onInput(ev));
    this.on(this, "change", (ev) => this.#onInput(ev));
    this.on(this, "click", (ev) => this.#onClick(ev));
    this.on(this, "keydown", (ev) => this.#onKey(ev));
    this.every(1000, () => {
      if (this.#drafting) this.#paintProgress();
    });
  }

  /** Start fresh, keeping remembered limits and reviewer choices. */
  reset() {
    const d = { ...DEFAULTS, ...prefs.get("goal-defaults", {}) };
    this.#on = false;
    this.#text = "";
    this.#criteria = [{ id: "k1", text: "" }];
    this.#checks = [];
    this.#questions = [];
    this.#limits = { rounds: d.rounds, cost_usd: d.cost_usd, minutes: d.minutes };
    this.#baseline = !!d.baseline_on;
    this.#reviewer = { on: !!d.reviewer_on, agent: d.reviewer_agent, model: d.reviewer_model };
    this.#draftError = "";
    this.#draftNote = "";
    this.#drafting = false;
    this.#missingGuards = new Set();
    this.#workspaceNote = "";
    this.#paint();
  }

  /** A line under the toggle about where the session will run (set by the dialog). */
  set workspaceNote(text) {
    this.#workspaceNote = text || "";
    this.#paint();
  }

  get enabled() {
    return this.#on;
  }

  /** An error sentence, or "" when the goal can be sent. */
  validate() {
    if (!this.#on) return "";
    const err = validateGoal(this.#goalShape());
    if (err) return err;
    const limits = this.#goalShape().limits;
    if (Number.isNaN(limits.cost_usd)) return "The cost limit must be a number of dollars.";
    if (Number.isNaN(limits.minutes)) return "The time limit must be a whole number of minutes.";
    return "";
  }

  /** The goal to send with POST /sessions, or null when off. */
  payload() {
    if (!this.#on) return null;
    const g = this.#goalShape();
    prefs.set("goal-defaults", {
      rounds: g.limits.rounds,
      cost_usd: this.#limits.cost_usd,
      minutes: this.#limits.minutes,
      baseline_on: this.#baseline,
      reviewer_on: this.#reviewer.on,
      reviewer_agent: this.#reviewer.agent,
      reviewer_model: this.#reviewer.model,
    });
    return g;
  }

  get goalText() {
    return this.#text.trim();
  }

  #goalShape() {
    const criteria = this.#criteria
      .filter((k) => k.text.trim())
      .map((k) => ({ id: k.id, text: k.text.trim() }));
    const keep = new Set(criteria.map((k) => k.id));
    return {
      text: this.#text.trim(),
      criteria,
      checks: this.#checks
        .filter((c) => keep.has(c.criterion))
        .map((c) => ({
          id: c.id,
          criterion: c.criterion,
          title: c.title.trim(),
          kind: c.kind,
          stage: c.stage,
          command: c.kind === "command" ? c.command.trim() : null,
          guards: c.guards,
          timeout_s: Number.parseInt(c.timeout_s, 10),
          why: c.why || "",
        })),
      limits: {
        rounds: Number.parseInt(this.#limits.rounds, 10),
        cost_usd: parseLimit(this.#limits.cost_usd),
        minutes: parseLimit(this.#limits.minutes, { integer: true }),
      },
      baseline_on: this.#baseline,
      reviewer: {
        on: this.#reviewer.on,
        agent: this.#reviewer.agent,
        model: this.#reviewer.model.trim() || null,
      },
    };
  }

  // -------------------------------------------------------------------------------------------

  #installed() {
    const meta = store.meta?.agents || [];
    return AGENTS.filter((a) => meta.find((m) => m.id === a.id)?.installed);
  }

  #paint() {
    const installed = this.#installed();
    const ctx = this.context?.() || {};
    if (!this.#draftAgent || !installed.find((a) => a.id === this.#draftAgent)) {
      this.#draftAgent =
        installed.find((a) => a.id === ctx.agent)?.id || installed[0]?.id || "claude";
    }
    const missing = new Set(uncovered(this.#criteria, this.#checks).map((k) => k.id));
    const agentOptions = (selected) =>
      (installed.length ? installed : AGENTS).map(
        (a) => html`<option value="${a.id}" ${a.id === selected ? "selected" : ""}>${a.label}</option>`,
      );

    this.render(html`
      <label class="check goal-toggle">
        <input type="checkbox" data-f="on" ${this.#on ? "checked" : ""} />
        <span><strong>Set a goal</strong> — SyncAiDync runs your checks after every turn and sends failures back.</span>
      </label>
      <div class="goal-body" ${this.#on ? "" : "hidden"}>
        ${this.#workspaceNote ? html`<p class="goal-workspace-note">${this.#workspaceNote}</p>` : ""}
        <label>
          <span>Goal</span>
          <input data-f="text" maxlength="500" placeholder="The end state, in one sentence" value="${this.#text}" />
        </label>

        <div class="goal-criteria">
          <div class="goal-subhead">
            <span>Done when</span>
            <button type="button" class="ghost sm" data-act="add-criterion">Add</button>
          </div>
          ${this.#criteria.map(
            (k) => html`<div class="goal-criterion ${missing.has(k.id) ? "is-missing" : ""}" data-crit-row="${k.id}">
              <div class="goal-criterion-row">
                <input data-crit="${k.id}" data-f="text" maxlength="200" placeholder="One thing that must be true" value="${k.text}" />
                ${this.#criteria.length > 1
                  ? html`<button type="button" class="icon sm" data-act="remove-criterion" data-id="${k.id}" title="Remove" aria-label="Remove">×</button>`
                  : ""}
              </div>
              <div class="goal-checks">
                ${this.#checks.filter((c) => c.criterion === k.id).map((c) => this.#checkMarkup(c))}
                ${missing.has(k.id) ? html`<p class="goal-missing sm">No check yet.</p>` : ""}
                <div class="goal-add-check">
                  <button type="button" class="ghost sm" data-act="add-check" data-id="${k.id}" data-kind="command">+ Command check</button>
                  <button type="button" class="ghost sm" data-act="add-check" data-id="${k.id}" data-kind="manual">+ Manual check</button>
                </div>
              </div>
            </div>`,
          )}
        </div>

        <div class="goal-draft">
          <div class="goal-draft-row">
            <button type="button" class="sm" data-act="draft" ${this.#drafting ? "disabled" : ""}>
              ${this.#questions.length || this.#checks.length ? "Draft again" : "Draft checks"}
            </button>
            <select data-f="draft-agent" aria-label="Agent that drafts">${agentOptions(this.#draftAgent)}</select>
            <input data-f="draft-model" class="goal-model" placeholder="Model (default)" value="${this.#draftModel}" />
          </div>
          <p class="muted sm" data-el="draft-progress" ${this.#drafting ? "" : "hidden"}></p>
          ${this.#draftError ? html`<p class="error sm">${this.#draftError}</p>` : ""}
          ${this.#draftNote ? html`<p class="muted sm">${this.#draftNote}</p>` : ""}
        </div>

        ${this.#questions.length
          ? html`<div class="goal-questions">
              <div class="goal-subhead"><span>The draft asks</span></div>
              ${this.#questions.map(
                (q, i) => html`<label class="goal-question">
                  <span>${q.question}</span>
                  <input data-q="${i}" data-f="answer" placeholder="Your answer (then Draft again)" value="${q.answer}" />
                </label>`,
              )}
            </div>`
          : ""}

        <div class="goal-limits">
          <div class="goal-subhead"><span>Limits</span></div>
          <div class="goal-limit-row">
            <label><span>Rounds</span><input data-f="rounds" type="number" min="1" max="10" value="${this.#limits.rounds}" /></label>
            <label><span>Cost ($)</span><input data-f="cost" inputmode="decimal" placeholder="No limit" value="${this.#limits.cost_usd}" /></label>
            <label><span>Working time (min)</span><input data-f="minutes" inputmode="numeric" placeholder="No limit" value="${this.#limits.minutes}" /></label>
          </div>
        </div>

        <label class="check">
          <input type="checkbox" data-f="baseline" ${this.#baseline ? "checked" : ""} />
          <span>Run the checks once before the agent starts.</span>
        </label>

        <div class="goal-reviewer">
          <label class="check">
            <input type="checkbox" data-f="reviewer" ${this.#reviewer.on ? "checked" : ""} />
            <span>Have a reviewer read the change and the evidence before the goal counts as met.</span>
          </label>
          <div class="goal-draft-row" ${this.#reviewer.on ? "" : "hidden"}>
            <select data-f="reviewer-agent" aria-label="Reviewer agent">${agentOptions(this.#reviewer.agent)}</select>
            <input data-f="reviewer-model" class="goal-model" placeholder="Model (default)" value="${this.#reviewer.model}" />
          </div>
        </div>

        <p class="goal-warning sm">These commands will run in the workspace after every turn.</p>
      </div>
    `);
    this.#paintProgress();
  }

  #checkMarkup(c) {
    const manual = c.kind === "manual";
    return html`<div class="goal-check" data-check-row="${c.id}">
      <div class="goal-check-row">
        <span class="goal-kind">${manual ? "Manual" : c.stage === "full" ? "Full" : "Quick"}</span>
        <input data-check="${c.id}" data-f="title" maxlength="80" placeholder="${manual ? "What you will check" : "Title"}" value="${c.title}" />
        ${manual
          ? ""
          : html`<select data-check="${c.id}" data-f="stage" aria-label="Stage">
              <option value="quick" ${c.stage !== "full" ? "selected" : ""}>Quick</option>
              <option value="full" ${c.stage === "full" ? "selected" : ""}>Full</option>
            </select>`}
        <button type="button" class="icon sm" data-act="remove-check" data-id="${c.id}" title="Remove check" aria-label="Remove check">×</button>
      </div>
      ${manual
        ? ""
        : html`<textarea class="goal-command" data-check="${c.id}" data-f="command" spellcheck="false" maxlength="2000" rows="${Math.min(12, Math.max(1, c.command.split("\n").length))}" aria-label="Command" placeholder="Command, run from the workspace root">${c.command}</textarea>`}
      ${c.why ? html`<p class="muted sm goal-why">${c.why}</p>` : ""}
      ${c.guards
        .filter((g) => this.#missingGuards.has(g))
        .map(
          (g) => html`<p class="goal-missing-guard">${g} does not exist yet, so the agent would write the file that judges this check. Remove it from “Relies on”, or write it yourself before you start.</p>`,
        )}
      <div class="goal-guards">
        <span class="muted sm">Relies on</span>
        ${c.guards.map(
          (g) => html`<span class="goal-chip ${this.#missingGuards.has(g) ? "is-missing" : ""}" title="${this.#missingGuards.has(g) ? "Does not exist yet" : ""}">${g}<button type="button" data-act="remove-guard" data-id="${c.id}" data-guard="${g}" aria-label="Remove ${g}">×</button></span>`,
        )}
        <input class="goal-guard-input" data-check="${c.id}" data-f="guard" placeholder="Add a path, Enter" spellcheck="false" />
        ${manual
          ? ""
          : html`<label class="goal-timeout"><span class="muted sm">Timeout</span><input data-check="${c.id}" data-f="timeout" type="number" min="5" max="1800" value="${c.timeout_s}" /><span class="muted sm">s</span></label>`}
      </div>
    </div>`;
  }

  #paintProgress() {
    const el = this.querySelector("[data-el=draft-progress]");
    if (!el) return;
    el.hidden = !this.#drafting;
    if (this.#drafting) {
      const secs = Math.round((Date.now() - this.#draftStarted) / 1000);
      el.textContent = `Drafting checks… ${secs}s. This can take a minute.`;
    }
  }

  #onInput(ev) {
    const t = ev.target;
    const f = t.dataset?.f;
    if (!f) return;
    if (f === "on") {
      this.#on = t.checked;
      this.#paint();
      this.dispatchEvent(new CustomEvent("dync-goal-toggle", { bubbles: true }));
      return;
    }
    if (t.dataset.crit) {
      const k = this.#criteria.find((x) => x.id === t.dataset.crit);
      if (k) k.text = t.value;
      return;
    }
    if (t.dataset.check) {
      const c = this.#checks.find((x) => x.id === t.dataset.check);
      if (!c) return;
      if (f === "title") c.title = t.value;
      else if (f === "command") c.command = t.value;
      else if (f === "timeout") c.timeout_s = t.value;
      else if (f === "stage" && ev.type === "change") {
        c.stage = t.value;
        this.#paint();
      }
      return;
    }
    if (t.dataset.q != null && f === "answer") {
      const q = this.#questions[Number(t.dataset.q)];
      if (q) q.answer = t.value;
      return;
    }
    switch (f) {
      case "text":
        this.#text = t.value;
        break;
      case "draft-agent":
        this.#draftAgent = t.value;
        break;
      case "draft-model":
        this.#draftModel = t.value;
        break;
      case "rounds":
        this.#limits.rounds = t.value;
        break;
      case "cost":
        this.#limits.cost_usd = t.value;
        break;
      case "minutes":
        this.#limits.minutes = t.value;
        break;
      case "baseline":
        this.#baseline = t.checked;
        break;
      case "reviewer":
        if (ev.type === "change") {
          this.#reviewer.on = t.checked;
          this.#paint();
        }
        break;
      case "reviewer-agent":
        this.#reviewer.agent = t.value;
        break;
      case "reviewer-model":
        this.#reviewer.model = t.value;
        break;
      default:
        break;
    }
  }

  #onKey(ev) {
    const t = ev.target;
    if (t.dataset?.f === "guard" && ev.key === "Enter") {
      ev.preventDefault();
      const c = this.#checks.find((x) => x.id === t.dataset.check);
      const path = t.value.trim().replace(/^\.\//, "").replace(/\/+$/, "");
      if (!c || !path) return;
      if (path.startsWith("/") || path.split("/").some((s) => s === ".." || s === ".git")) {
        this.#draftError = `"${path}" is not a path inside the workspace.`;
        this.#paint();
        return;
      }
      if (!c.guards.includes(path) && c.guards.length < 8) c.guards.push(path);
      this.#draftError = "";
      this.#paint();
      this.querySelector(`[data-check="${CSS.escape(c.id)}"][data-f="guard"]`)?.focus();
    } else if (t.dataset?.crit && ev.key === "Enter") {
      ev.preventDefault();
      this.#addCriterion();
    }
  }

  #onClick(ev) {
    const btn = ev.target.closest("[data-act]");
    if (!btn) return;
    const act = btn.dataset.act;
    const id = btn.dataset.id;
    if (act === "add-criterion") this.#addCriterion();
    else if (act === "remove-criterion") {
      this.#criteria = this.#criteria.filter((k) => k.id !== id);
      this.#checks = this.#checks.filter((c) => c.criterion !== id);
      this.#paint();
    } else if (act === "add-check") {
      const kind = btn.dataset.kind === "manual" ? "manual" : "command";
      const c = {
        id: nextId("c", this.#checks),
        criterion: id,
        title: "",
        kind,
        stage: "quick",
        command: "",
        guards: [],
        timeout_s: 300,
        why: "",
      };
      this.#checks.push(c);
      this.#paint();
      this.querySelector(`[data-check="${CSS.escape(c.id)}"][data-f="title"]`)?.focus();
    } else if (act === "remove-check") {
      this.#checks = this.#checks.filter((c) => c.id !== id);
      this.#paint();
    } else if (act === "remove-guard") {
      const c = this.#checks.find((x) => x.id === id);
      if (c) c.guards = c.guards.filter((g) => g !== btn.dataset.guard);
      this.#paint();
    } else if (act === "draft") {
      this.#draft();
    }
  }

  #addCriterion() {
    if (this.#criteria.length >= 8) return;
    const k = { id: nextId("k", this.#criteria), text: "" };
    this.#criteria.push(k);
    this.#paint();
    this.querySelector(`[data-crit="${CSS.escape(k.id)}"]`)?.focus();
  }

  async #draft() {
    const ctx = this.context?.() || {};
    const criteria = this.#criteria
      .filter((k) => k.text.trim())
      .map((k) => ({ id: k.id, text: k.text.trim() }));
    if (!this.#text.trim()) {
      this.#draftError = "Write the goal first.";
      this.#paint();
      return;
    }
    if (!criteria.length) {
      this.#draftError = "Add at least one done criterion.";
      this.#paint();
      return;
    }
    if (!ctx.workspace) {
      this.#draftError = ctx.error || "Pick where the session will run first.";
      this.#paint();
      return;
    }
    this.#drafting = true;
    this.#draftStarted = Date.now();
    this.#draftError = "";
    this.#draftNote = "";
    this.#paint();
    try {
      const data = await api.post("/goals/draft", {
        agent: this.#draftAgent,
        model: this.#draftModel.trim() || null,
        text: this.#text.trim(),
        criteria,
        answers: this.#questions
          .filter((q) => q.answer.trim())
          .map((q) => ({ question: q.question, answer: q.answer.trim() })),
        workspace: ctx.workspace,
      });
      // Drafted checks replace the drafted ones; checks written by hand stay.
      const kept = this.#checks.filter((c) => !c.drafted);
      const drafted = (data.checks || []).map((c) => ({
        id: "",
        criterion: c.criterion,
        title: c.title || "",
        kind: c.kind === "manual" ? "manual" : "command",
        stage: c.stage === "full" ? "full" : "quick",
        command: c.command || "",
        guards: Array.isArray(c.guards) ? c.guards.slice(0, 8) : [],
        timeout_s: c.timeout_s || 300,
        why: c.why || "",
        drafted: true,
      }));
      const all = [...kept];
      for (const c of drafted) {
        c.id = nextId("c", all);
        all.push(c);
      }
      this.#checks = all.slice(0, 12);
      this.#questions = (data.questions || []).map((q) => ({ question: q, answer: "" }));
      this.#missingGuards = new Set(Array.isArray(data.missing_guards) ? data.missing_guards : []);
      this.#draftNote = this.#questions.length
        ? "Review the commands. Answer the questions and draft again to refine them."
        : "Review the commands before you start. You can edit or remove any of them.";
    } catch (err) {
      this.#draftError = err instanceof ApiError ? err.message : "Could not draft checks.";
    } finally {
      this.#drafting = false;
      this.#paint();
    }
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-goal-setup")) {
  customElements.define("dync-goal-setup", DyncGoalSetup);
}
