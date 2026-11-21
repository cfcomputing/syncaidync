/** Pure file-tree state helpers and language detection. */

/**
 * @typedef {{ name: string, kind: string, size?: number, ignored?: boolean }} TreeEntry
 * @typedef {{
 *   expanded: Set<string>,
 *   children: Map<string, TreeEntry[]>,
 * }} TreeState
 */

export function createTreeState() {
  return {
    expanded: new Set(),
    children: new Map(),
  };
}

/** Join parent path with a child name using `/`. Root parent is `""`. */
export function joinPath(parent, name) {
  if (!parent) return name;
  return `${parent}/${name}`;
}

/**
 * Flatten the tree into rows for rendering.
 * Only expanded folders with loaded children are walked.
 * @param {TreeState} state
 * @returns {{ path: string, name: string, kind: string, depth: number, ignored: boolean, expanded: boolean, loaded: boolean }[]}
 */
export function flatten(state) {
  const rows = [];
  const root = state.children.get("") || [];
  walk(state, root, "", 0, rows);
  return rows;
}

function walk(state, entries, parent, depth, rows) {
  for (const ent of entries) {
    const path = joinPath(parent, ent.name);
    const isDir = ent.kind === "dir";
    const expanded = isDir && state.expanded.has(path);
    const loaded = isDir && state.children.has(path);
    rows.push({
      path,
      name: ent.name,
      kind: ent.kind,
      depth,
      ignored: !!ent.ignored,
      expanded,
      loaded,
    });
    if (isDir && expanded && loaded) {
      walk(state, state.children.get(path) || [], path, depth + 1, rows);
    }
  }
}

const EXT_LANG = {
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "javascript",
  ts: "javascript",
  tsx: "javascript",
  rs: "rust",
  py: "python",
  json: "json",
  md: "markdown",
  markdown: "markdown",
  html: "html",
  htm: "html",
  css: "css",
  scss: "css",
  less: "css",
  yml: "yaml",
  yaml: "yaml",
  go: "go",
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  toml: "toml",
};

const NAME_LANG = {
  dockerfile: "shell",
  makefile: "shell",
  gnumakefile: "shell",
};

/** Return a language id for CodeMirror, or null. */
export function languageFor(path) {
  if (!path) return null;
  const base = path.split("/").pop() || "";
  const lower = base.toLowerCase();
  if (NAME_LANG[lower]) return NAME_LANG[lower];
  const dot = lower.lastIndexOf(".");
  if (dot < 0) return null;
  return EXT_LANG[lower.slice(dot + 1)] || null;
}

/** True for `.md` / `.markdown` paths. */
export function isMarkdownPath(path) {
  return languageFor(path) === "markdown";
}

/**
 * Normalize a user-typed relative path for creating a new file.
 * @returns {string|null} cleaned path, or null if refused
 */
export function normalizeNewFilePath(raw) {
  let p = String(raw ?? "")
    .trim()
    .replace(/\\/g, "/");
  while (p.startsWith("./")) p = p.slice(2);
  p = p.replace(/^\/+/, "");
  if (!p || p.endsWith("/")) return null;
  const parts = p.split("/");
  for (const part of parts) {
    if (!part || part === "." || part === ".." || part === ".git") return null;
  }
  return parts.join("/");
}

/** Parent folder of a relative path, or `""` for the workspace root. */
export function parentDir(path) {
  const p = String(path ?? "");
  const i = p.lastIndexOf("/");
  return i <= 0 ? "" : p.slice(0, i);
}
