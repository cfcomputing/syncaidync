#!/bin/sh
# Fails if any crate in the dependency tree does not offer MIT.
set -eu
cd "$(dirname "$0")/.."
ALLOW='^(MIT|MIT OR Apache-2\.0|MIT/Apache-2\.0|Apache-2\.0 OR MIT|Apache-2\.0/MIT|Unlicense OR MIT|MIT OR Unlicense|Unlicense/MIT|BSD-2-Clause OR Apache-2\.0 OR MIT|Apache-2\.0 WITH LLVM-exception OR Apache-2\.0 OR MIT|MIT OR Apache-2\.0 OR Zlib|Zlib OR Apache-2\.0 OR MIT) \|'
# Approved exception: used only by procedural macros at build time, not linked into the binary.
EXCEPT='^\(MIT OR Apache-2\.0\) AND Unicode-3\.0 \| unicode-ident '
tree=$(cargo tree --locked --workspace --prefix none --format '{l} | {p}' -e normal,build | sed 's/ (\*)$//' | sort -u)
bad=$(printf '%s\n' "$tree" | grep -Ev "$ALLOW" | grep -Ev "$EXCEPT" || true)
if [ -n "$bad" ]; then
  echo "license-check: crates that do not offer MIT:"
  printf '%s\n' "$bad"
  exit 1
fi
echo "license-check: ok ($(printf '%s\n' "$tree" | wc -l | tr -d ' ') crates)"
