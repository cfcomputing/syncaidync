import { DyncElement, html } from "../lib/element.js";
import { api, ApiError } from "../lib/api.js";
import { store } from "../lib/store.js";
import * as prefs from "../lib/prefs.js";
import { openDialog, setBusy } from "../lib/dialog.js";
import "./dync-goal-setup.js";

const DEFAULTS = {
  agent: "shell",
  model: "",
  model_by_agent: {},
  workspace_kind: "scratch",
  folder: "",
  repo: "",
  base: "",
  branch: "",
  title: "",
  prompt: "",
  autonomy: false,
};

const CUSTOM_MODEL = "__custom__";

export class DyncNewSession extends DyncElement {
  #dialog = null;
  #repos = [];
  #branchTouched = false;
  #movedForGoal = false;
  #comboIndex = -1;
  #folderParent = null;
  #folderLoad = 0;

  mount() {
    this.render(
      html`<dialog class="new-session-dialog">
        <form method="dialog" class="dialog-body" data-form>
          <h2>New session</h2>
          <p class="hint">Pick an agent and where it should work.</p>

          <fieldset data-agents>
            <legend>Agent</legend>
            <div class="choice-row" data-agent-row></div>
            <p class="muted sm" data-agent-missing hidden></p>
          </fieldset>

          <div data-model-fields>
            <label data-model-select-wrap>
              <span>Model</span>
              <select name="model_pick" aria-label="Model"></select>
            </label>
            <label data-model-custom-wrap hidden>
              <span data-model-custom-label>Model</span>
              <input name="model" placeholder="Agent default" autocomplete="off" spellcheck="false" />
            </label>
          </div>

          <fieldset>
            <legend>Where to run</legend>
            <div class="choice-row" role="radiogroup" aria-label="Workspace kind">
              <button type="button" class="choice" data-kind="worktree">New worktree</button>
              <button type="button" class="choice" data-kind="folder">Existing folder</button>
              <button type="button" class="choice" data-kind="scratch">Scratch folder</button>
            </div>
          </fieldset>

          <div data-worktree-fields hidden>
            <label class="combo">
              <span>Repository</span>
              <input name="repo" autocomplete="off" spellcheck="false" placeholder="Filter or paste a path" role="combobox" aria-autocomplete="list" aria-expanded="false" aria-controls="dync-repo-list" />
              <ul id="dync-repo-list" class="combo-list" role="listbox" hidden></ul>
            </label>
            <label>
              <span>Base branch</span>
              <input name="base" list="dync-base-branches" autocomplete="off" />
              <datalist id="dync-base-branches"></datalist>
            </label>
            <label>
              <span>Branch</span>
              <input name="branch" autocomplete="off" spellcheck="false" />
            </label>
          </div>

          <div data-folder-fields hidden class="folder-fields">
            <label>
              <span>Folder</span>
              <div class="folder-path-row">
                <button type="button" class="ghost sm" data-folder-up title="Up one level" aria-label="Up one level">↑</button>
                <input name="folder" placeholder="Pick below or type a path" autocomplete="off" spellcheck="false" />
                <button type="button" class="ghost sm" data-folder-go>Go</button>
              </div>
            </label>
            <div class="folder-picker" data-folder-picker>
              <div class="folder-picker-list" role="listbox" aria-label="Folders" data-folder-list></div>
              <p class="muted sm" data-folder-status></p>
            </div>
          </div>

          <label>
            <span>Title</span>
            <input name="title" maxlength="120" placeholder="Optional" />
          </label>

          <label>
            <span>First prompt</span>
            <textarea name="prompt" rows="4" placeholder="Optional"></textarea>
          </label>

          <dync-goal-setup data-goal></dync-goal-setup>

          <label class="check">
            <input type="checkbox" name="autonomy" />
            <span>Run without approval prompts — the agent will run commands and edit files without asking.</span>
          </label>

          <p class="error" data-error hidden></p>

          <div class="dialog-actions">
            <button type="submit" value="cancel" class="ghost">Cancel</button>
            <button type="submit" value="start" class="primary">Start</button>
          </div>
        </form>
      </dialog>`,
    );

    this.#dialog = this.querySelector("dialog");
    const form = this.querySelector("[data-form]");
    const goal = this.querySelector("[data-goal]");
    goal.context = () => this.#draftContext(form);
    this.on(goal, "dync-goal-toggle", () => this.#onGoalToggle(form, goal));

    this.on(this, "click", (ev) => {
      const kindBtn = ev.target.closest("[data-kind]");
      if (kindBtn && !kindBtn.disabled) {
        this.#setKind(kindBtn.getAttribute("data-kind"));
        this.#syncGoalNote();
        return;
      }
      const agentBtn = ev.target.closest("[data-agent]");
      if (agentBtn && !agentBtn.disabled) {
        this.#setAgent(agentBtn.getAttribute("data-agent"));
        return;
      }
      const opt = ev.target.closest("[data-repo-path]");
      if (opt) {
        this.#pickRepo(opt.getAttribute("data-repo-path"));
        return;
      }
      if (ev.target.closest("[data-folder-up]")) {
        this.#folderUp();
        return;
      }
      if (ev.target.closest("[data-folder-go]")) {
        this.#browseFolder(form.folder.value.trim());
        return;
      }
      const folder = ev.target.closest("[data-folder-path]");
      if (folder) {
        this.#enterFolder(folder.getAttribute("data-folder-path"));
      }
    });

    this.on(form.repo, "input", () => this.#filterRepos());
    this.on(form.repo, "keydown", (ev) => this.#repoKey(ev));
    this.on(form.repo, "blur", () => {
      setTimeout(() => this.#hideRepoList(), 150);
    });
    this.on(form.folder, "keydown", (ev) => {
      if (ev.key === "Enter") {
        ev.preventDefault();
        this.#browseFolder(form.folder.value.trim());
      }
    });
    this.on(form.branch, "input", () => {
      this.#branchTouched = true;
    });
    this.on(form.title, "input", () => {
      if (this.#selectedKind() === "worktree" && !this.#branchTouched) {
        form.branch.value = suggestBranch(form.title.value);
      }
    });
    this.on(form.model_pick, "change", () => this.#onModelPick());

    this.on(form, "submit", async (ev) => {
      const submitter = ev.submitter;
      if (submitter?.value === "cancel") return;
      ev.preventDefault();
      await this.#start(form);
    });
  }

  async open() {
    this.#paintAgents();
    this.#hydrate();
    this.querySelector("[data-goal]")?.reset();
    this.#branchTouched = false;
    this.#movedForGoal = false;
    openDialog(this.#dialog, {
      focus: this.querySelector("[data-agent].on") || this.querySelector("[data-kind].on"),
      closeOnBackdrop: false,
    });
    try {
      const [meta, data] = await Promise.all([
        api.get("/meta").catch(() => null),
        api.get("/repos").catch(() => null),
      ]);
      if (meta) {
        store.meta = meta;
        const agent = this.#selectedAgent();
        this.#paintAgents();
        this.#setAgent(agent);
      }
      this.#repos = data?.repos || [];
      this.#filterRepos();
    } catch {
      this.#repos = [];
    }
  }

  #saved() {
    return { ...DEFAULTS, ...prefs.get("new-session", {}) };
  }

  #hydrate() {
    const s = this.#saved();
    const form = this.querySelector("[data-form]");
    form.folder.value = s.folder || "";
    form.repo.value = s.repo || "";
    form.base.value = s.base || "";
    form.branch.value = s.branch || "";
    form.title.value = s.title || "";
    form.prompt.value = s.prompt || "";
    form.autonomy.checked = !!s.autonomy;
    const kind =
      s.workspace_kind === "folder" || s.workspace_kind === "worktree"
        ? s.workspace_kind
        : "scratch";
    this.#setKind(kind);
    this.#setAgent(s.agent || "shell");
    this.#setError("");
    if (kind === "worktree" && s.repo) {
      this.#loadBranches(s.repo, { keepBase: !!s.base });
    }
  }

  #modelPrefs() {
    const s = this.#saved();
    const by = s.model_by_agent && typeof s.model_by_agent === "object" ? s.model_by_agent : {};
    return { by, legacy: s.model || "" };
  }

  #preferredModel(agent) {
    const { by, legacy } = this.#modelPrefs();
    if (Object.prototype.hasOwnProperty.call(by, agent)) return String(by[agent] || "");
    return legacy;
  }

  #agentModels(agent) {
    const info = (store.meta?.agents || []).find((a) => a.id === agent);
    const list = info?.models;
    return Array.isArray(list) ? list.filter((m) => typeof m === "string" && m) : [];
  }

  #paintModel(agent, preferred) {
    const form = this.querySelector("[data-form]");
    const fields = this.querySelector("[data-model-fields]");
    const selectWrap = this.querySelector("[data-model-select-wrap]");
    const customWrap = this.querySelector("[data-model-custom-wrap]");
    const customLabel = this.querySelector("[data-model-custom-label]");
    const pick = form.model_pick;
    const input = form.model;
    if (agent === "shell") {
      fields.hidden = true;
      input.value = "";
      pick.innerHTML = "";
      return;
    }
    fields.hidden = false;
    const models = this.#agentModels(agent);
    const want = preferred == null ? this.#preferredModel(agent) : preferred;

    if (!models.length) {
      selectWrap.hidden = true;
      customWrap.hidden = false;
      customLabel.textContent = "Model";
      input.value = want;
      pick.innerHTML = "";
      return;
    }

    selectWrap.hidden = false;
    pick.innerHTML = "";
    const add = (value, label) => {
      const opt = document.createElement("option");
      opt.value = value;
      opt.textContent = label;
      pick.appendChild(opt);
    };
    add("", "Agent default");
    for (const m of models) add(m, m);
    add(CUSTOM_MODEL, "Custom…");

    if (!want) {
      pick.value = "";
      customWrap.hidden = true;
      input.value = "";
      return;
    }
    if (models.includes(want)) {
      pick.value = want;
      customWrap.hidden = true;
      input.value = want;
      return;
    }
    pick.value = CUSTOM_MODEL;
    customWrap.hidden = false;
    customLabel.textContent = "Custom model";
    input.value = want;
  }

  #onModelPick() {
    const form = this.querySelector("[data-form]");
    const customWrap = this.querySelector("[data-model-custom-wrap]");
    const customLabel = this.querySelector("[data-model-custom-label]");
    const value = form.model_pick.value;
    if (value === CUSTOM_MODEL) {
      customWrap.hidden = false;
      customLabel.textContent = "Custom model";
      form.model.value = "";
      form.model.focus();
      return;
    }
    customWrap.hidden = true;
    form.model.value = value;
  }

  #selectedModel(form) {
    const models = this.#agentModels(this.#selectedAgent());
    if (!models.length) return form.model.value.trim() || null;
    const pick = form.model_pick.value;
    if (pick === CUSTOM_MODEL) return form.model.value.trim() || null;
    return pick.trim() || null;
  }

  #paintAgents() {
    const row = this.querySelector("[data-agent-row]");
    const missing = this.querySelector("[data-agent-missing]");
    const agents = store.meta?.agents || [];
    const installed = new Map(agents.map((a) => [a.id, a]));
    const known = [
      { id: "claude", label: "Claude Code", cmd: "claude" },
      { id: "codex", label: "Codex", cmd: "codex" },
      { id: "pi", label: "Pi", cmd: "pi" },
      { id: "cursor", label: "Cursor Agent", cmd: "agent" },
      { id: "shell", label: "Shell", cmd: "shell" },
    ];
    row.innerHTML = "";
    const unavailable = [];
    for (const a of known) {
      const info = installed.get(a.id);
      const on = !!info?.installed;
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "choice";
      btn.dataset.agent = a.id;
      btn.textContent = a.label;
      btn.disabled = !on;
      if (!on) unavailable.push(a.cmd);
      row.appendChild(btn);
    }
    if (unavailable.length && missing) {
      missing.hidden = false;
      missing.textContent = `Not installed yet: ${unavailable.join(", ")}`;
    } else if (missing) {
      missing.hidden = true;
    }
  }

  #setKind(kind) {
    for (const btn of this.querySelectorAll("[data-kind]")) {
      btn.classList.toggle("on", btn.getAttribute("data-kind") === kind);
    }
    const folderFields = this.querySelector("[data-folder-fields]");
    const worktreeFields = this.querySelector("[data-worktree-fields]");
    if (folderFields) folderFields.hidden = kind !== "folder";
    if (worktreeFields) worktreeFields.hidden = kind !== "worktree";
    if (kind === "folder") {
      const form = this.querySelector("[data-form]");
      this.#browseFolder(form?.folder?.value?.trim() || "");
    }
  }

  #setAgent(id) {
    for (const btn of this.querySelectorAll("[data-agent]")) {
      btn.classList.toggle("on", btn.getAttribute("data-agent") === id);
    }
    this.#paintModel(id);
  }

  #selectedKind() {
    return this.querySelector("[data-kind].on")?.getAttribute("data-kind") || "scratch";
  }

  #selectedAgent() {
    return this.querySelector("[data-agent].on")?.getAttribute("data-agent") || "shell";
  }

  #setError(msg) {
    const el = this.querySelector("[data-error]");
    if (!el) return;
    el.hidden = !msg;
    el.textContent = msg || "";
  }

  #filterRepos() {
    const form = this.querySelector("[data-form]");
    const q = (form.repo.value || "").trim().toLowerCase();
    const list = this.querySelector("#dync-repo-list");
    const matches = this.#repos
      .filter((r) => !q || r.name.toLowerCase().includes(q) || r.path.toLowerCase().includes(q))
      .slice(0, 40);
    list.innerHTML = "";
    this.#comboIndex = -1;
    if (!matches.length || this.#selectedKind() !== "worktree") {
      this.#hideRepoList();
      return;
    }
    for (const r of matches) {
      const li = document.createElement("li");
      li.role = "option";
      li.dataset.repoPath = r.path;
      li.innerHTML = `<strong></strong><span></span>`;
      li.querySelector("strong").textContent = r.name;
      li.querySelector("span").textContent = r.path;
      list.appendChild(li);
    }
    list.hidden = false;
    form.repo.setAttribute("aria-expanded", "true");
  }

  #hideRepoList() {
    const list = this.querySelector("#dync-repo-list");
    const form = this.querySelector("[data-form]");
    if (list) list.hidden = true;
    form?.repo?.setAttribute("aria-expanded", "false");
    this.#comboIndex = -1;
  }

  #repoKey(ev) {
    const list = this.querySelector("#dync-repo-list");
    if (list.hidden) return;
    const items = [...list.querySelectorAll("[data-repo-path]")];
    if (ev.key === "ArrowDown") {
      ev.preventDefault();
      this.#comboIndex = Math.min(this.#comboIndex + 1, items.length - 1);
      this.#highlight(items);
    } else if (ev.key === "ArrowUp") {
      ev.preventDefault();
      this.#comboIndex = Math.max(this.#comboIndex - 1, 0);
      this.#highlight(items);
    } else if (ev.key === "Enter" && this.#comboIndex >= 0 && items[this.#comboIndex]) {
      ev.preventDefault();
      this.#pickRepo(items[this.#comboIndex].getAttribute("data-repo-path"));
    } else if (ev.key === "Escape") {
      this.#hideRepoList();
    }
  }

  #highlight(items) {
    items.forEach((el, i) => el.classList.toggle("on", i === this.#comboIndex));
    items[this.#comboIndex]?.scrollIntoView({ block: "nearest" });
  }

  async #pickRepo(path) {
    const form = this.querySelector("[data-form]");
    form.repo.value = path;
    this.#hideRepoList();
    await this.#loadBranches(path, { keepBase: false });
  }

  async #browseFolder(path) {
    const list = this.querySelector("[data-folder-list]");
    const status = this.querySelector("[data-folder-status]");
    const up = this.querySelector("[data-folder-up]");
    const form = this.querySelector("[data-form]");
    if (!list) return;
    const seq = ++this.#folderLoad;
    list.innerHTML = "";
    if (status) status.textContent = "Loading…";
    try {
      const q = path ? `?path=${encodeURIComponent(path)}` : "";
      const data = await api.get(`/folders${q}`);
      if (seq !== this.#folderLoad) return;
      this.#folderParent = data.parent ?? null;
      if (data.path) form.folder.value = data.path;
      list.innerHTML = "";
      for (const entry of data.entries || []) {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "folder-row";
        btn.role = "option";
        btn.dataset.folderPath = entry.path;
        btn.innerHTML = `<span class="folder-twist" aria-hidden="true">▸</span><span class="folder-name"></span>`;
        btn.querySelector(".folder-name").textContent = entry.name;
        list.appendChild(btn);
      }
      if (status) {
        if (!(data.entries || []).length) {
          status.textContent = data.path
            ? "No subfolders here. Start uses this folder."
            : "No folder roots found.";
        } else if (data.truncated) {
          status.textContent = "Showing the first 400 folders.";
        } else if (!data.path) {
          status.textContent = "Pick a root, then browse. Start uses the path above.";
        } else {
          status.textContent = "Click a folder to open it. Start uses the path above.";
        }
      }
      if (up) {
        // Enabled when inside a path, or when a saved typed path can return to roots.
        up.disabled = !data.path && this.#folderParent == null;
      }
    } catch (err) {
      if (seq !== this.#folderLoad) return;
      list.innerHTML = "";
      this.#folderParent = null;
      if (up) up.disabled = false;
      if (status) {
        const msg =
          err instanceof ApiError ? err.message : "Could not list folders.";
        status.textContent = `${msg} You can still Start with the typed path, or go up to browse.`;
      }
    }
  }

  #enterFolder(path) {
    const form = this.querySelector("[data-form]");
    if (form) form.folder.value = path;
    this.#browseFolder(path);
  }

  #folderUp() {
    const form = this.querySelector("[data-form]");
    if (this.#folderParent) {
      form.folder.value = this.#folderParent;
      this.#browseFolder(this.#folderParent);
      return;
    }
    form.folder.value = "";
    this.#browseFolder("");
  }

  async #loadBranches(path, { keepBase }) {
    const form = this.querySelector("[data-form]");
    const dl = this.querySelector("#dync-base-branches");
    try {
      const data = await api.get(`/repos/branches?path=${encodeURIComponent(path)}`);
      dl.innerHTML = "";
      for (const name of data.branches || []) {
        const opt = document.createElement("option");
        opt.value = name;
        dl.appendChild(opt);
      }
      if (!keepBase || !form.base.value) {
        form.base.value = data.default || data.current || "main";
      }
    } catch {
      /* typed path may still work at create time */
    }
  }

  /**
   * Turning a goal on while "Existing folder" points at a git checkout moves the session to a
   * new worktree of it, so the agent's rounds never land on the checkout itself.
   */
  async #onGoalToggle(form, goal) {
    if (!goal.enabled || this.#selectedKind() !== "folder") {
      this.#syncGoalNote();
      return;
    }
    const folder = form.folder.value.trim();
    if (!folder) {
      this.#syncGoalNote();
      return;
    }
    let data = null;
    try {
      data = await api.get(`/repos/branches?path=${encodeURIComponent(folder)}`);
    } catch {
      data = null;
    }
    if (!data || !goal.enabled || this.#selectedKind() !== "folder") {
      this.#syncGoalNote();
      return;
    }
    form.repo.value = folder;
    const dl = this.querySelector("#dync-base-branches");
    dl.innerHTML = "";
    for (const name of data.branches || []) {
      const opt = document.createElement("option");
      opt.value = name;
      dl.appendChild(opt);
    }
    form.base.value = data.current || data.default || "main";
    if (!this.#branchTouched || !form.branch.value.trim()) {
      form.branch.value = suggestBranch(form.title.value || goal.goalText || "goal");
    }
    this.#setKind("worktree");
    this.#movedForGoal = true;
    this.#syncGoalNote();
  }

  #syncGoalNote() {
    const goal = this.querySelector("[data-goal]");
    if (!goal) return;
    const kind = this.#selectedKind();
    let note = "";
    if (goal.enabled && kind === "worktree" && this.#movedForGoal) {
      const base = this.querySelector("[data-form]")?.base?.value?.trim() || "the base branch";
      note = `Goal sessions start in a new worktree, so the agent’s rounds stay off your checkout. It starts from the last commit on ${base}, without uncommitted changes. Pick Existing folder to run in the checkout instead.`;
    } else if (goal.enabled && kind === "folder") {
      note = "This runs in the folder itself: the agent’s changes land there directly.";
    }
    goal.workspaceNote = note;
  }

  /** What a goal draft needs from the dialog: the agent and where the session will run. */
  #draftContext(form) {
    const kind = this.#selectedKind();
    const agent = this.#selectedAgent();
    if (kind === "folder") {
      const path = form.folder.value.trim();
      return path
        ? { agent, workspace: { kind: "folder", path } }
        : { agent, error: "Pick a folder first so the draft can read it." };
    }
    if (kind === "worktree") {
      const repo = form.repo.value.trim();
      if (!repo) return { agent, error: "Pick a repository first so the draft can read it." };
      return {
        agent,
        workspace: {
          kind: "worktree",
          repo,
          base: form.base.value.trim() || "main",
          branch: form.branch.value.trim() || "dync/session",
        },
      };
    }
    return { agent, workspace: { kind: "scratch" } };
  }

  async #start(form) {
    const agent = this.#selectedAgent();
    const kind = this.#selectedKind();
    const model = this.#selectedModel(form);
    const title = form.title.value.trim() || null;
    const prompt = form.prompt.value.trim() || null;
    const autonomy = !!form.autonomy.checked;
    const folder = form.folder.value.trim();
    const repo = form.repo.value.trim();
    const base = form.base.value.trim() || "main";
    let branch = form.branch.value.trim();
    if (kind === "worktree" && !branch) {
      branch = suggestBranch(title || "");
      form.branch.value = branch;
    }

    const { by } = this.#modelPrefs();
    const model_by_agent = { ...by, [agent]: model || "" };
    prefs.set("new-session", {
      agent,
      model: model || "",
      model_by_agent,
      workspace_kind: kind,
      folder,
      repo,
      base,
      branch,
      title: title || "",
      prompt: prompt || "",
      autonomy,
    });

    let workspace;
    if (kind === "folder") {
      if (!folder) {
        this.#setError("Pick or type a folder path.");
        form.folder.focus();
        return;
      }
      workspace = { kind: "folder", path: folder };
    } else if (kind === "worktree") {
      if (!repo) {
        this.#setError("Pick or type a repository path.");
        form.repo.focus();
        return;
      }
      workspace = { kind: "worktree", repo, base, branch };
    } else {
      workspace = { kind: "scratch" };
    }

    const goalSetup = this.querySelector("[data-goal]");
    const goalError = goalSetup?.validate() || "";
    if (goalError) {
      this.#setError(goalError);
      return;
    }
    const goal = goalSetup?.payload() || null;
    const sessionTitle =
      title || (goal && !prompt ? goal.text.slice(0, 60) : null);

    this.#setError("");
    setBusy(form, true);
    try {
      await api.post("/sessions", {
        agent,
        model,
        title: sessionTitle,
        prompt,
        autonomy,
        workspace,
        ...(goal ? { goal } : {}),
      });
      this.#dialog.close();
    } catch (err) {
      const msg =
        err instanceof ApiError ? err.message : "Could not start the session.";
      this.#setError(msg);
    } finally {
      setBusy(form, false);
    }
  }
}

function suggestBranch(title) {
  const slug = String(title || "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-|-$/g, "")
    .slice(0, 60);
  return `dync/${slug || "session"}`;
}

if (typeof customElements !== "undefined" && !customElements.get("dync-new-session")) {
  customElements.define("dync-new-session", DyncNewSession);
}

export function openNewSession() {
  let el = document.querySelector("dync-new-session");
  if (!el) {
    el = document.createElement("dync-new-session");
    document.body.appendChild(el);
  }
  el.open();
}
