import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  createTreeState,
  flatten,
  languageFor,
  isMarkdownPath,
  joinPath,
  normalizeNewFilePath,
  parentDir,
} from "../js/lib/filetree.js";

describe("flatten", () => {
  it("walks nested expansion", () => {
    const state = createTreeState();
    state.children.set("", [
      { name: "src", kind: "dir" },
      { name: "README.md", kind: "file" },
    ]);
    state.children.set("src", [
      { name: "lib", kind: "dir" },
      { name: "main.rs", kind: "file" },
    ]);
    state.children.set("src/lib", [{ name: "mod.rs", kind: "file" }]);
    state.expanded.add("src");
    state.expanded.add("src/lib");

    const rows = flatten(state);
    assert.deepEqual(
      rows.map((r) => ({ path: r.path, depth: r.depth, expanded: r.expanded })),
      [
        { path: "src", depth: 0, expanded: true },
        { path: "src/lib", depth: 1, expanded: true },
        { path: "src/lib/mod.rs", depth: 2, expanded: false },
        { path: "src/main.rs", depth: 1, expanded: false },
        { path: "README.md", depth: 0, expanded: false },
      ],
    );
  });

  it("skips children of collapsed folders", () => {
    const state = createTreeState();
    state.children.set("", [{ name: "src", kind: "dir" }]);
    state.children.set("src", [{ name: "a.rs", kind: "file" }]);
    const rows = flatten(state);
    assert.equal(rows.length, 1);
    assert.equal(rows[0].path, "src");
    assert.equal(rows[0].expanded, false);
  });
});

describe("languageFor", () => {
  it("maps extensions and special names", () => {
    assert.equal(languageFor("src/main.rs"), "rust");
    assert.equal(languageFor("app.js"), "javascript");
    assert.equal(languageFor("app.ts"), "typescript");
    assert.equal(languageFor("app.tsx"), "tsx");
    assert.equal(languageFor("app.jsx"), "jsx");
    assert.equal(languageFor("Dockerfile"), "shell");
    assert.equal(languageFor("Makefile"), "shell");
    assert.equal(languageFor("config.toml"), "toml");
    assert.equal(languageFor("notes"), null);
  });
});

describe("isMarkdownPath", () => {
  it("recognizes md extensions", () => {
    assert.equal(isMarkdownPath("README.md"), true);
    assert.equal(isMarkdownPath("docs/a.markdown"), true);
    assert.equal(isMarkdownPath("main.rs"), false);
  });
});

describe("joinPath", () => {
  it("joins root and nested", () => {
    assert.equal(joinPath("", "a"), "a");
    assert.equal(joinPath("a", "b"), "a/b");
  });
});

describe("normalizeNewFilePath", () => {
  it("cleans relative paths and refuses escapes", () => {
    assert.equal(normalizeNewFilePath("  ./src/a.rs  "), "src/a.rs");
    assert.equal(normalizeNewFilePath("/README.md"), "README.md");
    assert.equal(normalizeNewFilePath(""), null);
    assert.equal(normalizeNewFilePath("src/"), null);
    assert.equal(normalizeNewFilePath("../x"), null);
    assert.equal(normalizeNewFilePath("a/../b"), null);
    assert.equal(normalizeNewFilePath(".git/config"), null);
    assert.equal(normalizeNewFilePath("src/.git"), null);
  });
});

describe("parentDir", () => {
  it("returns the folder or root", () => {
    assert.equal(parentDir("a.rs"), "");
    assert.equal(parentDir("src/a.rs"), "src");
    assert.equal(parentDir("src/lib/mod.rs"), "src/lib");
  });
});
