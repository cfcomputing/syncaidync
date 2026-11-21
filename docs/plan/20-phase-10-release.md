# Phase 10: Release

## WO-20: Release pass

**Goal.** The repo is ready to be made public: it reads well, the checks run in CI, and a
stranger can install and use it from the README.

**Depends on.** Everything else.

**Read first.** `00-ground-rules.md` (all); `01-product.md` (to check the README against it).

**Build.**

1. **Whole-app pass.** With two or three real sessions, go through every section of
   `01-product.md` and tick it off. Fix small gaps; list larger ones in the report instead of
   starting new features. Check both appearances and reduced motion.
2. **Audit the rules.**
   - `scripts/check.sh` green from a fresh clone.
   - Licences: `scripts/license-check.sh`; every folder under `web/vendor/` has its licence
     and `VERSION`; `THIRD_PARTY_NOTICES.md` lists each with version and licence; no file in
     `web/` loads anything from another host (search for `http://` and `https://` in `web/`
     outside `vendor/` and explain each hit).
   - Security: re-run the Host, Origin, credential and file-containment tests; confirm the
     server refuses to start on a non-loopback address (there should be no way to ask for one);
     confirm the launch key appears in no log file and no saved file
     (`grep -r` the home folder for the key after a run).
   - No `unwrap()`/`expect()` on fallible input paths (search and justify what remains).
   - No leftover placeholders: search for `TODO`, `FIXME`, `XXX`, "not built yet".
   - If `.local/originality-gate.sh` exists, both modes print `clean`.
3. **README.md**, rewritten for a newcomer:
   - What it is, in three sentences, and one screenshot of the board (night) and one of a
     workspace (store them in `docs/img/`, PNG, at most 400 KiB each; use made-up session
     titles and a scratch repo, nothing private).
   - Requirements; install (`cargo install --git … --locked syncaidync`); run (`syncaidync`).
   - Supported agents and what "approximate" state means for the ones without hooks.
   - A short tour: board, Your move, workspace, Changes, pull requests, close-out.
   - Keyboard shortcuts table.
   - Configuration: the environment variables.
   - Security model in five bullets (loopback only, launch key, what agents can call back,
     file containment, what leaves your machine: nothing except what the agent CLIs, `git` and
     `gh` do themselves, plus the daily release check).
   - The macOS app.
   - Development: layout, `scripts/dev.sh`, `scripts/check.sh`, how the UI is served from disk.
   - Licence and third-party notices.
4. **AGENTS.md** refreshed to match what was built (still short).
5. **CONTRIBUTING.md**: how to run the checks; the MIT-only dependency rule and how the check
   enforces it; "all contributions must be your own work, written for this project".
6. **SECURITY.md**: how to report a problem privately (GitHub's private vulnerability
   reporting on the repo), and the scope (a local tool; the threat it defends against is other
   web pages and other local users reaching the server).
7. **CI:** `.github/workflows/check.yml` running `scripts/check.sh` on `macos-latest` and
   `ubuntu-latest` for pushes and pull requests, with the Rust toolchain installed by
   `rustup` commands in a `run:` step and Node from the runner image. Use only
   `actions/checkout` (MIT) as a third-party action; pin it to a commit.
   PTY tests must pass on both runners; if a runner cannot open a PTY, mark those tests to skip
   with a printed reason rather than failing silently.
8. **CHANGELOG.md** with a `0.1.0` entry listing the features, in the product's words.
9. **Version:** confirm `0.1.0` in `Cargo.toml` matches `--version`, Meta, and the mac
   `Info.plist`. Do not tag or publish; Chris does that.

**Tests.** No new ones. Everything existing is green on both platforms.

**Done when.** Someone who has never seen the project can go from the README to a running
session with an installed agent in under five minutes, and Chris has a short list of anything
left open.

**Not in this order.** Publishing to crates.io, Homebrew, signing, a website.

## Field notes

- The local check suite covers Host, Origin, credentials, and workspace file containment. The macOS wrapper now keeps the launch URL out of the server log, and agent and Git subprocesses do not inherit the launch key.
- The CI workflow covers macOS and Linux. Hosted CI still needs a push. Chris’s initial real-session tests look good; deeper real-PR / day-long macOS checks can stay on his list. This work has not been tagged or published.
- Outside `web/vendor/`, the only `http(s)://` hits in `web/` are SVG `xmlns` attributes and a `https://github.com/` filter in `session-links.js`.
- Launch-key audit: after a live serve into a temp `SYNCAIDYNC_HOME`, `rg` found the 64-hex key in neither the home folder nor the server log (0 hits). Bind is hard-coded to `127.0.0.1`.
- Reduced motion: theme cycling skips when `prefers-reduced-motion: reduce`; pulse and dialog animations are gated on `no-preference`; `base.css` now disables animation/transition globally under `reduce`.
- `unwrap`/`expect` in production paths: only `ids.rs` `expect("os random")` on CSPRNG failure, plus a `chars().next().unwrap()` in ANSI stripping after a byte index that is known to start a character. Everything else is under `#[cfg(test)]`.
- Leftover `TODO` search: one hit in vendored `web/vendor/xterm/xterm.css` only. Version is `0.1.0` across Cargo, `--version`, and `Info.plist`. README screenshots are in `docs/img/`.
