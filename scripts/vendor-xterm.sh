#!/bin/sh
# Fetch MIT-licensed xterm.js and the fit addon into web/vendor/xterm/.
set -eu
cd "$(dirname "$0")/.."
TMP=".vendor-tmp/xterm"
rm -rf "$TMP" web/vendor/xterm
mkdir -p "$TMP" web/vendor/xterm
cd "$TMP"

npm pack @xterm/xterm@6.0.0 >/dev/null
npm pack @xterm/addon-fit@0.11.0 >/dev/null

XTERM_TGZ=$(ls xterm-xterm-*.tgz | head -1)
FIT_TGZ=$(ls xterm-addon-fit-*.tgz | head -1)

mkdir xterm-pkg
tar -xzf "$XTERM_TGZ" -C xterm-pkg --strip-components=1
lic=$(node -p "require('./xterm-pkg/package.json').license")
test "$lic" = "MIT" || { echo "xterm license is '$lic', expected MIT"; exit 1; }

mkdir fit-pkg
tar -xzf "$FIT_TGZ" -C fit-pkg --strip-components=1
lic=$(node -p "require('./fit-pkg/package.json').license")
test "$lic" = "MIT" || { echo "addon-fit license is '$lic', expected MIT"; exit 1; }

cd ../..
OUT=web/vendor/xterm
cp "$TMP/xterm-pkg/lib/xterm.mjs" "$OUT/xterm.mjs"
cp "$TMP/xterm-pkg/css/xterm.css" "$OUT/xterm.css"
cp "$TMP/xterm-pkg/LICENSE" "$OUT/LICENSE-xterm"
cp "$TMP/fit-pkg/lib/addon-fit.mjs" "$OUT/addon-fit.mjs"
cp "$TMP/fit-pkg/LICENSE" "$OUT/LICENSE-addon-fit"

# Fail if either module has a bare package import (must load in a browser as-is).
if grep -E "from ['\"][^./]" "$OUT/xterm.mjs" "$OUT/addon-fit.mjs" >/dev/null 2>&1; then
  echo "vendor-xterm: found a bare import; modules must be self-contained"
  grep -nE "from ['\"][^./]" "$OUT/xterm.mjs" "$OUT/addon-fit.mjs" || true
  exit 1
fi

# Fit addon may import from @xterm/xterm — rewrite to our local file if needed.
if grep -q "@xterm/xterm" "$OUT/addon-fit.mjs" 2>/dev/null; then
  # Replace with a relative import of our vendored xterm (or drop if unused at runtime).
  sed -i.bak "s|from '@xterm/xterm'|from './xterm.mjs'|g; s|from \"@xterm/xterm\"|from './xterm.mjs'|g" "$OUT/addon-fit.mjs"
  rm -f "$OUT/addon-fit.mjs.bak"
fi

{
  echo "@xterm/xterm 6.0.0"
  echo "@xterm/addon-fit 0.11.0"
  echo "vendored $(date -u +%Y-%m-%d)"
} > "$OUT/VERSION"

rm -rf .vendor-tmp
echo "vendor-xterm: ok -> $OUT"
