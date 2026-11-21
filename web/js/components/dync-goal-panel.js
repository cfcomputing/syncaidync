import { DyncElement, html } from "../lib/element.js";
import { api, ApiError } from "../lib/api.js";
import { store } from "../lib/store.js";
import { relativeTime } from "../lib/time.js";
import { showToast } from "../lib/toast.js";
import * as route from "../lib/route.js";
import { openCloseOut } from "./dync-close-out.js";
import {
  flaggedChecks,
  moreRounds,
  reportFilesLabel,
  limitMeters,
  minutesLabel,
  pillText,
  pillTone,
  resultLabel,
  resultTone,
  roundSummaries,
  stages,
  validateLimits,
  parseLimit,
} from "../lib/goal.js";

/**
 * A session's goal: the pipeline it moves through, each check with its evidence, the limits,
 * and what you can do about it. `compact` shows only the pill and the pipeline (cards).
 */
export class DyncGoalPanel extends DyncElement {
  static get observedAttributes() {
    return ["session", "compact"];
  }

  #record = null;
  #recordKey = "";
  #loading = 0;
  #open = new Set();
  #roundOpen = null;
  #keepGoing = false;
  #busy = "";
  #paintKey = "";
  #limitDraft = null;
  #commitOpen = false;
  #commitPreview = null;
  #commitDraft = null;
  #moreOpen = false;
  #mergeOpen = false;
  #mergePreview = null;
  #moreDraft = { text: "", rounds: "" };

  mount() {
    this.classList.add("goal-panel");
    this.cleanup(store.subscribe(() => this.#paint()));
    this.on(this, "click", (ev) => this.#onClick(ev));
    this.on(this, "change", (ev) => this.#onChange(ev));
    this.on(this, "input", (ev) => this.#onLimitInput(ev));
    this.every(1000, () => {
      const g = this.#session()?.goal;
      if (g && (g.status === "working" || g.status === "checking" || g.report?.state === "writing")) {
        this.#paint();
      }
    });
    this.#paint(true);
  }

  attributeChangedCallback() {
    this.#record = null;
    this.#recordKey = "";
    this.#open.clear();
    this.#roundOpen = null;
    this.#keepGoing = false;
    this.#closeForms();
    if (this.isConnected) this.#paint(true);
  }

  get compact() {
    return this.hasAttribute("compact");
  }

  #session() {
    const id = this.getAttribute("session");
    return id ? store.sessions.get(id) : null;
  }

  async #loadRecord(s) {
    const key = `${s.id}:${s.goal?.round}:${s.goal?.status}:${s.goal?.checks?.map((c) => c.last?.at || 0).join(",")}`;
    if (key === this.#recordKey) return;
    this.#recordKey = key;
    const req = ++this.#loading;
    try {
      const rec = await api.get(`/sessions/${encodeURIComponent(s.id)}/goal`);
      if (req !== this.#loading || !this.isConnected) return;
      this.#record = rec;
      this.#paint(true);
    } catch {
      /* the panel still works from the session's goal */
    }
  }

  #paint(force = false) {
    const s = this.#session();
    const g = s?.goal;
    this.hidden = !g;
    if (!g) {
      if (this.innerHTML) this.render("");
      this.#paintKey = "";
      return;
    }
    const key = JSON.stringify([
      g,
      s.state,
      s.state_since,
      s.live,
      s.usage?.cost_usd,
      this.compact,
      [...this.#open],
      this.#roundOpen,
      this.#keepGoing,
      this.#busy,
      this.#record?.rounds?.length,
      this.#commitOpen,
      this.#commitPreview,
      this.#moreOpen,
      this.#mergeOpen,
      this.#mergePreview,
      g.report?.state === "writing" ? Math.floor((Date.now() - g.report.at) / 1000) : 0,
    ]);
    if (!force && key === this.#paintKey) return;
    // Do not repaint under someone typing in one of the forms.
    const typing = this.#keepGoing || this.#commitOpen || this.#moreOpen || this.#mergeOpen;
    if (typing && this.contains(document.activeElement) && !force) return;
    this.#paintKey = key;
    if (!this.compact) this.#loadRecord(s);

    const tone = pillTone(g);
    const pipeline = this.#pipelineMarkup(g);
    if (this.compact) {
      this.render(html`<div class="goal-pill" data-tone="${tone}" title="${g.text}">
          <span class="goal-pill-dot" aria-hidden="true"></span>${pillText(g)}
        </div>
        ${pipeline}`);
      return;
    }

    const meters = limitMeters(g, s);
    const roundsList = roundSummaries(this.#record);
    const recordFor = (checkId) => {
      const latest = roundsList.find((r) => r.results.some((x) => x.check === checkId));
      return latest?.results.find((x) => x.check === checkId) || null;
    };
    const baseline = new Map((g.baseline || []).map((b) => [b.check, b.passed]));
    const paused = g.status === "paused";
    const reason = g.paused?.reason;
    const active = ["working", "paused", "met", "dropped"].includes(g.status);
    const flagged = flaggedChecks(g);

    this.render(html`
      <header class="goal-head">
        <div class="goal-pill" data-tone="${tone}"><span class="goal-pill-dot" aria-hidden="true"></span>${pillText(g)}</div>
        <p class="goal-text">${g.text}</p>
      </header>
      ${pipeline}
      ${paused && g.paused?.message ? html`<p class="goal-paused-msg">${g.paused.message}</p>` : ""}
      ${g.verdict && !g.verdict.approved && g.verdict.concerns?.length
        ? html`<div class="goal-verdict">
            <strong>Reviewer concerns (round ${g.verdict.round})</strong>
            <ul>${g.verdict.concerns.map((c) => html`<li>${c}</li>`)}</ul>
          </div>`
        : ""}

      <div class="goal-actions">
        <button type="button" class="sm" data-act="run" ${active && !this.#busy ? "" : "disabled"}>Run checks now</button>
        ${paused && reason === "guard" && flagged.length > 1
          ? html`<button type="button" class="primary sm" data-act="accept-all" ${this.#busy ? "disabled" : ""}>Accept all (${flagged.length})</button>`
          : ""}
        ${paused && reason !== "guard"
          ? html`<button type="button" class="primary sm" data-act="keep-going" ${this.#busy ? "disabled" : ""}>Keep going</button>`
          : ""}
        ${g.status !== "dropped" && g.status !== "met"
          ? html`<button type="button" class="ghost sm" data-act="drop" ${this.#busy ? "disabled" : ""}>Drop goal</button>`
          : ""}
      </div>
      ${this.#keepGoing && paused ? this.#limitsForm(g, reason) : ""}

      ${g.status === "met" ? this.#reportMarkup(g, s) : ""}

      <div class="goal-criteria-list">
        ${g.criteria.map(
          (k) => html`<section class="goal-crit">
            <h4>${k.text}</h4>
            ${g.checks
              .filter((c) => c.criterion === k.id)
              .map((c) => {
                const manual = c.kind === "manual";
                const rec = recordFor(c.id);
                const open = this.#open.has(c.id);
                const before = baseline.get(c.id);
                return html`<div class="goal-ev" data-tone="${resultTone(c)}">
                  <div class="goal-ev-row">
                    ${manual
                      ? html`<input type="checkbox" data-manual="${c.id}" ${c.done ? "checked" : ""} ${this.#busy ? "disabled" : ""} aria-label="Done: ${c.title}" />`
                      : html`<span class="goal-ev-badge">${resultLabel(c)}</span>`}
                    <span class="goal-ev-title">${c.title}</span>
                    <span class="goal-ev-meta">
                      ${c.stage === "full" && !manual ? "Full · " : ""}${c.last && !manual
                        ? `${(c.last.duration_ms / 1000).toFixed(1)}s · ${relativeTime(c.last.at)}`
                        : ""}
                    </span>
                  </div>
                  ${manual ? "" : html`<code class="goal-ev-cmd">${c.command}</code>`}
                  ${before === true ? html`<p class="goal-baseline sm">Passed before any work, so it proves nothing yet.</p>` : ""}
                  ${c.last?.tampered ? this.#tamperMarkup(c, rec) : ""}
                  ${!manual && rec?.output
                    ? html`<button type="button" class="ghost sm goal-ev-toggle" data-act="output" data-id="${c.id}">${open ? "Hide output" : "Show output"}</button>
                        ${open ? html`<pre class="goal-output">${rec.output}</pre>` : ""}`
                    : ""}
                </div>`;
              })}
          </section>`,
        )}
      </div>

      <div class="goal-meters">
        ${this.#meter("Rounds", `${meters.rounds.used} / ${meters.rounds.limit}`, meters.rounds.frac)}
        ${this.#meter(
          "Cost",
          meters.cost.used == null
            ? "Not reported"
            : `$${meters.cost.used.toFixed(2)}${meters.cost.limit != null ? ` / $${Number(meters.cost.limit).toFixed(2)}` : ""}`,
          meters.cost.frac,
        )}
        ${this.#meter(
          "Working time",
          `${minutesLabel(meters.time.used)}${meters.time.limit != null ? ` / ${meters.time.limit}m` : ""}`,
          meters.time.frac,
        )}
      </div>

      ${roundsList.length
        ? html`<div class="goal-rounds">
            <h4>Rounds</h4>
            <ul>
              ${roundsList.map(
                (r) => html`<li>
                  <button type="button" class="ghost sm goal-round" data-act="round" data-round="${r.round}" aria-expanded="${this.#roundOpen === r.round ? "true" : "false"}">
                    <span>${r.label}</span><span class="muted">${r.passed}/${r.total} pass · ${relativeTime(r.at)}</span>
                  </button>
                  ${this.#roundOpen === r.round
                    ? html`<ul class="goal-round-results">
                        ${r.results.map((x) => {
                          const c = g.checks.find((cc) => cc.id === x.check);
                          const label = x.tampered ? "Changed" : x.skipped ? "Skipped" : x.timed_out ? "Timed out" : x.passed ? "Pass" : "Fail";
                          return html`<li><span class="goal-ev-badge" data-tone="${x.passed && !x.tampered ? "ok" : x.skipped ? "quiet" : "danger"}">${label}</span> ${c?.title || x.check}</li>`;
                        })}
                      </ul>`
                    : ""}
                </li>`,
              )}
            </ul>
          </div>`
        : ""}
    `);
  }

  #tamperMarkup(c, rec) {
    const changed = rec?.changed?.length ? rec.changed : c.guards;
    const created = (rec?.created || []).filter((p) => changed.includes(p));
    const edited = changed.filter((p) => !created.includes(p));
    return html`<div class="goal-tamper sm" ${created.length ? "data-created" : ""}>
      <span>
        ${created.length
          ? html`<strong>The agent created ${created.join(", ")}.</strong> Read it before you
              accept: an accepted file becomes what this check is judged by.`
          : ""}
        ${edited.length ? html` The agent changed ${edited.join(", ")}.` : ""}
      </span>
      <button type="button" class="sm" data-act="accept" data-id="${c.id}" data-created="${created.join(", ")}" ${this.#busy ? "disabled" : ""}>
        ${created.length ? "Accept what it wrote" : "Accept change"}
      </button>
    </div>`;
  }

  #reportMarkup(g, s) {
    const r = g.report;
    if (!r) {
      return html`<section class="goal-report"><h4>Finish report</h4><p class="muted sm">Gathering the changes…</p></section>`;
    }
    const secs = Math.max(0, Math.floor((Date.now() - r.at) / 1000));
    const files = r.files || [];
    const shown = files.slice(0, 8);
    const hidden = files.length - shown.length + (r.more_files || 0);
    const busy = !!this.#busy;
    return html`<section class="goal-report" aria-label="Finish report">
      <h4>Finish report</h4>
      ${r.state === "writing" ? html`<p class="muted sm">Writing the summary… ${secs}s</p>` : ""}
      ${r.summary ? html`<p class="goal-report-summary">${r.summary}</p>` : ""}
      ${r.state === "failed"
        ? html`<p class="goal-report-error sm">The summary could not be written: ${r.error || "unknown error"}
            <button type="button" class="ghost sm" data-act="report" ${busy ? "disabled" : ""}>Try again</button></p>`
        : ""}
      ${r.concerns?.length
        ? html`<div class="goal-report-concerns">
            <strong class="sm">Worth a second look</strong>
            <ul>${r.concerns.map((c) => html`<li>${c}</li>`)}</ul>
          </div>`
        : ""}
      <div class="goal-report-files">
        <div class="goal-report-files-head">
          <span class="sm">${reportFilesLabel(r)}</span>
          ${files.length ? html`<button type="button" class="ghost sm" data-act="open-changes">Open Changes</button>` : ""}
        </div>
        ${shown.length
          ? html`<ul>
              ${shown.map(
                (f) => html`<li><code>${f.path}</code><span class="goal-report-stat"><span class="add">+${f.added}</span> <span class="del">−${f.removed}</span></span></li>`,
              )}
              ${hidden > 0 ? html`<li class="muted sm">and ${hidden} more</li>` : ""}
            </ul>`
          : ""}
      </div>
      ${r.committed
        ? html`<p class="goal-report-done sm">Committed as <code>${r.committed}</code>${s.workspace?.branch ? html` on <code>${s.workspace.branch}</code>` : ""}.</p>`
        : ""}
      ${r.merged
        ? html`<p class="goal-report-done sm">Merged into <code>${s.workspace?.base || "the base"}</code> as <code>${r.merged}</code>.</p>`
        : ""}
      ${this.#commitOpen ? this.#commitForm(r) : ""}
      ${this.#moreOpen ? this.#moreForm(g) : ""}
      ${this.#mergeOpen ? this.#mergeForm() : ""}
      ${this.#commitOpen || this.#moreOpen || this.#mergeOpen
        ? ""
        : html`<div class="goal-actions">
            ${r.committed
              ? ""
              : html`<button type="button" class="primary sm" data-act="commit-open" ${busy || r.state === "writing" ? "disabled" : ""}>Commit…</button>`}
            ${s.workspace?.kind === "worktree" && !r.merged
              ? html`<button type="button" class="${r.committed ? "primary" : ""} sm" data-act="merge-open" ${busy ? "disabled" : ""}>Merge into ${s.workspace.base || "base"}…</button>`
              : ""}
            <button type="button" class="sm" data-act="more-open" ${busy || !s.live ? "disabled" : ""} title="${s.live ? "" : "Resume the session first"}">Keep working…</button>
            <button type="button" class="ghost sm" data-act="close-out">Close out</button>
          </div>`}
    </section>`;
  }

  #commitForm(r) {
    const p = this.#commitPreview;
    const msg = this.#commitDraft ?? r.commit_message ?? "";
    let where = html`<p class="muted sm">Looking at what would be committed…</p>`;
    if (p?.error) where = html`<p class="goal-report-error sm">${p.error}</p>`;
    else if (p && !p.files.length) {
      const commits = p.commits || [];
      where = commits.length
        ? html`<p class="sm">Nothing left to commit: the work is already committed on
              <code>${p.branch || "this branch"}</code>.</p>
            <ul class="goal-commit-list">${commits.map((c) => html`<li><code>${c}</code></li>`)}</ul>`
        : html`<p class="sm">There is nothing to commit in this workspace.</p>`;
      const s = this.#session();
      const canMerge =
        commits.length && s?.workspace?.kind === "worktree" && !s.goal?.report?.merged;
      return html`<div class="goal-form" data-form="commit">
        ${where}
        <div class="goal-actions">
          ${canMerge
            ? html`<button type="button" class="primary sm" data-act="merge-open">Merge into ${s.workspace.base || "base"}…</button>`
            : ""}
          <button type="button" class="ghost sm" data-act="forms-cancel">Close</button>
        </div>
      </div>`;
    } else if (p) {
      const own = p.workspace === "folder";
      where = html`<p class="sm">Commits ${p.files.length} file${p.files.length === 1 ? "" : "s"} on
          <code>${p.branch || "a detached HEAD"}</code>.</p>
        ${own
          ? html`<p class="goal-warning sm">This session runs in your own checkout, so every uncommitted change there is included, not only the agent’s.</p>`
          : ""}`;
    }
    const ready = p && !p.error && p.files.length > 0;
    return html`<div class="goal-form" data-form="commit">
      ${where}
      <label><span class="sm">Commit message</span>
        <textarea data-draft="commit" rows="5" maxlength="2000" spellcheck="true">${msg}</textarea>
      </label>
      <div class="goal-actions">
        <button type="button" class="primary sm" data-act="commit" ${ready && !this.#busy ? "" : "disabled"}>Commit</button>
        <button type="button" class="ghost sm" data-act="forms-cancel">Cancel</button>
      </div>
    </div>`;
  }

  #moreForm(g) {
    const d = this.#moreDraft;
    if (!d.rounds) d.rounds = String(moreRounds(g));
    return html`<div class="goal-form" data-form="more">
      <label><span class="sm">What else should the agent do?</span>
        <textarea data-draft="more" rows="3" maxlength="4000" placeholder="The checks run again after its turn.">${d.text}</textarea>
      </label>
      <label class="goal-form-inline"><span class="sm">Round limit</span>
        <input data-draft="more-rounds" type="number" min="${g.round + 1}" max="10" value="${d.rounds}" />
        <span class="muted sm">${g.round} used</span>
      </label>
      <div class="goal-actions">
        <button type="button" class="primary sm" data-act="more" ${this.#busy ? "disabled" : ""}>Send</button>
        <button type="button" class="ghost sm" data-act="forms-cancel">Cancel</button>
      </div>
    </div>`;
  }

  #mergeForm() {
    const p = this.#mergePreview;
    if (!p) return html`<div class="goal-form"><p class="muted sm">Looking at the branch…</p></div>`;
    if (p.error) {
      return html`<div class="goal-form"><p class="goal-report-error sm">${p.error}</p>
        <div class="goal-actions"><button type="button" class="ghost sm" data-act="forms-cancel">Close</button></div></div>`;
    }
    return html`<div class="goal-form" data-form="merge">
      <p class="sm">Merges <code>${p.branch}</code> into <code>${p.base}</code> in <code>${p.repo}</code>,
        with a merge commit. Nothing is pushed.</p>
      ${p.commits.length
        ? html`<ul class="goal-commit-list">${p.commits.map((c) => html`<li><code>${c}</code></li>`)}</ul>`
        : ""}
      ${p.blocked ? html`<p class="goal-warning sm">${p.blocked}</p>` : ""}
      <div class="goal-actions">
        <button type="button" class="primary sm" data-act="merge" ${p.blocked || this.#busy ? "disabled" : ""}>Merge</button>
        <button type="button" class="ghost sm" data-act="forms-cancel">Cancel</button>
      </div>
    </div>`;
  }

  async #openMerge() {
    const s = this.#session();
    if (!s) return;
    this.#closeForms();
    this.#mergeOpen = true;
    this.#paint(true);
    try {
      this.#mergePreview = await api.get(`/sessions/${encodeURIComponent(s.id)}/goal/merge`);
    } catch (err) {
      this.#mergePreview = { error: err instanceof ApiError ? err.message : "Could not read the branch." };
    }
    if (this.#mergeOpen) this.#paint(true);
  }

  #closeForms() {
    this.#mergeOpen = false;
    this.#mergePreview = null;
    this.#commitOpen = false;
    this.#commitPreview = null;
    this.#commitDraft = null;
    this.#moreOpen = false;
    this.#moreDraft = { text: "", rounds: "" };
  }

  async #openCommit() {
    const s = this.#session();
    if (!s) return;
    this.#closeForms();
    this.#commitOpen = true;
    this.#paint(true);
    try {
      this.#commitPreview = await api.get(`/sessions/${encodeURIComponent(s.id)}/goal/commit`);
    } catch (err) {
      this.#commitPreview = { error: err instanceof ApiError ? err.message : "Could not read the workspace." };
    }
    if (this.#commitOpen) this.#paint(true);
    this.querySelector('[data-draft="commit"]')?.focus();
  }

  #pipelineMarkup(g) {
    const list = stages(g);
    return html`<ol class="goal-pipe" aria-label="Goal progress">
      ${list.map(
        (st, i) => html`<li class="goal-stage" data-state="${st.state}">
          <span class="goal-stage-node" aria-hidden="true"></span>
          <span class="goal-stage-label">${st.label}</span>
          ${st.detail ? html`<span class="goal-stage-detail">${st.detail}</span>` : ""}
          ${i < list.length - 1 ? html`<span class="goal-stage-link" aria-hidden="true"></span>` : ""}
        </li>`,
      )}
    </ol>`;
  }

  #meter(label, value, frac) {
    const pct = frac == null ? 0 : Math.round(frac * 100);
    return html`<div class="sess-meter" ${frac != null && frac >= 0.8 ? "data-warn" : ""}>
      <div class="sess-meter-label"><span>${label}</span><span>${value}</span></div>
      <div class="sess-meter-bar" role="meter" aria-valuenow="${pct}" aria-valuemin="0" aria-valuemax="100">
        <span style="width:${pct}%"></span>
      </div>
    </div>`;
  }

  #limitsForm(g, reason) {
    const d = this.#limitDraft || {
      rounds: String(Math.max(g.limits.rounds, reason === "rounds" ? g.round + 1 : g.limits.rounds)),
      cost: g.limits.cost_usd != null ? String(g.limits.cost_usd) : "",
      minutes: g.limits.minutes != null ? String(g.limits.minutes) : "",
    };
    this.#limitDraft = d;
    const hint = {
      rounds: "Raise the round limit to give the agent another round.",
      cost: "Raise the cost limit, then keep going.",
      time: "Raise the working time limit, then keep going.",
      guard: "Accept each changed check first.",
      manual: "Tick the manual checks to finish.",
      ended: "Resume the session first.",
      review: "The reviewer runs again with the next round.",
    }[reason] || "";
    return html`<div class="goal-keep">
      <p class="muted sm">${hint}</p>
      <div class="goal-limit-row">
        <label><span>Rounds</span><input data-limit="rounds" type="number" min="1" max="10" value="${d.rounds}" /></label>
        <label><span>Cost ($)</span><input data-limit="cost" inputmode="decimal" placeholder="No limit" value="${d.cost}" /></label>
        <label><span>Time (min)</span><input data-limit="minutes" inputmode="numeric" placeholder="No limit" value="${d.minutes}" /></label>
      </div>
      <div class="goal-actions">
        <button type="button" class="primary sm" data-act="continue" ${this.#busy ? "disabled" : ""}>Keep going</button>
        <button type="button" class="ghost sm" data-act="cancel-keep">Cancel</button>
      </div>
    </div>`;
  }

  #onLimitInput(ev) {
    const d = ev.target.dataset?.draft;
    if (d === "commit") this.#commitDraft = ev.target.value;
    else if (d === "more") this.#moreDraft.text = ev.target.value;
    else if (d === "more-rounds") this.#moreDraft.rounds = ev.target.value;
    const k = ev.target.dataset?.limit;
    if (!k || !this.#limitDraft) return;
    this.#limitDraft[k] = ev.target.value;
  }

  #onChange(ev) {
    const id = ev.target.dataset?.manual;
    if (!id) return;
    this.#act("manual", { check: id, done: ev.target.checked });
  }

  #onClick(ev) {
    const btn = ev.target.closest("[data-act]");
    if (!btn) return;
    ev.stopPropagation();
    const act = btn.dataset.act;
    if (act === "output") {
      const id = btn.dataset.id;
      if (this.#open.has(id)) this.#open.delete(id);
      else this.#open.add(id);
      this.#paint(true);
    } else if (act === "round") {
      const r = Number(btn.dataset.round);
      this.#roundOpen = this.#roundOpen === r ? null : r;
      this.#paint(true);
    } else if (act === "keep-going") {
      this.#keepGoing = true;
      this.#limitDraft = null;
      this.#paint(true);
    } else if (act === "cancel-keep") {
      this.#keepGoing = false;
      this.#paint(true);
    } else if (act === "continue") {
      const d = this.#limitDraft || {};
      const limits = {
        rounds: Number.parseInt(d.rounds, 10),
        cost_usd: parseLimit(d.cost),
        minutes: parseLimit(d.minutes, { integer: true }),
      };
      const err = Number.isNaN(limits.cost_usd) || Number.isNaN(limits.minutes)
        ? "Limits must be numbers."
        : validateLimits(limits);
      if (err) {
        showToast(err, { kind: "error" });
        return;
      }
      this.#act("continue", { limits }).then((ok) => {
        if (ok) {
          this.#keepGoing = false;
          this.#paint(true);
        }
      });
    } else if (act === "run") {
      this.#act("run");
    } else if (act === "accept") {
      const created = btn.dataset.created;
      if (
        created &&
        !window.confirm(
          `Accept ${created} as the agent wrote it? From now on this check is judged by that file.`,
        )
      ) {
        return;
      }
      this.#act("accept", { check: btn.dataset.id });
    } else if (act === "accept-all") {
      const created = flaggedChecks(this.#session()?.goal).length;
      if (window.confirm(`Accept the changes to all ${created} checks? The checks then run again.`)) {
        this.#act("accept", {});
      }
    } else if (act === "report") {
      this.#act("report");
    } else if (act === "open-changes") {
      const s = this.#session();
      if (s) route.go({ name: "session", id: s.id, tool: "changes" });
    } else if (act === "merge-open") {
      this.#openMerge();
    } else if (act === "merge") {
      this.#act("merge").then((ok) => {
        if (ok) {
          this.#closeForms();
          showToast(`Merged into ${this.#session()?.workspace?.base || "the base"}.`);
          this.#paint(true);
        }
      });
    } else if (act === "commit-open") {
      this.#openCommit();
    } else if (act === "more-open") {
      this.#closeForms();
      this.#moreOpen = true;
      this.#paint(true);
      this.querySelector('[data-draft="more"]')?.focus();
    } else if (act === "forms-cancel") {
      this.#closeForms();
      this.#paint(true);
    } else if (act === "commit") {
      const message = (this.#commitDraft ?? this.#session()?.goal?.report?.commit_message ?? "").trim();
      if (!message) {
        showToast("Write a commit message first.", { kind: "error" });
        return;
      }
      this.#act("commit", { message }).then((ok) => {
        if (ok) {
          this.#closeForms();
          showToast("Committed.");
          this.#paint(true);
        }
      });
    } else if (act === "more") {
      const text = this.#moreDraft.text.trim();
      const rounds = Number.parseInt(this.#moreDraft.rounds, 10);
      if (!text) {
        showToast("Say what else the agent should do.", { kind: "error" });
        return;
      }
      this.#act("more", { text, rounds: Number.isNaN(rounds) ? null : rounds }).then((ok) => {
        if (ok) {
          this.#closeForms();
          this.#paint(true);
        }
      });
    } else if (act === "close-out") {
      const s = this.#session();
      if (s) openCloseOut(s.id);
    } else if (act === "drop") {
      if (window.confirm("Drop this goal? The session carries on without checks.")) this.#act("drop");
    }
  }

  async #act(what, body) {
    const s = this.#session();
    if (!s || this.#busy) return false;
    this.#busy = what;
    this.#paint(true);
    try {
      await api.post(`/sessions/${encodeURIComponent(s.id)}/goal/${what}`, body);
      return true;
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : "That did not work.", { kind: "error" });
      return false;
    } finally {
      this.#busy = "";
      this.#paint(true);
    }
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-goal-panel")) {
  customElements.define("dync-goal-panel", DyncGoalPanel);
}
