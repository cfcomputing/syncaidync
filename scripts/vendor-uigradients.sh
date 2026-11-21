#!/bin/sh
# Fetch MIT-licensed uiGradients gradients.json into web/vendor/uigradients/.
set -eu
cd "$(dirname "$0")/.."

# Pinned commit of ghosh/uiGradients (gradients.json).
COMMIT=c07376ba587c6607bfb01465a88b26ee5f92d80f
BASE="https://raw.githubusercontent.com/ghosh/uiGradients/${COMMIT}"

TMP=".vendor-tmp/uigradients"
rm -rf "$TMP" web/vendor/uigradients
mkdir -p "$TMP" web/vendor/uigradients

curl -fsSL "$BASE/gradients.json" -o "$TMP/gradients.json"
curl -fsSL "$BASE/LICENSE.md" -o "$TMP/LICENSE.md" 2>/dev/null \
  || curl -fsSL "$BASE/LICENSE" -o "$TMP/LICENSE.md" 2>/dev/null \
  || curl -fsSL "https://raw.githubusercontent.com/ghosh/uiGradients/${COMMIT}/LICENSE.md" -o "$TMP/LICENSE.md"

if ! grep -qi "MIT License" "$TMP/LICENSE.md"; then
  echo "vendor-uigradients: licence file does not contain 'MIT License'" >&2
  head -20 "$TMP/LICENSE.md" >&2 || true
  exit 1
fi

# Sanity: JSON array with name + colors
node -e '
const g = require("./.vendor-tmp/uigradients/gradients.json");
if (!Array.isArray(g) || g.length < 10) throw new Error("gradients.json looks wrong");
const ok = g.filter((x) => x && typeof x.name === "string" && Array.isArray(x.colors) && x.colors.length >= 2);
if (ok.length < 10) throw new Error("not enough usable gradients");
console.log("gradients usable:", ok.length);
'

cp "$TMP/gradients.json" web/vendor/uigradients/gradients.json
cp "$TMP/LICENSE.md" web/vendor/uigradients/LICENSE
{
  echo "ghosh/uiGradients"
  echo "commit ${COMMIT}"
  echo "file gradients.json"
  echo "vendored $(date -u +%Y-%m-%d)"
} > web/vendor/uigradients/VERSION

rm -rf .vendor-tmp
echo "vendor-uigradients: ok -> web/vendor/uigradients"
