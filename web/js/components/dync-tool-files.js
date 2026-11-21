import { DyncElement, html, escapeText } from "../lib/element.js";
import { api, ApiError } from "../lib/api.js";
import { store } from "../lib/store.js";
import * as prefs from "../lib/prefs.js";
import {
  createTreeState,
  flatten,
  languageFor,
  isMarkdownPath,
  normalizeNewFilePath,
  parentDir,
} from "../lib/filetree.js";
import { fromServerHtml } from "../lib/markdown.js";
import { quoteLines, relocateAll } from "../lib/margins.js";
import { createMarginGutter } from "../lib/margin-gutter.js";
import { askConfirm, askTextNear } from "../lib/prompt.js";
import { showToast } from "../lib/toast.js";
import {
  EditorView,
  EditorState,
  Compartment,
  basicSetup,
  keymap,
  indentWithTab,
  HighlightStyle,
  syntaxHighlighting,
  tags,
  javascript,
  rust,
  python,
  json,
  markdown,
  html as htmlLang,
  css,
  yaml,
  go,
  shell,
  toml,
} from "/vendor/codemirror/codemirror.js";

function uuid() {
  if (globalThis.crypto?.randomUUID) return crypto.randomUUID();
  return `m-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

const LANGS = {
  javascript,
  rust,
  python,
  json,
  markdown,
  html: htmlLang,
  css,
  yaml,
  go,
  shell,
  toml,
};

const CACHE_MAX = 10;
const MIN_PANE = 160;

function setupExt() {
  return typeof basicSetup === "function" ? basicSetup() : basicSetup;
}

function buildTheme() {
  const cssVar = (name) =>
    getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  const text = cssVar("--text") || "#e8e8ed";
  const muted = cssVar("--text-muted") || "#9a9aad";
  const ok = cssVar("--ok") || "#3ecf8e";
  const warn = cssVar("--warn") || "#e6a23c";
  const accentInk = cssVar("--accent-ink") || cssVar("--accent") || "#a78bfa";
  const accent = cssVar("--accent") || "#a78bfa";

  const theme = EditorView.theme(
    {
      "&": {
        backgroundColor: "transparent",
        color: text,
        fontFamily: "var(--font-mono)",
        fontSize: "13px",
      },
      ".cm-content": { caretColor: accentInk },
      "&.cm-focused .cm-cursor": { borderLeftColor: accentInk },
      "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
        {
          backgroundColor: `color-mix(in srgb, ${accent} 30%, transparent)`,
        },
      ".cm-gutters": {
        backgroundColor: "transparent",
        color: muted,
        border: "none",
      },
      ".cm-activeLineGutter": { backgroundColor: "transparent" },
    },
    { dark: document.documentElement.getAttribute("data-appearance") !== "day" },
  );

  const highlight = HighlightStyle.define([
    { tag: tags.comment, color: muted },
    { tag: tags.lineComment, color: muted },
    { tag: tags.blockComment, color: muted },
    { tag: tags.string, color: ok },
    { tag: tags.special(tags.string), color: ok },
    { tag: tags.keyword, color: accentInk },
    { tag: tags.controlKeyword, color: accentInk },
    { tag: tags.operatorKeyword, color: accentInk },
    { tag: tags.number, color: warn },
    { tag: tags.bool, color: warn },
    { tag: tags.atom, color: text },
    { tag: tags.name, color: text },
    { tag: tags.variableName, color: text },
    { tag: tags.propertyName, color: text },
    { tag: tags.definition(tags.variableName), color: text },
    { tag: tags.typeName, color: accentInk },
    { tag: tags.className, color: accentInk },
    { tag: tags.function(tags.variableName), color: text },
    { tag: tags.meta, color: muted },
    { tag: tags.punctuation, color: text },
    { tag: tags.operator, color: text },
  ]);

  return [theme, syntaxHighlighting(highlight)];
}

export class DyncToolFiles extends DyncElement {
  static get observedAttributes() {
    return ["session"];
  }

  #tree = createTreeState();
  #view = null;
  #themeComp = new Compartment();
  #langComp = new Compartment();
  #cache = new Map();
  #openPath = null;
  #revision = null;
  #dirty = false;
  #stale = false;
  #openError = null;
  #filter = "";
  #findPaths = [];
  #findTimer = null;
  #selected = "";
  #split = 260;
  #prevState = null;
  #built = false;
  /** @type {"view"|"source"} */
  #mdMode = "view";
  #mdHtml = "";
  #mdError = null;
  #mdRenderToken = 0;
  #margins = [];
  #marginOpen = null;
  #marginDraft = null;
  #marginMode = "view";
  #marginGutter = createMarginGutter((entries) => this.#openMarginEntries(entries));

  mount() {
    this.classList.add("tool", "tool-files");
    this.#split = Number(prefs.get("files-split", 260)) || 260;
    this.#mdMode = prefs.get("files-md-mode", "view") === "source" ? "source" : "view";
    this.style.setProperty("--files-tree-w", `${this.#split}px`);
    this.render(
      html`<div class="files-split">
        <div class="files-tree-pane" data-tree-pane>
          <div class="files-filter">
            <input type="search" placeholder="Filter files…" data-filter aria-label="Filter files" />
            <button type="button" class="ghost sm" data-new-file title="Create a new file in the workspace">New file</button>
          </div>
          <div class="files-tree" role="tree" tabindex="0" data-tree hidden></div>
          <div class="files-find" role="listbox" tabindex="0" data-find hidden></div>
        </div>
        <div class="files-gutter" data-gutter title="Resize"></div>
        <div class="files-editor-pane">
          <div class="files-editor-header">
            <span class="files-path" data-path></span>
            <span class="files-dirty" data-dirty hidden></span>
            <span class="files-mode" data-md-mode hidden role="group" aria-label="Markdown mode">
              <button type="button" class="ghost sm" data-md="view">View</button>
              <button type="button" class="ghost sm" data-md="source">Source</button>
            </span>
            <button type="button" class="ghost sm" data-add-margin hidden title="Add a margin on the current line">Add margin</button>
            <button type="button" class="ghost sm" data-reload>Reload</button>
            <button type="button" class="sm" data-save>Save</button>
          </div>
          <div class="files-stale" data-stale hidden>
            <span>This file changed on disk.</span>
            <button type="button" class="ghost sm" data-reload-theirs>Reload theirs</button>
            <button type="button" class="sm" data-overwrite>Overwrite with mine</button>
          </div>
          <div class="files-empty" data-empty>Open a file from the tree.</div>
          <div class="files-error" data-error hidden></div>
          <div class="files-main">
            <div class="files-md-view" data-md-view hidden></div>
            <div class="files-cm" data-cm hidden></div>
            <aside class="files-margin-pane" data-margin-pane hidden></aside>
          </div>
        </div>
      </div>`,
    );

    this.#built = true;
    this.#bind();
    this.#loadRoot();
    this.#paintChrome();
  }

  unmount() {
    this.#teardownUnload();
    if (this.#view) {
      this.#stashOpen();
      this.#view.destroy();
      this.#view = null;
    }
  }

  attributeChangedCallback() {
    if (!this.#built) return;
    void this.#resetForSession();
  }

  #sessionId() {
    return this.getAttribute("session") || "";
  }

  /** Used by the workspace before leaving the session view. */
  hasUnsaved() {
    return this.#dirty;
  }

  #bind() {
    this.on(this.querySelector("[data-filter]"), "input", (ev) => {
      this.#filter = ev.target.value.trim();
      this.#onFilter();
    });
    this.on(this.querySelector("[data-new-file]"), "click", (ev) => {
      void this.#newFile(ev.currentTarget);
    });
    this.on(this.querySelector("[data-tree]"), "keydown", (ev) => this.#onTreeKey(ev));
    this.on(this.querySelector("[data-tree]"), "click", (ev) => this.#onTreeClick(ev));
    this.on(this.querySelector("[data-find]"), "keydown", (ev) => this.#onFindKey(ev));
    this.on(this.querySelector("[data-find]"), "click", (ev) => this.#onFindClick(ev));
    this.on(this.querySelector("[data-save]"), "click", () => this.#save(false));
    this.on(this.querySelector("[data-reload]"), "click", () => this.#reloadOpen(false));
    this.on(this.querySelector("[data-reload-theirs]"), "click", () => this.#reloadOpen(true));
    this.on(this.querySelector("[data-overwrite]"), "click", () => this.#save(true));
    this.on(this.querySelector("[data-md-mode]"), "click", (ev) => {
      const btn = ev.target.closest("[data-md]");
      if (!btn) return;
      const mode = btn.getAttribute("data-md");
      if (mode !== "view" && mode !== "source") return;
      this.#mdMode = mode;
      prefs.set("files-md-mode", mode);
      this.#paintChrome();
      if (mode === "view") this.#renderMarkdown();
    });
    this.on(this.querySelector("[data-add-margin]"), "click", () => this.#addMarginAtCursor());
    this.on(this.querySelector("[data-margin-pane]"), "click", (ev) => this.#onMarginPaneClick(ev));
    this.on(this.querySelector("[data-md-view]"), "click", (ev) => this.#onMdLinkClick(ev));
    this.on(this.querySelector("[data-gutter]"), "pointerdown", (ev) => this.#startSplit(ev));
    this.on(window, "keydown", (ev) => {
      if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === "s") {
        if (!this.isConnected || !this.#openPath) return;
        ev.preventDefault();
        this.#save(false);
      }
    });
    this.on(window, "dync-appearance", () => this.#retheme());
    this.on(window, "dync-theme", () => this.#retheme());

    let lastState = store.sessions.get(this.#sessionId())?.state;
    const unsub = store.subscribe(() => {
      const s = store.sessions.get(this.#sessionId());
      const next = s?.state;
      if (lastState === "working" && next === "ready") {
        this.#onTurnEnded();
      }
      lastState = next;
    });
    this.cleanup(unsub);
  }

  async #resetForSession() {
    if (
      this.#dirty &&
      !(await askConfirm({
        title: "Unsaved edits",
        message: "Leave without saving?",
        confirmLabel: "Leave",
        danger: true,
      }))
    ) {
      return;
    }
    this.#stashOpen();
    this.#tree = createTreeState();
    this.#cache.clear();
    this.#openPath = null;
    this.#revision = null;
    this.#dirty = false;
    this.#stale = false;
    this.#openError = null;
    this.#selected = "";
    this.#margins = [];
    this.#marginDraft = null;
    this.#marginOpen = null;
    if (this.#view) {
      this.#view.destroy();
      this.#view = null;
    }
    this.#paintMarginPane();
    this.#paintChrome();
    this.#loadRoot();
  }

  async #loadRoot() {
    const id = this.#sessionId();
    if (!id) return;
    try {
      const data = await api.get(`/sessions/${id}/tree?path=`);
      this.#tree.children.set("", data.entries || []);
      this.#paintTree();
    } catch (err) {
      this.#openError = err.message || "Could not load the tree.";
      this.#paintChrome();
    }
  }

  #suggestedNewDir() {
    const sel = this.#selected;
    if (!sel) return "";
    const row = flatten(this.#tree).find((r) => r.path === sel);
    if (row?.kind === "dir") return sel;
    return parentDir(sel);
  }

  async #newFile(anchor) {
    const id = this.#sessionId();
    if (!id) return;
    const dir = this.#suggestedNewDir();
    const typed = await askTextNear(anchor, {
      title: "New file",
      label: "Path in the workspace",
      value: dir ? `${dir}/` : "",
      placeholder: dir ? `${dir}/name.rs` : "src/name.rs",
      confirmLabel: "Create",
      requiredMessage: "Enter a file path.",
    });
    if (typed == null) return;
    const path = normalizeNewFilePath(typed);
    if (!path) {
      showToast("That path is not allowed.", { kind: "error" });
      return;
    }
    try {
      await api.put(`/sessions/${id}/file`, {
        path,
        text: "",
        revision: "",
        force: false,
      });
      this.#cache.delete(path);
      await this.#revealInTree(path);
      await this.#openFile(path);
      showToast(`Created ${path}.`, { kind: "ok" });
    } catch (err) {
      const msg =
        err instanceof ApiError && err.code === "stale"
          ? "That file already exists."
          : err.message || "Could not create that file.";
      showToast(msg, { kind: "error" });
    }
  }

  async #revealInTree(filePath) {
    const folders = [];
    let cur = "";
    for (const seg of filePath.split("/").slice(0, -1)) {
      cur = cur ? `${cur}/${seg}` : seg;
      folders.push(cur);
    }
    this.#tree.children.delete("");
    try {
      await this.#ensureChildren("");
    } catch (_) {
      /* paint what we can */
    }
    for (const folder of folders) {
      this.#tree.expanded.add(folder);
      this.#tree.children.delete(folder);
      try {
        await this.#ensureChildren(folder);
      } catch (_) {
        break;
      }
    }
    this.#selected = filePath;
    this.#filter = "";
    this.#findPaths = [];
    const filterEl = this.querySelector("[data-filter]");
    if (filterEl) filterEl.value = "";
    this.#paintTree();
  }

  async #ensureChildren(path) {
    if (this.#tree.children.has(path)) return;
    const id = this.#sessionId();
    const q = path ? `?path=${encodeURIComponent(path)}` : "?path=";
    const data = await api.get(`/sessions/${id}/tree${q}`);
    this.#tree.children.set(path, data.entries || []);
  }

  #paintTree() {
    const treeEl = this.querySelector("[data-tree]");
    const findEl = this.querySelector("[data-find]");
    if (!treeEl || !findEl) return;
    if (this.#filter) {
      treeEl.hidden = true;
      findEl.hidden = false;
      findEl.innerHTML = this.#findPaths
        .map(
          (p) =>
            `<button type="button" class="ff-row" role="option" data-path="${escapeText(p)}" aria-selected="${p === this.#selected}">${escapeText(p)}</button>`,
        )
        .join("");
      return;
    }
    findEl.hidden = true;
    treeEl.hidden = false;
    const rows = flatten(this.#tree);
    treeEl.innerHTML = rows
      .map((r) => {
        const pad = 0.55 + r.depth * 0.85;
        const twist =
          r.kind === "dir" ? (r.expanded ? "▾" : "▸") : "";
        const sel = r.path === this.#selected ? "true" : "false";
        const expandedAttr =
          r.kind === "dir" ? ` aria-expanded="${r.expanded}"` : "";
        return `<button type="button" class="ft-row${r.ignored ? " is-ignored" : ""}" role="treeitem" aria-selected="${sel}"${expandedAttr} data-path="${escapeText(r.path)}" data-kind="${escapeText(r.kind)}" style="padding-left:${pad}rem"><span class="ft-twist">${twist}</span><span class="ft-name">${escapeText(r.name)}</span></button>`;
      })
      .join("");
  }

  #onFilter() {
    clearTimeout(this.#findTimer);
    if (!this.#filter) {
      this.#findPaths = [];
      this.#paintTree();
      return;
    }
    this.#findTimer = setTimeout(() => this.#runFind(), 200);
  }

  async #runFind() {
    const id = this.#sessionId();
    if (!id || !this.#filter) return;
    try {
      const data = await api.get(
        `/sessions/${id}/find?q=${encodeURIComponent(this.#filter)}`,
      );
      this.#findPaths = data.paths || [];
      this.#paintTree();
    } catch (_) {
      this.#findPaths = [];
      this.#paintTree();
    }
  }

  async #onTreeClick(ev) {
    const row = ev.target.closest("[data-path]");
    if (!row) return;
    const path = row.getAttribute("data-path");
    const kind = row.getAttribute("data-kind");
    this.#selected = path;
    if (kind === "dir") {
      await this.#toggleDir(path);
    } else if (kind === "file") {
      await this.#openFile(path);
    } else {
      this.#paintTree();
    }
  }

  async #toggleDir(path) {
    if (this.#tree.expanded.has(path)) {
      this.#tree.expanded.delete(path);
    } else {
      try {
        await this.#ensureChildren(path);
        this.#tree.expanded.add(path);
      } catch (err) {
        this.#openError = err.message || "Could not open that folder.";
        this.#paintChrome();
        return;
      }
    }
    this.#paintTree();
  }

  async #onTreeKey(ev) {
    const rows = [...this.querySelectorAll("[data-tree] .ft-row")];
    if (!rows.length) return;
    let idx = rows.findIndex((r) => r.getAttribute("data-path") === this.#selected);
    if (idx < 0) idx = 0;
    if (ev.key === "ArrowDown") {
      ev.preventDefault();
      idx = Math.min(rows.length - 1, idx + 1);
      this.#selected = rows[idx].getAttribute("data-path");
      this.#paintTree();
      rows[idx]?.focus?.();
    } else if (ev.key === "ArrowUp") {
      ev.preventDefault();
      idx = Math.max(0, idx - 1);
      this.#selected = rows[idx].getAttribute("data-path");
      this.#paintTree();
    } else if (ev.key === "ArrowRight") {
      ev.preventDefault();
      const row = rows[idx];
      if (row?.getAttribute("data-kind") === "dir") {
        const path = row.getAttribute("data-path");
        if (!this.#tree.expanded.has(path)) await this.#toggleDir(path);
      }
    } else if (ev.key === "ArrowLeft") {
      ev.preventDefault();
      const row = rows[idx];
      const path = row?.getAttribute("data-path");
      if (row?.getAttribute("data-kind") === "dir" && this.#tree.expanded.has(path)) {
        await this.#toggleDir(path);
      } else if (path && path.includes("/")) {
        this.#selected = path.split("/").slice(0, -1).join("/");
        this.#paintTree();
      }
    } else if (ev.key === "Enter") {
      ev.preventDefault();
      const row = rows[idx];
      if (!row) return;
      const path = row.getAttribute("data-path");
      const kind = row.getAttribute("data-kind");
      if (kind === "dir") await this.#toggleDir(path);
      else if (kind === "file") await this.#openFile(path);
    }
  }

  #onFindClick(ev) {
    const row = ev.target.closest("[data-path]");
    if (!row) return;
    const path = row.getAttribute("data-path");
    this.#selected = path;
    this.#openFile(path);
  }

  #onFindKey(ev) {
    const rows = [...this.querySelectorAll("[data-find] .ff-row")];
    if (!rows.length) return;
    let idx = rows.findIndex((r) => r.getAttribute("data-path") === this.#selected);
    if (idx < 0) idx = 0;
    if (ev.key === "ArrowDown") {
      ev.preventDefault();
      idx = Math.min(rows.length - 1, idx + 1);
      this.#selected = rows[idx].getAttribute("data-path");
      this.#paintTree();
    } else if (ev.key === "ArrowUp") {
      ev.preventDefault();
      idx = Math.max(0, idx - 1);
      this.#selected = rows[idx].getAttribute("data-path");
      this.#paintTree();
    } else if (ev.key === "Enter") {
      ev.preventDefault();
      const path = rows[idx]?.getAttribute("data-path");
      if (path) this.#openFile(path);
    }
  }

  #stashOpen() {
    if (!this.#openPath || !this.#view) return;
    const scroll = this.#view.scrollDOM.scrollTop;
    const selection = this.#view.state.selection;
    const text = this.#view.state.doc.toString();
    this.#cache.set(this.#openPath, {
      text: this.#dirty ? this.#cache.get(this.#openPath)?.text ?? text : text,
      revision: this.#revision,
      unsaved: this.#dirty ? text : null,
      scroll,
      selection: { anchor: selection.main.anchor, head: selection.main.head },
      dirty: this.#dirty,
      stale: this.#stale,
    });
    while (this.#cache.size > CACHE_MAX) {
      const first = this.#cache.keys().next().value;
      this.#cache.delete(first);
    }
  }

  async #openFile(path) {
    if (this.#openPath === path && this.#view && !this.#openError) {
      this.#paintChrome();
      return;
    }
    if (this.#dirty && this.#openPath && this.#openPath !== path) {
      if (
        !(await askConfirm({
          title: "Unsaved edits",
          message: "Leave without saving?",
          confirmLabel: "Leave",
          danger: true,
        }))
      ) {
        return;
      }
    }
    this.#stashOpen();
    this.#openPath = path;
    this.#selected = path;
    this.#openError = null;
    this.#stale = false;
    this.#dirty = false;

    const cached = this.#cache.get(path);
    if (cached) {
      this.#revision = cached.revision;
      this.#dirty = !!cached.dirty;
      this.#stale = !!cached.stale;
      this.#showEditor(cached.unsaved ?? cached.text, path, cached);
      this.#paintChrome();
      this.#paintTree();
      return;
    }

    const id = this.#sessionId();
    try {
      const data = await api.get(
        `/sessions/${id}/file?path=${encodeURIComponent(path)}`,
      );
      this.#revision = data.revision;
      this.#cache.set(path, {
        text: data.text,
        revision: data.revision,
        unsaved: null,
        scroll: 0,
        selection: { anchor: 0, head: 0 },
        dirty: false,
        stale: false,
      });
      this.#showEditor(data.text, path, null);
    } catch (err) {
      this.#openError = err.message || "Can't open this here.";
      if (this.#view) {
        this.#view.destroy();
        this.#view = null;
      }
    }
    this.#paintChrome();
    this.#paintTree();
  }

  #showEditor(text, path, cached) {
    const host = this.querySelector("[data-cm]");
    if (!host) return;
    const langId = languageFor(path);
    const langExt = langId && LANGS[langId] ? LANGS[langId]() : [];
    const extensions = [
      setupExt(),
      keymap.of([indentWithTab]),
      this.#themeComp.of(buildTheme()),
      this.#langComp.of(langExt),
      ...this.#marginGutter.extensions,
      EditorView.updateListener.of((u) => {
        if (u.docChanged) {
          this.#dirty = true;
          this.#paintChrome();
          this.#setupUnload();
        }
      }),
    ];
    if (this.#view) {
      this.#view.setState(
        EditorState.create({
          doc: text,
          extensions,
          selection: cached?.selection
            ? { anchor: cached.selection.anchor, head: cached.selection.head }
            : undefined,
        }),
      );
    } else {
      this.#view = new EditorView({
        parent: host,
        state: EditorState.create({
          doc: text,
          extensions,
          selection: cached?.selection
            ? { anchor: cached.selection.anchor, head: cached.selection.head }
            : undefined,
        }),
      });
    }
    if (cached?.scroll != null) {
      requestAnimationFrame(() => {
        if (this.#view) this.#view.scrollDOM.scrollTop = cached.scroll;
      });
    }
    if (isMarkdownPath(path) && this.#mdMode === "view") {
      this.#renderMarkdown();
    }
    this.#loadMarginsFor(path);
  }

  async #loadMarginsFor(path) {
    const id = this.#sessionId();
    if (!id || !path) {
      this.#margins = [];
      this.#applyMarginMarks();
      return;
    }
    try {
      const data = await api.get(
        `/sessions/${id}/margins?path=${encodeURIComponent(path)}`,
      );
      if (this.#openPath !== path) return;
      const text = this.#currentText();
      const { margins, moved } = relocateAll(text, data.margins || []);
      this.#margins = margins;
      this.#applyMarginMarks();
      if (moved.length) this.#persistMovedMargins(moved);
    } catch (_) {
      if (this.#openPath !== path) return;
      this.#margins = [];
      this.#applyMarginMarks();
    }
  }

  async #persistMovedMargins(moved) {
    const id = this.#sessionId();
    for (const m of moved) {
      try {
        const full = await api.get(`/sessions/${id}/margins/${encodeURIComponent(m.id)}`);
        await api.put(`/sessions/${id}/margins/${encodeURIComponent(m.id)}`, {
          ...full,
          start: m.start,
          end: m.end,
        });
      } catch (_) {
        /* leave the mark where relocate put it in-memory */
      }
    }
  }

  #applyMarginMarks() {
    if (this.#view) this.#marginGutter.apply(this.#view, this.#margins);
  }

  #openMarginEntries(entries) {
    if (!entries?.length) return;
    this.#loadMargin(entries[0].id);
  }

  async #loadMargin(mid) {
    const id = this.#sessionId();
    try {
      const data = await api.get(`/sessions/${id}/margins/${encodeURIComponent(mid)}`);
      this.#marginOpen = data;
      this.#marginDraft = {
        id: data.id,
        path: data.path,
        start: data.start,
        end: data.end,
        quote: data.quote || "",
        title: data.title || "",
        body: data.body || "",
        media: data.media || [],
      };
      this.#marginMode = "view";
      this.#paintMarginPane();
      this.#paintChrome();
    } catch (err) {
      showToast(err.message || "Could not open that margin.", { kind: "error" });
    }
  }

  #addMarginAtCursor() {
    if (!this.#openPath || !this.#view) return;
    const sel = this.#view.state.selection.main;
    const doc = this.#view.state.doc;
    const startLine = doc.lineAt(sel.from).number;
    const endLine = doc.lineAt(sel.to).number;
    const text = doc.toString();
    const draft = {
      id: uuid(),
      path: this.#openPath,
      start: startLine,
      end: endLine,
      quote: quoteLines(text, startLine, endLine),
      title: "",
      body: "",
      media: [],
    };
    this.#marginOpen = null;
    this.#marginDraft = draft;
    this.#marginMode = "edit";
    this.#paintMarginPane();
    this.#paintChrome();
  }

  #paintMarginPane() {
    const pane = this.querySelector("[data-margin-pane]");
    if (!pane) return;
    const d = this.#marginDraft;
    if (!d) {
      pane.hidden = true;
      pane.innerHTML = "";
      return;
    }
    pane.hidden = false;
    const lines =
      d.start === d.end ? `line ${d.start}` : `lines ${d.start}–${d.end}`;
    if (this.#marginMode === "edit") {
      pane.innerHTML = `<header class="files-margin-head">
          <strong>Margin</strong>
          <span class="muted">${escapeText(lines)}</span>
          <button type="button" class="ghost sm" data-m-act="close">Close</button>
        </header>
        <label class="files-margin-label">Title
          <input type="text" data-m-title maxlength="120" value="${escapeText(d.title)}" />
        </label>
        <label class="files-margin-label">Body
          <textarea data-m-body rows="12">${escapeText(d.body)}</textarea>
        </label>
        <div class="files-margin-actions">
          <button type="button" class="sm" data-m-act="save">Save</button>
          ${d.id && this.#marginOpen ? `<button type="button" class="ghost sm danger" data-m-act="delete">Delete</button>` : ""}
          <label class="ghost sm files-margin-attach">Attach image
            <input type="file" accept="image/png,image/jpeg,image/gif,image/webp,image/svg+xml" data-m-file hidden />
          </label>
        </div>
        <p class="muted sm">Images in the body: <code>![alt](media/name.png)</code></p>`;
      return;
    }
    const bodyHtml = this.#marginOpen?.body_html || "";
    const markup = html`<div class="md-body md-body-compact">${fromServerHtml(bodyHtml)}</div>`;
    pane.innerHTML = `<header class="files-margin-head">
        <strong>${escapeText(d.title || "Margin")}</strong>
        <span class="muted">${escapeText(lines)}</span>
        <button type="button" class="ghost sm" data-m-act="edit">Edit</button>
        <button type="button" class="ghost sm" data-m-act="close">Close</button>
      </header>
      <div class="files-margin-body">${markup.value}</div>`;
  }

  async #onMarginPaneClick(ev) {
    const act = ev.target.closest("[data-m-act]")?.getAttribute("data-m-act");
    const fileInput = ev.target.closest("[data-m-file]");
    if (fileInput) {
      fileInput.addEventListener(
        "change",
        () => this.#attachMarginImage(fileInput),
        { once: true },
      );
      return;
    }
    if (!act) return;
    if (act === "close") {
      this.#marginDraft = null;
      this.#marginOpen = null;
      this.#paintMarginPane();
      this.#paintChrome();
      return;
    }
    if (act === "edit") {
      this.#marginMode = "edit";
      this.#paintMarginPane();
      return;
    }
    if (act === "save") {
      await this.#saveMargin();
      return;
    }
    if (act === "delete") {
      await this.#deleteMargin();
    }
  }

  async #saveMargin() {
    const pane = this.querySelector("[data-margin-pane]");
    const d = this.#marginDraft;
    if (!d || !pane) return;
    const title = pane.querySelector("[data-m-title]")?.value?.trim() || "";
    const body = pane.querySelector("[data-m-body]")?.value ?? d.body;
    if (!title) {
      let err = pane.querySelector("[data-m-err]");
      if (!err) {
        err = document.createElement("p");
        err.className = "files-error";
        err.setAttribute("data-m-err", "");
        pane.querySelector(".files-margin-actions")?.before(err);
      }
      err.hidden = false;
      err.textContent = "Give the margin a title.";
      pane.querySelector("[data-m-title]")?.focus();
      return;
    }
    const id = this.#sessionId();
    const payload = { ...d, title, body };
    try {
      const saved = await api.put(
        `/sessions/${id}/margins/${encodeURIComponent(d.id)}`,
        payload,
      );
      this.#marginOpen = saved;
      this.#marginDraft = {
        id: saved.id,
        path: saved.path,
        start: saved.start,
        end: saved.end,
        quote: saved.quote || "",
        title: saved.title || "",
        body: saved.body || "",
        media: saved.media || [],
      };
      this.#marginMode = "view";
      await this.#loadMarginsFor(this.#openPath);
      this.#paintMarginPane();
    } catch (err) {
      showToast(err.message || "Could not save the margin.", { kind: "error" });
    }
  }

  async #deleteMargin() {
    const d = this.#marginDraft;
    if (!d?.id) return;
    if (
      !(await askConfirm({
        title: "Delete margin",
        message: "Delete this margin? Attached images go with it.",
        confirmLabel: "Delete",
        danger: true,
      }))
    ) {
      return;
    }
    const id = this.#sessionId();
    try {
      await api.del(`/sessions/${id}/margins/${encodeURIComponent(d.id)}`);
      this.#marginDraft = null;
      this.#marginOpen = null;
      await this.#loadMarginsFor(this.#openPath);
      this.#paintMarginPane();
      this.#paintChrome();
    } catch (err) {
      showToast(err.message || "Could not delete the margin.", { kind: "error" });
    }
  }

  async #attachMarginImage(input) {
    const file = input.files?.[0];
    const d = this.#marginDraft;
    if (!file || !d) return;
    // Save first so the margin id exists on disk.
    const pane = this.querySelector("[data-margin-pane]");
    if (pane?.querySelector("[data-m-title]")) {
      d.title = pane.querySelector("[data-m-title]").value.trim() || d.title || "Margin";
      d.body = pane.querySelector("[data-m-body]")?.value ?? d.body;
    }
    if (!d.title) d.title = "Margin";
    const id = this.#sessionId();
    try {
      await api.put(`/sessions/${id}/margins/${encodeURIComponent(d.id)}`, d);
      const buf = await file.arrayBuffer();
      const name = (file.name || "image.png").replace(/[^\w.\-]+/g, "_");
      await fetch(
        `/api/v1/sessions/${encodeURIComponent(id)}/margins/${encodeURIComponent(d.id)}/media/${encodeURIComponent(name)}`,
        {
          method: "PUT",
          credentials: "same-origin",
          headers: { "Content-Type": file.type || "application/octet-stream" },
          body: buf,
        },
      ).then(async (res) => {
        if (!res.ok) {
          const data = await res.json().catch(() => ({}));
          throw new ApiError({
            code: data?.error?.code || "http_error",
            message: data?.error?.message || res.statusText,
            status: res.status,
          });
        }
      });
      const snip = `![${name}](media/${name})`;
      d.body = d.body ? `${d.body.trimEnd()}\n\n${snip}\n` : `${snip}\n`;
      if (!d.media.includes(name)) d.media.push(name);
      this.#marginDraft = d;
      this.#marginMode = "edit";
      await this.#loadMargin(d.id);
      this.#marginMode = "edit";
      this.#paintMarginPane();
      const bodyEl = this.querySelector("[data-m-body]");
      if (bodyEl) bodyEl.value = d.body;
    } catch (err) {
      showToast(err.message || "Could not attach that image.", { kind: "error" });
    } finally {
      input.value = "";
    }
  }

  #currentText() {
    if (this.#view) return this.#view.state.doc.toString();
    const cached = this.#openPath ? this.#cache.get(this.#openPath) : null;
    return cached?.unsaved ?? cached?.text ?? "";
  }

  async #renderMarkdown() {
    if (!this.#openPath || !isMarkdownPath(this.#openPath)) return;
    const id = this.#sessionId();
    const text = this.#currentText();
    const token = ++this.#mdRenderToken;
    const pane = this.querySelector("[data-md-view]");
    try {
      const data = await api.post(`/sessions/${id}/markdown`, {
        text,
        base_path: this.#openPath,
      });
      if (token !== this.#mdRenderToken) return;
      this.#mdHtml = data.html || "";
      this.#mdError = null;
      if (pane) {
        const markup = html`<div class="md-body">${fromServerHtml(this.#mdHtml)}</div>`;
        pane.innerHTML = markup.value;
      }
    } catch (err) {
      if (token !== this.#mdRenderToken) return;
      this.#mdError = err.message || "Could not render markdown.";
      this.#mdHtml = "";
      if (pane) {
        pane.innerHTML = `<div class="files-md-error">${escapeText(this.#mdError)}</div>`;
      }
    }
  }

  #onMdLinkClick(ev) {
    const a = ev.target.closest?.('a[href^="#dync-open:"]');
    if (!a) return;
    ev.preventDefault();
    const href = a.getAttribute("href") || "";
    const enc = href.slice("#dync-open:".length);
    let path = "";
    try {
      path = decodeURIComponent(enc);
    } catch (_) {
      return;
    }
    if (!path || path.includes("\0")) return;
    this.#openFile(path);
  }

  #retheme() {
    if (!this.#view) return;
    this.#view.dispatch({
      effects: this.#themeComp.reconfigure(buildTheme()),
    });
  }

  async #save(force) {
    if (!this.#openPath || !this.#view) return;
    const id = this.#sessionId();
    const text = this.#currentText();
    try {
      const data = await api.put(`/sessions/${id}/file`, {
        path: this.#openPath,
        text,
        revision: this.#revision || "",
        force: !!force,
      });
      this.#revision = data.revision;
      this.#dirty = false;
      this.#stale = false;
      this.#cache.set(this.#openPath, {
        text,
        revision: data.revision,
        unsaved: null,
        scroll: this.#view.scrollDOM.scrollTop,
        selection: {
          anchor: this.#view.state.selection.main.anchor,
          head: this.#view.state.selection.main.head,
        },
        dirty: false,
        stale: false,
      });
      this.#teardownUnload();
      this.#paintChrome();
    } catch (err) {
      if (err instanceof ApiError && err.code === "stale") {
        this.#stale = true;
        this.#paintChrome();
        return;
      }
      this.#openError = err.message || "Save failed.";
      this.#paintChrome();
    }
  }

  async #reloadOpen(clearDirty) {
    if (!this.#openPath) return;
    if (!clearDirty && this.#dirty) {
      if (
        !(await askConfirm({
          title: "Reload file",
          message: "Reload and lose unsaved edits?",
          confirmLabel: "Reload",
          danger: true,
        }))
      ) {
        return;
      }
    }
    const id = this.#sessionId();
    try {
      const data = await api.get(
        `/sessions/${id}/file?path=${encodeURIComponent(this.#openPath)}`,
      );
      this.#revision = data.revision;
      this.#dirty = false;
      this.#stale = false;
      this.#openError = null;
      this.#cache.set(this.#openPath, {
        text: data.text,
        revision: data.revision,
        unsaved: null,
        scroll: 0,
        selection: { anchor: 0, head: 0 },
        dirty: false,
        stale: false,
      });
      this.#showEditor(data.text, this.#openPath, null);
      this.#teardownUnload();
    } catch (err) {
      this.#openError = err.message || "Reload failed.";
    }
    this.#paintChrome();
  }

  async #onTurnEnded() {
    if (!this.#openPath) return;
    const id = this.#sessionId();
    try {
      const data = await api.get(
        `/sessions/${id}/file?path=${encodeURIComponent(this.#openPath)}`,
      );
      if (data.revision === this.#revision) return;
      if (!this.#dirty) {
        this.#revision = data.revision;
        this.#cache.set(this.#openPath, {
          text: data.text,
          revision: data.revision,
          unsaved: null,
          scroll: this.#view?.scrollDOM.scrollTop || 0,
          selection: this.#view
            ? {
                anchor: this.#view.state.selection.main.anchor,
                head: this.#view.state.selection.main.head,
              }
            : { anchor: 0, head: 0 },
          dirty: false,
          stale: false,
        });
        this.#showEditor(data.text, this.#openPath, this.#cache.get(this.#openPath));
      } else {
        this.#stale = true;
      }
      this.#paintChrome();
    } catch (_) {
      /* ignore transient */
    }
  }

  #paintChrome() {
    const pathEl = this.querySelector("[data-path]");
    const dirtyEl = this.querySelector("[data-dirty]");
    const staleEl = this.querySelector("[data-stale]");
    const emptyEl = this.querySelector("[data-empty]");
    const errorEl = this.querySelector("[data-error]");
    const cmEl = this.querySelector("[data-cm]");
    const mdEl = this.querySelector("[data-md-view]");
    const mdModeEl = this.querySelector("[data-md-mode]");
    const saveBtn = this.querySelector("[data-save]");
    const reloadBtn = this.querySelector("[data-reload]");
    const addMarginBtn = this.querySelector("[data-add-margin]");

    if (pathEl) pathEl.textContent = this.#openPath || "";
    if (dirtyEl) dirtyEl.hidden = !this.#dirty;
    if (staleEl) staleEl.hidden = !this.#stale;
    if (saveBtn) saveBtn.disabled = !this.#openPath || (!this.#dirty && !this.#stale);
    if (reloadBtn) reloadBtn.disabled = !this.#openPath;

    const showError = !!this.#openError && !!this.#openPath;
    const isMd = !!this.#openPath && isMarkdownPath(this.#openPath);
    const showView = !!this.#openPath && !showError && isMd && this.#mdMode === "view";
    const showSource = !!this.#openPath && !showError && (!isMd || this.#mdMode === "source");
    if (emptyEl) emptyEl.hidden = !!this.#openPath;
    if (errorEl) {
      errorEl.hidden = !showError;
      errorEl.textContent = this.#openError || "";
    }
    if (mdModeEl) {
      mdModeEl.hidden = !isMd || showError;
      mdModeEl.querySelectorAll("[data-md]").forEach((btn) => {
        btn.classList.toggle("active", btn.getAttribute("data-md") === this.#mdMode);
      });
    }
    if (addMarginBtn) {
      addMarginBtn.hidden = !this.#openPath || showError || (isMd && this.#mdMode === "view");
    }
    if (mdEl) mdEl.hidden = !showView;
    if (cmEl) cmEl.hidden = !showSource;
  }

  #setupUnload() {
    if (this.#prevState) return;
    this.#prevState = (ev) => {
      if (!this.#dirty) return;
      ev.preventDefault();
      ev.returnValue = "";
    };
    window.addEventListener("beforeunload", this.#prevState);
  }

  #teardownUnload() {
    if (!this.#prevState) return;
    window.removeEventListener("beforeunload", this.#prevState);
    this.#prevState = null;
  }

  #startSplit(ev) {
    ev.preventDefault();
    const pane = this.querySelector("[data-tree-pane]");
    const split = this.querySelector(".files-split");
    if (!pane || !split) return;
    this.classList.add("is-splitting");
    const startX = ev.clientX;
    const startW = pane.getBoundingClientRect().width;
    const onMove = (e) => {
      const max = split.getBoundingClientRect().width - MIN_PANE - 5;
      const w = Math.min(max, Math.max(MIN_PANE, startW + (e.clientX - startX)));
      this.#split = w;
      this.style.setProperty("--files-tree-w", `${w}px`);
    };
    const onUp = () => {
      this.classList.remove("is-splitting");
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      prefs.set("files-split", Math.round(this.#split));
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-tool-files")) {
  customElements.define("dync-tool-files", DyncToolFiles);
}
