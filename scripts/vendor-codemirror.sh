#!/bin/sh
# Bundle MIT-licensed CodeMirror 6 into web/vendor/codemirror/.
set -eu
cd "$(dirname "$0")/.."

OUT=web/vendor/codemirror
TMP=.vendor-tmp/codemirror
rm -rf "$TMP" "$OUT"
mkdir -p "$TMP" "$OUT"

# Pinned exact versions (update together when refreshing the vendor).
CM=6.0.2
LANG_JS=6.2.5
LANG_RUST=6.0.2
LANG_PY=6.2.1
LANG_JSON=6.0.2
LANG_MD=6.5.2
LANG_HTML=6.4.12
LANG_CSS=6.3.1
LANG_YAML=6.1.3
LANG_GO=6.0.1
LEGACY=6.5.5
ESBUILD=0.25.12

cd "$TMP"
npm init -y >/dev/null
npm install --no-fund --no-audit \
  "codemirror@${CM}" \
  "@codemirror/lang-javascript@${LANG_JS}" \
  "@codemirror/lang-rust@${LANG_RUST}" \
  "@codemirror/lang-python@${LANG_PY}" \
  "@codemirror/lang-json@${LANG_JSON}" \
  "@codemirror/lang-markdown@${LANG_MD}" \
  "@codemirror/lang-html@${LANG_HTML}" \
  "@codemirror/lang-css@${LANG_CSS}" \
  "@codemirror/lang-yaml@${LANG_YAML}" \
  "@codemirror/lang-go@${LANG_GO}" \
  "@codemirror/legacy-modes@${LEGACY}" \
  "esbuild@${ESBUILD}" >/dev/null

# Fail unless every installed package declares license MIT.
node <<'NODE'
const fs = require("fs");
const path = require("path");
const root = path.join(process.cwd(), "node_modules");
const offenders = [];

function walk(dir) {
  if (!fs.existsSync(dir)) return;
  for (const name of fs.readdirSync(dir)) {
    if (name.startsWith(".")) continue;
    const p = path.join(dir, name);
    if (name.startsWith("@")) {
      for (const scoped of fs.readdirSync(p)) check(path.join(p, scoped));
    } else {
      check(p);
    }
  }
}

function check(pkgDir) {
  const pj = path.join(pkgDir, "package.json");
  if (!fs.existsSync(pj)) return;
  const pkg = JSON.parse(fs.readFileSync(pj, "utf8"));
  if (pkg.license !== "MIT") {
    offenders.push(`${pkg.name || pkgDir}: ${JSON.stringify(pkg.license)}`);
  }
  const nested = path.join(pkgDir, "node_modules");
  if (fs.existsSync(nested)) walk(nested);
}

walk(root);
if (offenders.length) {
  console.error("vendor-codemirror: non-MIT licences:");
  for (const o of offenders) console.error("  " + o);
  process.exit(1);
}
console.log("vendor-codemirror: all packages MIT");
NODE

cat > entry.js <<'JS'
export { basicSetup } from "codemirror";
export {
  EditorView,
  keymap,
  Decoration,
  gutter,
  GutterMarker,
} from "@codemirror/view";
export {
  EditorState,
  Compartment,
  StateField,
  StateEffect,
  RangeSetBuilder,
} from "@codemirror/state";
export {
  HighlightStyle,
  syntaxHighlighting,
  StreamLanguage,
} from "@codemirror/language";
export { indentWithTab } from "@codemirror/commands";
export { tags } from "@lezer/highlight";

export { javascript } from "@codemirror/lang-javascript";
export { rust } from "@codemirror/lang-rust";
export { python } from "@codemirror/lang-python";
export { json } from "@codemirror/lang-json";
export { markdown } from "@codemirror/lang-markdown";
export { html } from "@codemirror/lang-html";
export { css } from "@codemirror/lang-css";
export { yaml } from "@codemirror/lang-yaml";
export { go } from "@codemirror/lang-go";

import { StreamLanguage } from "@codemirror/language";
import { shell as shellMode } from "@codemirror/legacy-modes/mode/shell";
import { toml as tomlMode } from "@codemirror/legacy-modes/mode/toml";
export const shell = () => StreamLanguage.define(shellMode);
export const toml = () => StreamLanguage.define(tomlMode);
JS

./node_modules/.bin/esbuild entry.js \
  --bundle \
  --format=esm \
  --minify \
  --target=es2022 \
  --outfile="../../${OUT}/codemirror.js"

node <<NODE
const fs = require("fs");
const path = require("path");
const root = path.join(process.cwd(), "node_modules");
const outDir = path.resolve("../..", "${OUT}");
const seen = new Map();

function walk(dir) {
  if (!fs.existsSync(dir)) return;
  for (const name of fs.readdirSync(dir)) {
    if (name.startsWith(".")) continue;
    const p = path.join(dir, name);
    if (name.startsWith("@")) {
      for (const scoped of fs.readdirSync(p)) check(path.join(p, scoped));
    } else {
      check(p);
    }
  }
}

function check(pkgDir) {
  const pj = path.join(pkgDir, "package.json");
  if (!fs.existsSync(pj)) return;
  const pkg = JSON.parse(fs.readFileSync(pj, "utf8"));
  if (!pkg.name || seen.has(pkg.name)) {
    const nested = path.join(pkgDir, "node_modules");
    if (fs.existsSync(nested)) walk(nested);
    return;
  }
  let licenseText = "";
  for (const cand of ["LICENSE", "LICENSE.md", "LICENSE.txt", "license", "LICENCE"]) {
    const lp = path.join(pkgDir, cand);
    if (fs.existsSync(lp) && fs.statSync(lp).isFile()) {
      licenseText = fs.readFileSync(lp, "utf8");
      break;
    }
  }
  if (!licenseText) {
    licenseText = "(no LICENSE file; package.json license: " + pkg.license + ")\n";
  }
  seen.set(pkg.name, { version: pkg.version, licenseText });
  const nested = path.join(pkgDir, "node_modules");
  if (fs.existsSync(nested)) walk(nested);
}

walk(root);
const names = [...seen.keys()].sort();
let out = "";
for (const name of names) {
  const { version, licenseText } = seen.get(name);
  out += "=".repeat(72) + "\n" + name + " " + version + "\n" + "=".repeat(72) + "\n" + licenseText.trim() + "\n\n";
}
fs.writeFileSync(path.join(outDir, "LICENSES.txt"), out);
console.log("vendor-codemirror: wrote LICENSES.txt (" + names.length + " packages)");
NODE

{
  echo "codemirror ${CM}"
  echo "@codemirror/lang-javascript ${LANG_JS}"
  echo "@codemirror/lang-rust ${LANG_RUST}"
  echo "@codemirror/lang-python ${LANG_PY}"
  echo "@codemirror/lang-json ${LANG_JSON}"
  echo "@codemirror/lang-markdown ${LANG_MD}"
  echo "@codemirror/lang-html ${LANG_HTML}"
  echo "@codemirror/lang-css ${LANG_CSS}"
  echo "@codemirror/lang-yaml ${LANG_YAML}"
  echo "@codemirror/lang-go ${LANG_GO}"
  echo "@codemirror/legacy-modes ${LEGACY}"
  echo "esbuild ${ESBUILD} (build only)"
  echo "vendored $(date -u +%Y-%m-%d)"
} > "../../${OUT}/VERSION"

cd ../..
rm -rf .vendor-tmp
echo "vendor-codemirror: ok -> $OUT"
