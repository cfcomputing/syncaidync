# Phase 0: Foundation

## WO-00: Repo scaffold and checks

**Goal.** An empty but real project: it builds, the checks run, and the rules are written down
where the next agent will find them.

**Depends on.** Nothing.

**Read first.** `00-ground-rules.md` (all), `02-architecture.md` sections 2, 3, 15.

**Build.**

1. `Cargo.toml` (workspace):

   ```toml
   [workspace]
   resolver = "2"
   members = ["crates/syncaidync-core", "crates/syncaidync"]

   [workspace.package]
   version = "0.1.0"
   edition = "2021"
   license = "MIT"
   repository = "https://github.com/cfcomputing/syncaidync"

   [profile.release]
   lto = "thin"
   strip = true
   ```

2. `crates/syncaidync-core`: `Cargo.toml` with the dependencies from the architecture doc,
   `src/lib.rs` with a crate doc comment and one placeholder test.
3. `crates/syncaidync`: `Cargo.toml` with the dependencies from the architecture doc (all of
   them, now, so the lockfile and the licence check are settled from day one), `build.rs` that
   prints `cargo:rerun-if-changed=../../web`, and `src/main.rs` that handles `--version`
   (prints `syncaidync 0.1.0`) and `--help`, and otherwise prints "not built yet" and exits 1.
4. `web/index.html`: a valid page with the title "SyncAiDync" and one line of text. `web/test/`
   with one passing placeholder test (`smoke.test.js`).
5. `LICENSE`: the standard MIT licence text, first line
   `Copyright (c) 2026 World Sync Technologies, LLC`.
6. `README.md`: what it is (three sentences from `01-product.md` section 1), status ("early,
   being built"), requirements (Rust stable, git; optional: gh, node for UI tests), how to run
   (`cargo run`), how to check (`scripts/check.sh`), licence. No badges, no screenshots yet.
7. `AGENTS.md`, at most 60 lines: what the project is; "read `docs/plan/00-ground-rules.md`
   before changing anything"; the stack in five bullets; the check command; the words table
   pointer; the never-bend security rules as a short list.
8. `THIRD_PARTY_NOTICES.md`: a heading and a table (name, version, licence, where it lives)
   that is empty for now, with a sentence saying Rust crate licences are checked by
   `scripts/license-check.sh`.
9. `scripts/license-check.sh` (this exact logic was run against the dependency set and passed):

   ```sh
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
   ```

   The two workspace crates print as `MIT | syncaidync ...`, so they pass.
10. `scripts/check.sh`: `set -eu`, `cd` to the repo root, then run in order and stop at the
    first failure: `cargo fmt --all --check`,
    `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
    `node --test "web/test/*.test.js"` (if `node` is missing, print a clear warning and
    continue), `scripts/license-check.sh`. Print `check: all green` at the end.
11. `scripts/dev.sh`: runs
    `SYNCAIDYNC_WEB_DIR="$PWD/web" SYNCAIDYNC_DEV_SHELL=1 cargo run -- serve "$@"`.
12. Make the scripts executable. Keep the existing `.gitignore`; do not remove its `.local/`
    line.

**Tests.** The placeholder tests pass. `scripts/check.sh` is green. Prove the licence check can
fail without adding a crate: run it once with `MIT OR Apache-2\.0` removed from `ALLOW`, see it
list crates and exit 1, then revert.

**Done when.** A fresh clone runs `scripts/check.sh` green; `cargo run -- --version` prints the
version; `Cargo.lock` is committed.

**Not in this order.** Any server or UI behaviour. CI configuration (WO-20).

## Field notes
