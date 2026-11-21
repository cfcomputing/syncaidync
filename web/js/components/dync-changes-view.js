import { DyncElement, html, escapeText, raw } from "../lib/element.js";
import { api, ApiError } from "../lib/api.js";
import { store } from "../lib/store.js";
import * as prefs from "../lib/prefs.js";
import { unifiedRows, splitRows } from "../lib/diffview.js";
import {
  isReviewed,
  progress,
  toggleReviewed,
  addNote,
  editNote,
  removeNote,
  notesFor,
  unsent,
  markSent,
} from "../lib/review.js";
import { composeMessage } from "../lib/notes.js";
import { layoutMap, providersFirst } from "../lib/maplayout.js";
import { isMarkdownPath } from "../lib/filetree.js";
import { fromServerHtml } from "../lib/markdown.js";
import { byStartLine, quoteLines, relocateAll, relocateMargin } from "../lib/margins.js";
import { askTextNear } from "../lib/prompt.js";
import { openMenuBelow } from "../lib/menu.js";
import { showToast } from "../lib/toast.js";

const MAP_MIN_K = 0.1;
const MAP_MAX_K = 4;

function uuid() {
  if (globalThis.crypto?.randomUUID) return crypto.randomUUID();
  return `n-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function statusLetter(status) {
  return ({ added: "A", modified: "M", deleted: "D", renamed: "R" }[status] || "?");
}

export class DyncChangesView extends DyncElement {
  static get observedAttributes() {
    return ["source-kind", "source-id"];
  }

  #cs = null;
  #review = { reviewed: {}, notes: [], overall: "", plan: null, updated_at: 0 };
  #selected = null;
  #view = "unified";
  #sort = "folder";
  #mapOpen = true;
  #mapSize = { w: 1, h: 1 };
  #mapView = null; // { k, x, y } once you zoom or pan; null means fit the map to its box
  #mapStamp = null;
  #mapDragged = false;
  #error = null;
  #loadVersion = 0;
  #turnHint = false;
  #prevState = null;
  #saveTimer = null;
  #noteDraft = null;
  #built = false;
  #hunkIndex = 0;
  #planMode = false;
  #checkpointIndex = 0;
  #planning = false;
  #planError = null;
  #planSeconds = 0;
  #planTimer = null;
  #planAgent = "claude";
  #planModel = "";
  /** @type {"view"|"diff"} */
  #mdMode = "view";
  #mdHtml = "";
  #mdText = "";
  #mdError = null;
  #mdPath = null;
  #mdToken = 0;
  #margins = [];
  #marginsPath = null;
  #marginPeek = null;
  #marginPeekToken = 0;
  /** @type {{ path: string, start: number, end: number } | null} */
  #marginLineDraft = null;

  get source() {
    return {
      kind: this.getAttribute("source-kind") || "session",
      id: this.getAttribute("source-id") || "",
    };
  }

  set source(v) {
    if (!v) return;
    this.setAttribute("source-kind", v.kind || "session");
    this.setAttribute("source-id", v.id || "");
  }

  mount() {
    this.classList.add("changes-view");
    this.tabIndex = 0;
    const pref = prefs.get("changes", { view: "unified" });
    this.#view = pref.view === "split" ? "split" : "unified";
    this.#mdMode = prefs.get("changes-md-mode", "view") === "diff" ? "diff" : "view";
    this.#mapOpen = prefs.get("changes-map-open", true) !== false;
    this.#sort = prefs.get("changes-sort", "folder") === "providers" ? "providers" : "folder";
    const split = prefs.get("changes-split", {});
    if (split?.side > 0) this.style.setProperty("--ch-side-w", `${split.side}px`);
    if (split?.map > 0) this.style.setProperty("--ch-map-h", `${split.map}px`);
    this.#built = true;
    this.#bind();
    this.#load();
  }

  unmount() {
    this.#built = false;
    this.#loadVersion += 1;
    clearInterval(this.#planTimer);
    this.#flushReview();
  }

  attributeChangedCallback() {
    if (this.#built) this.#load();
  }

  #apiBase() {
    const { kind, id } = this.source;
    if (kind === "pull") return `/pulls/${id}`;
    return `/sessions/${id}`;
  }

  #bind() {
    this.on(this, "click", (ev) => this.#onClick(ev));
    this.on(this, "keydown", (ev) => this.#onKey(ev));
    this.on(this, "input", (ev) => this.#onInput(ev));
    this.on(this, "change", (ev) => this.#onInput(ev));
    this.on(window, "pagehide", () => this.#flushReview());
    this.on(this, "wheel", (ev) => this.#onMapWheel(ev), { passive: false });
    this.on(this, "pointerdown", (ev) => this.#onPointerDown(ev));
    this.on(this, "dblclick", (ev) => this.#onSplitReset(ev));
    if (typeof ResizeObserver !== "undefined") {
      const ro = new ResizeObserver(() => this.#applyMapView());
      ro.observe(this);
      this.cleanup(() => ro.disconnect());
    }
    let last = store.sessions.get(this.source.id)?.state;
    const unsub = store.subscribe(() => {
      if (this.source.kind !== "session") return;
      const next = store.sessions.get(this.source.id)?.state;
      if (last === "working" && next === "ready") {
        this.#turnHint = true;
        this.#paint();
      }
      last = next;
    });
    this.cleanup(unsub);
  }

  reload() {
    return this.#load();
  }

  async #load() {
    const id = this.source.id;
    if (!id) return;
    const version = ++this.#loadVersion;
    const base = this.#apiBase();
    this.#error = null;
    if (!this.#cs) this.render(html`<p class="empty" role="status">Loading changes…</p>`);
    try {
      const [cs, review] = await Promise.all([
        api.get(`${base}/changes`),
        api.get(`${base}/review`),
      ]);
      if (version !== this.#loadVersion) return;
      this.#cs = cs;
      if (cs?.stamp !== this.#mapStamp) {
        this.#mapStamp = cs?.stamp ?? null;
        this.#mapView = null;
      }
      this.#review = review || { reviewed: {}, notes: [], overall: "", plan: null };
      if (this.#selected && !cs.files.some((f) => f.path === this.#selected)) {
        this.#selected = cs.files[0]?.path || null;
      } else if (!this.#selected) {
        this.#selected = cs.files[0]?.path || null;
      }
      this.#hunkIndex = 0;
      this.#mdHtml = "";
      this.#mdText = "";
      this.#mdError = null;
      this.#mdPath = null;
      this.#margins = [];
      this.#marginsPath = null;
      this.#marginPeek = null;
      const session = store.sessions.get(this.source.id);
      if (session?.agent && session.agent !== "shell") {
        this.#planAgent = session.agent;
      }
      if (session?.model) this.#planModel = session.model;
    } catch (err) {
      if (version !== this.#loadVersion) return;
      this.#cs = null;
      this.#error = err.message || "Could not load changes.";
    }
    this.#paint();
  }

  #scheduleSave() {
    clearTimeout(this.#saveTimer);
    this.#saveTimer = setTimeout(() => this.#flushReview(), 600);
  }

  async #flushReview() {
    if (this.#saveTimer === null) return;
    clearTimeout(this.#saveTimer);
    this.#saveTimer = null;
    if (!this.source.id || this.#error) return;
    try {
      const saved = await api.put(`${this.#apiBase()}/review`, {
        reviewed: this.#review.reviewed || {},
        notes: this.#review.notes || [],
        overall: this.#review.overall || "",
      });
      this.#review = saved;
      this.dispatchEvent(new CustomEvent("dync-review-saved", { bubbles: true, detail: saved }));
    } catch (_) {
      /* keep local */
    }
  }

  #filesOrdered() {
    const files = this.#cs?.files || [];
    if (this.#sort === "providers") return providersFirst(files, this.#cs.map);
    return files;
  }

  #selectedFile() {
    return (this.#cs?.files || []).find((f) => f.path === this.#selected) || null;
  }

  #paint() {
    if (this.#error && !this.#cs) {
      this.render(
        html`<div class="empty changes-empty">
          <h2>Changes</h2>
          <p>${this.#error}</p>
        </div>`,
      );
      return;
    }
    const prog = progress(this.#review, this.#cs?.files || []);
    const unsentNotes = unsent(this.#review);
    const session = store.sessions.get(this.source.id);
    const live = !!session?.live;
    const sendTitle = live
      ? "Send unsent notes to the agent"
      : "The session is not live, so notes cannot be sent.";

    this.render(
      html`<div class="ch-root">
        <div class="ch-top">
          <span class="ch-progress">${prog.reviewed} of ${prog.total} reviewed</span>
          <button type="button" class="ghost sm" data-act="refresh">Refresh</button>
          <span class="ch-switch" role="group" aria-label="Diff view">
            <button type="button" class="ghost sm ${this.#view === "unified" ? "active" : ""}" data-act="view" data-v="unified">Unified</button>
            <button type="button" class="ghost sm ${this.#view === "split" ? "active" : ""}" data-act="view" data-v="split">Split</button>
          </span>
          <span class="ch-switch" role="group" aria-label="Sort">
            <button type="button" class="ghost sm ${this.#sort === "folder" ? "active" : ""}" data-act="sort" data-v="folder">By folder</button>
            <button type="button" class="ghost sm ${this.#sort === "providers" ? "active" : ""}" data-act="sort" data-v="providers">Providers first</button>
          </span>
          ${this.source.kind === "session"
            ? html`<button type="button" class="sm" data-act="send" ${live && unsentNotes.length ? "" : "disabled"} title="${escapeText(sendTitle)}">
                Send notes${unsentNotes.length ? ` (${unsentNotes.length})` : ""}
              </button>`
            : ""}
          ${this.#planControlsHtml()}
        </div>
        ${this.#planBarHtml()}
        ${this.#turnHint
          ? html`<div class="ch-hint">The agent finished a turn. <button type="button" class="ghost sm" data-act="refresh">Refresh?</button></div>`
          : ""}
        <div class="ch-body">
          <div class="ch-side">
            ${this.#mapHtml()}
            ${this.#fileListHtml()}
          </div>
          <div class="ch-split ch-split-x" data-split="side" role="separator"
            aria-orientation="vertical" aria-label="Resize file side" title="Drag to resize. Double-click to reset."></div>
          <div class="ch-main">
            ${this.#overallHtml()}
            ${this.#mdToolbarHtml()}
            <div class="ch-main-stack">
              ${this.#diffHtml()}
              ${this.#marginPeekHtml()}
            </div>
          </div>
        </div>
      </div>`,
    );
    this.#applyMapView();
    if (this.#shouldShowMdView()) this.#ensureMdView();
    this.#ensureMargins();
    this.#hydrateNoteMarkdown();
  }

  #marginsOnNewLine(num) {
    if (num == null) return [];
    return byStartLine(this.#margins).get(Number(num)) || [];
  }

  async #ensureMargins() {
    const file = this.#selectedFile();
    if (!file || file.status === "deleted" || !this.source.id) {
      this.#margins = [];
      this.#marginsPath = null;
      return;
    }
    const path = file.path;
    if (this.#marginsPath === path) return;
    const base = this.#apiBase();
    try {
      const [listed, fileData] = await Promise.all([
        api.get(`${base}/margins?path=${encodeURIComponent(path)}`),
        api.get(`${base}/file?path=${encodeURIComponent(path)}`).catch(() => null),
      ]);
      if (this.#selected !== path) return;
      const text = fileData?.text || "";
      const { margins } = relocateAll(text, listed.margins || []);
      this.#margins = margins;
      this.#marginsPath = path;
      if (!this.#shouldShowMdView()) this.#paint();
    } catch (_) {
      if (this.#selected !== path) return;
      this.#margins = [];
      this.#marginsPath = path;
    }
  }

  async #openMarginPeek(mid) {
    if (!this.source.id) return;
    const token = ++this.#marginPeekToken;
    const status =
      this.#margins.find((m) => m.id === mid)?.status || "ok";
    this.#marginPeek = { loading: true };
    this.#paint();
    try {
      const data = await api.get(
        `${this.#apiBase()}/margins/${encodeURIComponent(mid)}`,
      );
      if (token !== this.#marginPeekToken) return;
      this.#marginPeek = { ...data, status };
      this.#paint();
    } catch (err) {
      if (token !== this.#marginPeekToken) return;
      this.#marginPeek = {
        error: err.message || "Could not open that margin.",
      };
      this.#paint();
    }
  }

  async #addMarginOnFile(anchor, start, end) {
    const file = this.#selectedFile();
    if (!file || file.status === "deleted" || !this.source.id) return;
    let a = Number(start);
    let b = Number(end);
    if (!a) {
      if (this.#marginLineDraft?.path === file.path) {
        a = this.#marginLineDraft.start;
        b = this.#marginLineDraft.end;
      }
    }
    if (!a) {
      showToast("Click + beside a new-side line to place the margin.", { kind: "error" });
      return;
    }
    if (!b || b < a) b = a;
    const linesLabel = a === b ? `Line ${a}` : `Lines ${a}–${b}`;
    const title = await askTextNear(anchor, {
      title: `Add margin · ${linesLabel}`,
      placeholder: "Title",
      confirmLabel: "Add",
      requiredMessage: "Give the margin a title.",
    });
    if (title == null) return;
    const trimmed = title.trim();
    const base = this.#apiBase();
    let text = "";
    try {
      const fileData = await api.get(`${base}/file?path=${encodeURIComponent(file.path)}`);
      text = fileData?.text || "";
    } catch (_) {
      /* quote may be empty */
    }
    const mid = uuid();
    try {
      await api.put(`${base}/margins/${encodeURIComponent(mid)}`, {
        id: mid,
        path: file.path,
        start: a,
        end: b,
        quote: quoteLines(text, a, b),
        title: trimmed,
        body: "",
        media: [],
      });
      this.#marginLineDraft = { path: file.path, start: a, end: b };
      this.#marginsPath = null;
      await this.#ensureMargins();
      await this.#openMarginPeek(mid);
    } catch (err) {
      showToast(err.message || "Could not add that margin.", { kind: "error" });
    }
  }

  #marginMarkHtml(num) {
    if (num == null) return `<span class="ch-margin-slot"></span>`;
    const marks = this.#marginsOnNewLine(num);
    if (!marks.length) {
      return `<button type="button" class="ch-margin-add" data-act="add-margin-line" data-num="${num}" title="Add margin on line ${num}">+</button>`;
    }
    const outdated = marks.some((m) => m.status === "outdated");
    const ids = marks.map((m) => m.id).join(",");
    const title = marks.length === 1 ? marks[0].title || "Margin" : `${marks.length} margins`;
    return `<button type="button" class="ch-margin-mark${outdated ? " is-outdated" : ""}" data-act="open-margin" data-ids="${escapeText(ids)}" data-num="${num}" title="${escapeText(title)}">¶</button>`;
  }

  #marginPeekHtml() {
    const p = this.#marginPeek;
    if (!p) return "";
    if (p.loading) {
      return html`<aside class="ch-margin-peek"><div class="empty">Loading margin…</div></aside>`;
    }
    if (p.error) {
      return html`<aside class="ch-margin-peek">
        <header class="ch-margin-peek-head">
          <strong>Margin</strong>
          <button type="button" class="ghost sm" data-act="close-margin">Close</button>
        </header>
        <p class="empty">${escapeText(p.error)}</p>
      </aside>`;
    }
    const lines =
      p.start === p.end ? `line ${p.start}` : `lines ${p.start}–${p.end}`;
    const stale = p.status === "outdated" ? html`<span class="ch-outdated">outdated</span>` : "";
    return html`<aside class="ch-margin-peek">
      <header class="ch-margin-peek-head">
        <strong>${escapeText(p.title || "Margin")}</strong>
        <span class="muted">${escapeText(lines)}</span>
        ${stale}
        <button type="button" class="ghost sm" data-act="close-margin">Close</button>
      </header>
      <div class="ch-margin-peek-body md-body md-body-compact">${fromServerHtml(p.body_html || "")}</div>
    </aside>`;
  }

  #selectedIsMarkdown() {
    const file = this.#selectedFile();
    return !!(file && isMarkdownPath(file.path));
  }

  #shouldShowMdView() {
    return this.#selectedIsMarkdown() && this.#mdMode === "view" && !this.#checkpointHunkIds();
  }

  #mdToolbarHtml() {
    if (!this.#selectedIsMarkdown() || this.#checkpointHunkIds()) return "";
    return html`<div class="ch-md-bar">
      <span class="ch-switch" role="group" aria-label="Markdown pane">
        <button type="button" class="ghost sm ${this.#mdMode === "view" ? "active" : ""}" data-act="md-mode" data-v="view">View</button>
        <button type="button" class="ghost sm ${this.#mdMode === "diff" ? "active" : ""}" data-act="md-mode" data-v="diff">Diff</button>
      </span>
    </div>`;
  }

  async #ensureMdView() {
    const file = this.#selectedFile();
    if (!file) return;
    if (file.status === "deleted") {
      this.#mdHtml = "";
      this.#mdText = "";
      this.#mdError = null;
      this.#mdPath = file.path;
      this.#paintMdPane();
      return;
    }
    if (this.#mdPath === file.path && (this.#mdHtml || this.#mdError)) {
      this.#paintMdPane();
      return;
    }
    const token = ++this.#mdToken;
    const path = file.path;
    try {
      let text = "";
      if (this.source.kind === "session") {
        const data = await api.get(
          `/sessions/${this.source.id}/file?path=${encodeURIComponent(path)}`,
        );
        text = data.text || "";
      } else {
        const data = await api.get(
          `/pulls/${this.source.id}/file?path=${encodeURIComponent(path)}`,
        );
        text = data.text || "";
      }
      let htmlOut = "";
      if (this.source.kind === "session") {
        const rendered = await api.post(`/sessions/${this.source.id}/markdown`, {
          text,
          base_path: path,
          source_lines: true,
        });
        htmlOut = rendered.html || "";
      } else {
        const rendered = await api.post("/markdown", {
          text,
          source_lines: true,
        });
        htmlOut = rendered.html || "";
      }
      if (token !== this.#mdToken || this.#selected !== path) return;
      this.#mdHtml = htmlOut;
      this.#mdText = text;
      this.#mdError = null;
      this.#mdPath = path;
      this.#paintMdPane();
    } catch (err) {
      if (token !== this.#mdToken) return;
      this.#mdHtml = "";
      this.#mdText = "";
      this.#mdError = err.message || "Could not render this markdown.";
      this.#mdPath = path;
      this.#paintMdPane();
    }
  }

  #paintMdPane() {
    const pane = this.querySelector("[data-md-pane]");
    if (!pane) return;
    const file = this.#selectedFile();
    if (file?.status === "deleted") {
      pane.innerHTML = `<div class="empty">This markdown file was deleted.</div>`;
      return;
    }
    if (this.#mdError) {
      pane.innerHTML = `<div class="empty">${escapeText(this.#mdError)}</div>`;
      return;
    }
    const markup = html`<div class="md-body">${fromServerHtml(this.#mdHtml)}</div>
      <div class="ch-md-loose" data-md-loose hidden></div>`;
    pane.innerHTML = markup.value;
    this.#decorateMdBlocks(pane);
    this.#placeMdNotes(pane);
    this.#hydrateNoteMarkdown();
  }

  #decorateMdBlocks(pane) {
    const body = pane.querySelector(".md-body");
    if (!body) return;
    for (const el of [...body.querySelectorAll("[data-source-line]")]) {
      if (el.closest(".md-sourced-block")) continue;
      const line = el.getAttribute("data-source-line");
      if (!line) continue;
      const wrap = document.createElement("div");
      wrap.className = "md-sourced-block";
      wrap.dataset.line = line;
      el.parentNode.insertBefore(wrap, el);
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "md-line-note-btn";
      btn.setAttribute("data-act", "md-line-note");
      btn.setAttribute("data-num", line);
      btn.title = `Note on line ${line}`;
      btn.textContent = "+";
      wrap.appendChild(btn);
      wrap.appendChild(el);
    }
  }

  #placeMdNotes(pane) {
    const file = this.#selectedFile();
    const loose = pane.querySelector("[data-md-loose]");
    if (!file || !loose) return;
    const lineNotes = notesFor(this.#review, file.path).filter(
      (n) => n.start != null && n.end != null,
    );
    const blocks = [...pane.querySelectorAll(".md-sourced-block")];
    const placed = new Set();
    for (const note of lineNotes) {
      const start = Number(note.start);
      const end = Number(note.end);
      let best = null;
      for (const wrap of blocks) {
        const line = Number(wrap.dataset.line);
        if (!line) continue;
        if (line >= start && line <= end) {
          best = wrap;
          break;
        }
        if (line <= start) best = wrap;
      }
      if (!best) continue;
      best.insertAdjacentHTML("afterend", this.#noteCard(note, false));
      placed.add(note.id);
    }
    const leftover = lineNotes.filter((n) => !placed.has(n.id));
    if (!leftover.length) {
      loose.hidden = true;
      loose.innerHTML = "";
      return;
    }
    loose.hidden = false;
    loose.innerHTML = `<div class="ch-md-loose-label">Notes not tied to the current text</div>${leftover
      .map((n) => this.#noteCard(n, false))
      .join("")}`;
  }

  #planControlsHtml() {
    const agents = (store.meta?.agents || []).filter((a) => a.installed && a.id !== "shell");
    if (!agents.length && !this.#planning) return "";
    const options = agents
      .map(
        (a) =>
          `<option value="${escapeText(a.id)}" ${a.id === this.#planAgent ? "selected" : ""}>${escapeText(a.label || a.id)}</option>`,
      )
      .join("");
    if (this.#planning) {
      return html`<span class="ch-planning">Planning… ${this.#planSeconds}s — This can take a minute</span>`;
    }
    return html`<span class="ch-plan-controls">
      <select data-act="plan-agent">${raw(options)}</select>
      <input type="text" data-act="plan-model" placeholder="model" value="${escapeText(this.#planModel)}" />
      <button type="button" class="ghost sm" data-act="plan" ${agents.length ? "" : "disabled"}>Plan my read-through</button>
    </span>`;
  }

  #planBarHtml() {
    const plan = this.#review?.plan;
    if (!plan) {
      if (this.#planError) {
        return html`<div class="ch-hint ch-plan-error">${escapeText(this.#planError)}
          <button type="button" class="ghost sm" data-act="plan">Try again</button></div>`;
      }
      return "";
    }
    const stale = plan.stamp && this.#cs?.stamp && plan.stamp !== this.#cs.stamp;
    const cps = plan.checkpoints || [];
    const i = Math.min(this.#checkpointIndex, Math.max(0, cps.length - 1));
    const cp = cps[i];
    if (this.#planMode && cp) {
      const looks = (cp.look_for || [])
        .map((t) => `<li>${escapeText(t)}</li>`)
        .join("");
      return html`<div class="ch-plan-bar">
        ${stale
          ? html`<div class="ch-hint">This plan was made for an earlier version of the changes.
              <button type="button" class="ghost sm" data-act="plan">Plan again</button></div>`
          : ""}
        <div class="ch-plan-nav">
          <span>Checkpoint ${i + 1} of ${cps.length}</span>
          <button type="button" class="ghost sm" data-act="cp-prev" ${i === 0 ? "disabled" : ""}>Previous</button>
          <button type="button" class="ghost sm" data-act="cp-next" ${i >= cps.length - 1 ? "disabled" : ""}>Next</button>
          <button type="button" class="ghost sm" data-act="plan-all">All files</button>
        </div>
        <h3 class="ch-plan-title">${escapeText(cp.title)}</h3>
        <p class="ch-plan-why">${escapeText(cp.why)}</p>
        ${looks ? html`<ul class="ch-plan-look">${raw(looks)}</ul>` : ""}
        ${plan.summary ? html`<p class="ch-plan-summary">${escapeText(plan.summary)}</p>` : ""}
      </div>`;
    }
    return html`<div class="ch-plan-bar">
      ${stale
        ? html`<div class="ch-hint">This plan was made for an earlier version of the changes.
            <button type="button" class="ghost sm" data-act="plan">Plan again</button></div>`
        : ""}
      <button type="button" class="ghost sm" data-act="plan-mode">Follow checkpoints (${cps.length})</button>
      ${this.#planError ? html`<span class="ch-plan-error">${escapeText(this.#planError)}</span>` : ""}
    </div>`;
  }

  #checkpointHunkIds() {
    if (!this.#planMode) return null;
    const cps = this.#review?.plan?.checkpoints || [];
    const cp = cps[this.#checkpointIndex];
    return new Set(cp?.hunks || []);
  }

  #mapHtml() {
    if (!this.#mapOpen) {
      return html`<div class="ch-map"><div class="ch-map-bar">
        <button type="button" class="ghost sm" data-act="toggle-map">Show map</button>
      </div></div>`;
    }
    const layout = layoutMap(this.#cs?.map || { nodes: [], edges: [] });
    this.#mapSize = { w: layout.width, h: layout.height };
    const byPath = Object.fromEntries(layout.nodes.map((n) => [n.path, n]));
    const edges = (layout.edges || [])
      .map((e) => {
        const a = byPath[e.from];
        const b = byPath[e.to];
        if (!a || !b) return "";
        const x1 = a.x + 70;
        const y1 = a.y + 12;
        const x2 = b.x;
        const y2 = b.y + 12;
        const mx = (x1 + x2) / 2;
        return `<path d="M${x1},${y1} C${mx},${y1} ${mx},${y2} ${x2},${y2}" class="ch-edge" marker-end="url(#ch-arrow)"/>`;
      })
      .join("");
    const nodes = layout.nodes
      .map((n) => {
        const name = n.path.split("/").pop();
        const file = (this.#cs?.files || []).find((f) => f.path === n.path);
        const reviewed = file && isReviewed(this.#review, file);
        const inCp = this.#fileInCheckpoint(n.path);
        const cls = [
          "ch-node",
          n.changed ? "is-changed" : "is-unchanged",
          n.path === this.#selected ? "is-selected" : "",
          reviewed ? "is-reviewed" : "",
          inCp ? "in-checkpoint" : "",
        ]
          .filter(Boolean)
          .join(" ");
        return `<g class="${cls}" data-act="select" data-path="${escapeText(n.path)}" transform="translate(${n.x},${n.y})">
          <title>${escapeText(n.path)}</title>
          <rect width="140" height="24" rx="6"/>
          <text x="10" y="16">${escapeText(name)}${reviewed ? " ✓" : ""}</text>
        </g>`;
      })
      .join("");
    return html`<div class="ch-map is-open">
      <div class="ch-map-bar">
        <button type="button" class="ghost sm" data-act="toggle-map">Hide map</button>
        <span class="ch-map-count">${layout.nodes.length} files</span>
        <span class="ch-switch ch-map-zoom" role="group" aria-label="Map zoom">
          <button type="button" class="ghost sm" data-act="map-zoom" data-v="out" title="Zoom out" aria-label="Zoom out">−</button>
          <button type="button" class="ghost sm" data-act="map-fit" title="Fit the whole map">Fit</button>
          <button type="button" class="ghost sm" data-act="map-zoom" data-v="in" title="Zoom in" aria-label="Zoom in">+</button>
        </span>
      </div>
      <div class="ch-map-view" data-el="map-view" title="Drag or scroll to move. Pinch or Ctrl+scroll to zoom.">
        <svg class="ch-map-svg" width="100%" height="100%">
          <defs>
            <marker id="ch-arrow" markerWidth="6" markerHeight="6" refX="5" refY="3" orient="auto">
              <path d="M0,0 L6,3 L0,6 Z" fill="currentColor"/>
            </marker>
          </defs>
          <g data-el="map-world">
            ${raw(edges)}
            ${raw(nodes)}
          </g>
        </svg>
      </div>
    </div>
    <div class="ch-split ch-split-y" data-split="map" role="separator" aria-orientation="horizontal"
      aria-label="Resize map" title="Drag to resize. Double-click to reset."></div>`;
  }

  /** The map's fit-to-box view: whole map visible, never larger than 125%. */
  #mapFit() {
    const view = this.querySelector("[data-el=map-view]");
    const cw = view?.clientWidth || 0;
    const ch = view?.clientHeight || 0;
    if (!cw || !ch) return null;
    const { w, h } = this.#mapSize;
    const k = Math.max(MAP_MIN_K, Math.min(cw / w, ch / h, 1.25));
    return { k, x: (cw - w * k) / 2, y: Math.max(0, (ch - h * k) / 2) };
  }

  #applyMapView() {
    const world = this.querySelector("[data-el=map-world]");
    if (!world) return;
    const v = this.#mapView || this.#mapFit();
    if (!v) return;
    world.setAttribute("transform", `translate(${v.x} ${v.y}) scale(${v.k})`);
  }

  #zoomMap(factor, cx, cy) {
    const cur = this.#mapView || this.#mapFit();
    if (!cur) return;
    if (cx == null) {
      const view = this.querySelector("[data-el=map-view]");
      cx = (view?.clientWidth || 0) / 2;
      cy = (view?.clientHeight || 0) / 2;
    }
    const k = Math.min(MAP_MAX_K, Math.max(MAP_MIN_K, cur.k * factor));
    const r = k / cur.k;
    this.#mapView = { k, x: cx - (cx - cur.x) * r, y: cy - (cy - cur.y) * r };
    this.#applyMapView();
  }

  #panMap(dx, dy) {
    const cur = this.#mapView || this.#mapFit();
    if (!cur) return;
    this.#mapView = { ...cur, x: cur.x + dx, y: cur.y + dy };
    this.#applyMapView();
  }

  #onMapWheel(ev) {
    const view = ev.target.closest?.("[data-el=map-view]");
    if (!view) return;
    ev.preventDefault();
    if (ev.ctrlKey || ev.metaKey) {
      const r = view.getBoundingClientRect();
      this.#zoomMap(Math.exp(-ev.deltaY * 0.01), ev.clientX - r.left, ev.clientY - r.top);
    } else {
      this.#panMap(-ev.deltaX, -ev.deltaY);
    }
  }

  #onPointerDown(ev) {
    if (ev.button !== 0) return;
    const split = ev.target.closest?.("[data-split]");
    if (split) {
      this.#dragSplit(ev, split);
      return;
    }
    const view = ev.target.closest?.("[data-el=map-view]");
    if (!view) return;
    let lastX = ev.clientX;
    let lastY = ev.clientY;
    let moved = false;
    this.#mapDragged = false;
    const onMove = (e) => {
      if (!moved && Math.hypot(e.clientX - ev.clientX, e.clientY - ev.clientY) < 4) return;
      if (!moved) {
        moved = true;
        view.classList.add("is-panning");
        try {
          view.setPointerCapture(ev.pointerId);
        } catch {
          /* ignore */
        }
      }
      this.#panMap(e.clientX - lastX, e.clientY - lastY);
      lastX = e.clientX;
      lastY = e.clientY;
    };
    const onUp = () => {
      view.removeEventListener("pointermove", onMove);
      view.removeEventListener("pointerup", onUp);
      view.removeEventListener("pointercancel", onUp);
      view.classList.remove("is-panning");
      // The click that follows a drag must not select a node.
      this.#mapDragged = moved;
    };
    view.addEventListener("pointermove", onMove);
    view.addEventListener("pointerup", onUp);
    view.addEventListener("pointercancel", onUp);
  }

  #dragSplit(ev, split) {
    const kind = split.getAttribute("data-split");
    const box = (kind === "side" ? this.querySelector(".ch-body") : this.querySelector(".ch-side"))
      ?.getBoundingClientRect();
    const mapTop = this.querySelector(".ch-map")?.getBoundingClientRect().top ?? box?.top;
    if (!box) return;
    ev.preventDefault();
    split.setPointerCapture(ev.pointerId);
    split.classList.add("is-dragging");
    let size = 0;
    const onMove = (e) => {
      if (kind === "side") {
        size = Math.round(Math.min(Math.max(e.clientX - box.left, 200), box.width - 240));
        this.style.setProperty("--ch-side-w", `${size}px`);
      } else {
        size = Math.round(Math.min(Math.max(e.clientY - mapTop, 120), box.bottom - mapTop - 90));
        this.style.setProperty("--ch-map-h", `${size}px`);
      }
      this.#applyMapView();
    };
    const onUp = () => {
      split.removeEventListener("pointermove", onMove);
      split.removeEventListener("pointerup", onUp);
      split.removeEventListener("pointercancel", onUp);
      split.classList.remove("is-dragging");
      if (size > 0) {
        const saved = prefs.get("changes-split", {}) || {};
        prefs.set("changes-split", { ...saved, [kind === "side" ? "side" : "map"]: size });
      }
    };
    split.addEventListener("pointermove", onMove);
    split.addEventListener("pointerup", onUp);
    split.addEventListener("pointercancel", onUp);
  }

  #onSplitReset(ev) {
    const kind = ev.target.closest?.("[data-split]")?.getAttribute("data-split");
    if (!kind) return;
    const key = kind === "side" ? "side" : "map";
    this.style.removeProperty(kind === "side" ? "--ch-side-w" : "--ch-map-h");
    const saved = { ...(prefs.get("changes-split", {}) || {}) };
    delete saved[key];
    prefs.set("changes-split", saved);
    this.#applyMapView();
  }


  #fileListHtml() {
    const files = this.#filesOrdered();
    const groups = new Map();
    for (const f of files) {
      const g = f.group || "(root)";
      if (!groups.has(g)) groups.set(g, []);
      groups.get(g).push(f);
    }
    const blocks = [];
    for (const [name, list] of groups) {
      blocks.push(`<div class="ch-group-name">${escapeText(name)}</div>`);
      for (const f of list) {
        const noteCount = notesFor(this.#review, f.path).length;
        const checked = isReviewed(this.#review, f) ? "checked" : "";
        const sel = f.path === this.#selected ? "is-selected" : "";
        const inCp = this.#fileInCheckpoint(f.path) ? "in-checkpoint" : "";
        blocks.push(`<div class="ch-file ${sel} ${inCp}" data-act="select" data-path="${escapeText(f.path)}">
          <span class="ch-badge" data-st="${escapeText(f.status)}">${statusLetter(f.status)}</span>
          <span class="ch-file-path">${escapeText(f.path)}</span>
          <span class="ch-counts">+${f.added || 0} −${f.removed || 0}</span>
          ${noteCount ? `<span class="ch-note-count">${noteCount}</span>` : ""}
          <label class="ch-rev" onclick="event.stopPropagation()">
            <input type="checkbox" data-act="toggle-rev" data-path="${escapeText(f.path)}" ${checked}/>
          </label>
        </div>`);
      }
    }
    return html`<div class="ch-files">${raw(blocks.join(""))}</div>`;
  }

  #overallHtml() {
    return html`<div class="ch-overall">
      <label>Overall note
        <textarea data-act="overall" rows="2">${this.#review.overall || ""}</textarea>
      </label>
    </div>`;
  }

  #fileInCheckpoint(path) {
    const ids = this.#checkpointHunkIds();
    if (!ids) return false;
    const file = (this.#cs?.files || []).find((f) => f.path === path);
    return !!(file?.hunks || []).some((h) => ids.has(h.id));
  }

  #diffHtml() {
    const ids = this.#checkpointHunkIds();
    if (ids) {
      const parts = [];
      for (const file of this.#cs?.files || []) {
        const hunks = (file.hunks || []).filter((h) => ids.has(h.id));
        if (!hunks.length) continue;
        parts.push(`<div class="ch-file-head sticky"><span>${escapeText(file.path)}</span></div>`);
        hunks.forEach((h, hi) => parts.push(this.#hunkHtml(file, h, hi)));
      }
      if (!parts.length) {
        return html`<div class="empty">No hunks in this checkpoint still exist.</div>`;
      }
      return html`<div class="ch-diff">${raw(parts.join(""))}</div>`;
    }
    const file = this.#selectedFile();
    if (!file) {
      return html`<div class="empty">Select a file.</div>`;
    }
    if (file.binary) {
      return html`<div class="ch-diff"><div class="ch-file-head sticky">${escapeText(file.path)}</div><p class="ch-bin">Binary file — not shown.</p></div>`;
    }
    if (this.#shouldShowMdView()) {
      const fileNotes = notesFor(this.#review, file.path).filter(
        (n) => n.start == null || n.end == null,
      );
      return html`<div class="ch-diff ch-md-wrap">
        <div class="ch-file-head sticky">
          <span>${escapeText(file.path)}</span>
          <button type="button" class="ghost sm" data-act="file-note">Note on this file</button>
        </div>
        ${raw(fileNotes.map((n) => this.#noteCard(n, true)).join(""))}
        <div class="ch-md-pane" data-md-pane><div class="empty">Rendering…</div></div>
      </div>`;
    }
    const fileNotes = notesFor(this.#review, file.path).filter(
      (n) => n.start == null || n.end == null,
    );
    const hunks = file.hunks || [];
    const hunkHtml = hunks
      .map((h, hi) => this.#hunkHtml(file, h, hi))
      .join("");
    return html`<div class="ch-diff">
      <div class="ch-file-head sticky">
        <span>${escapeText(file.path)}</span>
        <button type="button" class="ghost sm" data-act="file-note">Note on this file</button>
      </div>
      ${raw(fileNotes.map((n) => this.#noteCard(n, true)).join(""))}
      ${raw(hunkHtml)}
    </div>`;
  }

  #hunkHtml(file, hunk, hi) {
    const active = hi === this.#hunkIndex ? "is-active-hunk" : "";
    const lines =
      this.#view === "split"
        ? this.#splitHunk(file, hunk)
        : this.#unifiedHunk(file, hunk);
    return `<section class="ch-hunk ${active}" data-hunk="${hi}">
      <header class="ch-hunk-head">${escapeText(hunk.heading || hunk.id)}</header>
      <div class="ch-hunk-body">${lines}</div>
    </section>`;
  }

  #unifiedHunk(file, hunk) {
    const rows = unifiedRows(hunk);
    return rows
      .map((r) => {
        const side = r.kind === "remove" ? "old" : "new";
        const num = r.kind === "remove" ? r.old : r.new;
        const notes = notesFor(this.#review, file.path).filter(
          (n) => n.side === side && n.start != null && num != null && n.start <= num && n.end >= num,
        );
        const noteHtml = notes.map((n) => this.#noteCard(n, false)).join("");
        const mark = this.#marginMarkHtml(r.new);
        return `<div class="ch-line kind-${r.kind}">
          <button type="button" class="ch-ln" data-act="line-note" data-side="${side}" data-num="${num ?? ""}">${r.old ?? ""}</button>
          <button type="button" class="ch-ln" data-act="line-note" data-side="new" data-num="${r.new ?? ""}">${r.new ?? ""}</button>
          ${mark}
          <span class="ch-sign">${r.sign}</span>
          <code class="ch-text">${escapeText(r.text)}</code>
        </div>${noteHtml}`;
      })
      .join("");
  }

  #splitHunk(file, hunk) {
    return splitRows(hunk)
      .map((row) => {
        const left = row.left
          ? `<div class="ch-split-cell kind-${row.left.kind}"><button type="button" class="ch-ln" data-act="line-note" data-side="old" data-num="${row.left.old ?? ""}">${row.left.old ?? ""}</button><span class="ch-sign">${row.left.sign}</span><code>${escapeText(row.left.text)}</code></div>`
          : `<div class="ch-split-cell is-empty"></div>`;
        const mark = this.#marginMarkHtml(row.right?.new);
        const right = row.right
          ? `<div class="ch-split-cell kind-${row.right.kind}"><button type="button" class="ch-ln" data-act="line-note" data-side="new" data-num="${row.right.new ?? ""}">${row.right.new ?? ""}</button>${mark}<span class="ch-sign">${row.right.sign}</span><code>${escapeText(row.right.text)}</code></div>`
          : `<div class="ch-split-cell is-empty"></div>`;
        return `<div class="ch-split-row">${left}${right}</div>`;
      })
      .join("");
  }

  #noteIsOutdated(note) {
    if (note?.start == null) return false;
    if (this.#shouldShowMdView() && this.#mdText) {
      if (!note.quote) return false;
      return (
        relocateMargin(this.#mdText, {
          start: note.start,
          end: note.end,
          quote: note.quote,
        }).status === "outdated"
      );
    }
    const file = this.#selectedFile();
    if (!file) return false;
    return !(file.hunks || []).some((h) =>
      (h.lines || []).some((l) => {
        const n = note.side === "old" ? l.old : l.new;
        return n != null && n >= note.start && n <= note.end;
      }),
    );
  }

  #noteCard(note, fileLevel) {
    const outdated = this.#noteIsOutdated(note);
    const lines =
      note.start != null
        ? note.start === note.end
          ? `line ${note.start}`
          : `lines ${note.start}–${note.end}`
        : "";
    return `<div class="ch-note ${note.sent ? "is-sent" : ""} ${outdated ? "is-outdated" : ""}" data-note="${escapeText(note.id)}">
      ${outdated ? `<div class="ch-outdated">outdated</div>` : ""}
      ${fileLevel ? `<div class="ch-note-label">File note</div>` : ""}
      ${!fileLevel && lines ? `<div class="ch-note-label">${escapeText(lines)}</div>` : ""}
      <div class="ch-note-text md-body md-body-compact" data-note-md="${escapeText(note.id)}">${escapeText(note.text)}</div>
      <div class="ch-note-actions">
        <button type="button" class="ghost sm" data-act="edit-note" data-id="${escapeText(note.id)}">Edit</button>
        <button type="button" class="ghost sm" data-act="del-note" data-id="${escapeText(note.id)}">Delete</button>
        ${note.sent ? `<span class="ch-sent">sent</span>` : ""}
      </div>
    </div>`;
  }

  async #hydrateNoteMarkdown() {
    const nodes = [...this.querySelectorAll("[data-note-md]")];
    if (!nodes.length) return;
    const byId = new Map((this.#review?.notes || []).map((n) => [n.id, n]));
    await Promise.all(
      nodes.map(async (el) => {
        const id = el.getAttribute("data-note-md");
        const note = byId.get(id);
        if (!note?.text) return;
        try {
          const data = await api.post("/markdown", { text: note.text });
          const markup = html`${fromServerHtml(data.html || "")}`;
          el.innerHTML = markup.value;
        } catch (_) {
          /* leave escaped plain text */
        }
      }),
    );
  }

  async #onClick(ev) {
    const t = ev.target.closest("[data-act]");
    if (!t) {
      if (ev.target.matches?.("textarea[data-act=overall]")) return;
      return;
    }
    const act = t.getAttribute("data-act");
    if (act === "refresh") {
      this.#mdHtml = "";
      this.#mdText = "";
      this.#mdError = null;
      this.#mdPath = null;
      this.#turnHint = false;
      this.#load();
      return;
    }
    if (act === "view") {
      this.#view = t.getAttribute("data-v");
      prefs.set("changes", { view: this.#view });
      this.#paint();
      return;
    }
    if (act === "md-mode") {
      const v = t.getAttribute("data-v");
      if (v !== "view" && v !== "diff") return;
      this.#mdMode = v;
      prefs.set("changes-md-mode", v);
      this.#mdHtml = "";
      this.#mdText = "";
      this.#mdError = null;
      this.#mdPath = null;
      this.#paint();
      return;
    }
    if (act === "sort") {
      this.#sort = t.getAttribute("data-v");
      prefs.set("changes-sort", this.#sort);
      this.#paint();
      return;
    }
    if (act === "toggle-map") {
      this.#mapOpen = !this.#mapOpen;
      prefs.set("changes-map-open", this.#mapOpen);
      this.#paint();
      return;
    }
    if (act === "map-zoom") {
      this.#zoomMap(t.getAttribute("data-v") === "in" ? 1.25 : 0.8);
      return;
    }
    if (act === "map-fit") {
      this.#mapView = null;
      this.#applyMapView();
      return;
    }
    if (act === "select" && this.#mapDragged && t.closest("[data-el=map-view]")) {
      this.#mapDragged = false;
      return;
    }
    if (act === "select") {
      this.#selected = t.getAttribute("data-path");
      this.#hunkIndex = 0;
      this.#mdHtml = "";
      this.#mdText = "";
      this.#mdError = null;
      this.#mdPath = null;
      this.#margins = [];
      this.#marginsPath = null;
      this.#marginPeek = null;
      this.#paint();
      return;
    }
    if (act === "open-margin") {
      const ids = (t.getAttribute("data-ids") || "").split(",").filter(Boolean);
      const num = Number(t.getAttribute("data-num"));
      const file = this.#selectedFile();
      if (!ids.length) return;
      if (ids.length === 1 && !ev.shiftKey) {
        this.#openMarginPeek(ids[0]);
        return;
      }
      const items = [
        ...ids.map((id, i) => ({
          id: `open:${id}`,
          label: ids.length === 1 ? "Open margin" : `Open ${i + 1}`,
        })),
        ...(num && file ? [{ id: `add:${num}`, label: "Add another" }] : []),
      ];
      openMenuBelow(t, items, (pick) => {
        if (pick.startsWith("open:")) this.#openMarginPeek(pick.slice(5));
        if (pick.startsWith("add:")) {
          const line = Number(pick.slice(4));
          if (file && line) {
            this.#marginLineDraft = { path: file.path, start: line, end: line };
            void this.#addMarginOnFile(t, line, line);
          }
        }
      });
      return;
    }
    if (act === "close-margin") {
      this.#marginPeek = null;
      this.#paint();
      return;
    }
    if (act === "add-margin-line") {
      const num = Number(t.getAttribute("data-num"));
      if (!num) return;
      const file = this.#selectedFile();
      if (!file) return;
      let start = num;
      let end = num;
      if (ev.shiftKey && this.#marginLineDraft?.path === file.path) {
        start = Math.min(this.#marginLineDraft.start, num);
        end = Math.max(this.#marginLineDraft.end, num);
      }
      this.#marginLineDraft = { path: file.path, start, end };
      void this.#addMarginOnFile(t, start, end);
      return;
    }
    if (act === "toggle-rev") {
      const path = t.getAttribute("data-path");
      const file = (this.#cs?.files || []).find((f) => f.path === path);
      if (file) {
        this.#review = toggleReviewed(this.#review, file);
        this.#scheduleSave();
        this.#paint();
      }
      return;
    }
    if (act === "line-note" || act === "md-line-note") {
      const side = act === "md-line-note" ? "new" : t.getAttribute("data-side");
      const num = Number(t.getAttribute("data-num"));
      if (!num) return;
      const file = this.#selectedFile();
      if (!file) return;
      let start = num;
      let end = num;
      if (ev.shiftKey && this.#noteDraft?.path === file.path && this.#noteDraft.side === side) {
        start = Math.min(this.#noteDraft.start, num);
        end = Math.max(this.#noteDraft.end, num);
      }
      const quote =
        act === "md-line-note"
          ? quoteLines(this.#mdText, start, end)
          : this.#quoteLines(file, side, start, end);
      const text = await askTextNear(t, {
        title: start === end ? `Line ${start}` : `Lines ${start}–${end}`,
        placeholder: "Note",
        multiline: true,
        rows: 3,
        confirmLabel: "Add note",
        requiredMessage: "Write a note first.",
      });
      if (text == null || !text.trim()) return;
      const note = {
        id: uuid(),
        path: file.path,
        side,
        start,
        end,
        quote,
        text: text.trim(),
        sent: false,
        created_at: Date.now(),
      };
      this.#noteDraft = note;
      this.#review = addNote(this.#review, note);
      this.#scheduleSave();
      this.#paint();
      return;
    }
    if (act === "file-note") {
      const file = this.#selectedFile();
      if (!file) return;
      const text = await askTextNear(t, {
        title: "Note on this file",
        placeholder: "Note",
        multiline: true,
        rows: 3,
        confirmLabel: "Add note",
        requiredMessage: "Write a note first.",
      });
      if (text == null || !text.trim()) return;
      this.#review = addNote(this.#review, {
        id: uuid(),
        path: file.path,
        side: "new",
        start: null,
        end: null,
        quote: "",
        text: text.trim(),
        sent: false,
        created_at: Date.now(),
      });
      this.#scheduleSave();
      this.#paint();
      return;
    }
    if (act === "del-note") {
      this.#review = removeNote(this.#review, t.getAttribute("data-id"));
      this.#scheduleSave();
      this.#paint();
      return;
    }
    if (act === "edit-note") {
      const id = t.getAttribute("data-id");
      const note = (this.#review.notes || []).find((n) => n.id === id);
      if (!note) return;
      const text = await askTextNear(t, {
        title: "Edit note",
        value: note.text || "",
        multiline: true,
        rows: 3,
        confirmLabel: "Save",
        requiredMessage: "Write a note first.",
      });
      if (text == null) return;
      this.#review = editNote(this.#review, id, { text: text.trim() });
      this.#scheduleSave();
      this.#paint();
      return;
    }
    if (act === "send") {
      this.#sendNotes();
      return;
    }
    if (act === "plan") {
      this.#runPlan();
      return;
    }
    if (act === "plan-mode") {
      this.#planMode = true;
      this.#checkpointIndex = 0;
      this.#paint();
      return;
    }
    if (act === "plan-all") {
      this.#planMode = false;
      this.#paint();
      return;
    }
    if (act === "cp-prev") {
      this.#checkpointIndex = Math.max(0, this.#checkpointIndex - 1);
      this.#paint();
      return;
    }
    if (act === "cp-next") {
      const n = (this.#review?.plan?.checkpoints || []).length;
      this.#checkpointIndex = Math.min(n - 1, this.#checkpointIndex + 1);
      this.#paint();
    }
  }

  #onInput(ev) {
    if (ev.target.matches?.("textarea[data-act=overall]")) {
      this.#review = { ...this.#review, overall: ev.target.value };
      this.#scheduleSave();
    }
    if (ev.target.matches?.("select[data-act=plan-agent]")) {
      this.#planAgent = ev.target.value;
    }
    if (ev.target.matches?.("input[data-act=plan-model]")) {
      this.#planModel = ev.target.value;
    }
  }

  async #runPlan() {
    if (this.#planning) return;
    this.#planning = true;
    this.#planError = null;
    this.#planSeconds = 0;
    clearInterval(this.#planTimer);
    this.#planTimer = setInterval(() => {
      this.#planSeconds += 1;
      this.#paint();
    }, 1000);
    this.#paint();
    try {
      const planPath =
        this.source.kind === "pull"
          ? `${this.#apiBase()}/plan`
          : `${this.#apiBase()}/changes/plan`;
      const plan = await api.post(planPath, {
        agent: this.#planAgent,
        model: this.#planModel || null,
      });
      this.#review = { ...this.#review, plan };
      this.#planMode = true;
      this.#checkpointIndex = 0;
    } catch (err) {
      this.#planError = err instanceof ApiError ? err.message : "Planning failed.";
    } finally {
      clearInterval(this.#planTimer);
      this.#planTimer = null;
      this.#planning = false;
      this.#paint();
    }
  }

  #quoteLines(file, side, start, end) {
    const lines = [];
    for (const h of file.hunks || []) {
      for (const l of h.lines || []) {
        const n = side === "old" ? l.old : l.new;
        if (n != null && n >= start && n <= end) lines.push(l.text);
      }
    }
    return lines.join("\n");
  }

  async #sendNotes() {
    const notes = unsent(this.#review);
    const text = composeMessage(notes, this.#review.overall);
    if (!text.trim()) return;
    try {
      await api.post(`/sessions/${this.source.id}/send`, { text, submit: true });
      this.#review = markSent(
        this.#review,
        notes.map((n) => n.id),
      );
      this.#scheduleSave();
      this.#paint();
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : "Could not send notes.", {
        kind: "error",
      });
    }
  }

  #onKey(ev) {
    if (ev.target.closest("input, textarea, select")) return;
    const files = this.#filesOrdered();
    const file = this.#selectedFile();
    const idx = files.findIndex((f) => f.path === this.#selected);
    if (ev.key === "j" || ev.key === "J") {
      ev.preventDefault();
      if (ev.shiftKey) {
        if (idx < files.length - 1) {
          this.#selected = files[idx + 1].path;
          this.#hunkIndex = 0;
          this.#paint();
        }
      } else if (file) {
        this.#hunkIndex = Math.min((file.hunks || []).length - 1, this.#hunkIndex + 1);
        this.#paint();
      }
    } else if (ev.key === "k" || ev.key === "K") {
      ev.preventDefault();
      if (ev.shiftKey) {
        if (idx > 0) {
          this.#selected = files[idx - 1].path;
          this.#hunkIndex = 0;
          this.#paint();
        }
      } else if (file) {
        this.#hunkIndex = Math.max(0, this.#hunkIndex - 1);
        this.#paint();
      }
    } else if (ev.key === "r" || ev.key === "R") {
      if (file) {
        this.#review = toggleReviewed(this.#review, file);
        this.#scheduleSave();
        this.#paint();
      }
    } else if (ev.key === "n" || ev.key === "N") {
      if (!file) return;
      const hunk = (file.hunks || [])[this.#hunkIndex];
      const line = (hunk?.lines || []).find((l) => l.kind === "add" || l.kind === "remove");
      if (!line) return;
      const side = line.kind === "remove" ? "old" : "new";
      const num = side === "old" ? line.old : line.new;
      const btn = this.querySelector(
        `[data-act="line-note"][data-side="${side}"][data-num="${num}"]`,
      );
      btn?.click();
    }
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-changes-view")) {
  customElements.define("dync-changes-view", DyncChangesView);
}
