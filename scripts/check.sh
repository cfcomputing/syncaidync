#!/bin/sh
# Repo health: format, lint, test, licences. Stop on the first failure.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$ROOT"

rust_checks() {
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
}

js_checks() {
  if ! command -v node >/dev/null 2>&1; then
    echo "check: node is required for web/test (install Node 20+)" >&2
    return 1
  fi
  # Expand the glob in the shell so older Node versions do not see a literal pattern.
  set -- web/test/*.test.js
  if [ ! -f "$1" ]; then
    echo "check: no web/test/*.test.js files found" >&2
    return 1
  fi
  node --test "$@"
}

rust_checks
js_checks
"$ROOT/scripts/license-check.sh"

echo "check: all green"
