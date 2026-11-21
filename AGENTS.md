# Agents working in this repo

SyncAiDync is a local board for AI coding agent CLIs (Claude Code, Codex, Pi, Cursor Agent).
It owns their terminals, shows which sessions need you, and gives each one a workspace.

**Read [`docs/plan/00-ground-rules.md`](docs/plan/00-ground-rules.md) before changing anything.**
If `.local/HANDOFF.md` exists, read that first.

## Stack

- One Rust binary (`syncaidync`) serving a static web UI
- Pure logic in `syncaidync-core` (no I/O)
- Vanilla JS custom elements (`dync-*`), no framework, no build step
- MIT only: no TypeScript, htmx, axum, or non-MIT crates
- Optional macOS wrapper in `mac/`, using only Apple frameworks
- Work orders live in `docs/plan/`

## Check

```sh
scripts/check.sh
```

Use `mac/build.sh --build-only` to typecheck and build the macOS bundle.

## Words

Use the vocabulary in `docs/plan/00-ground-rules.md` section 5. Do not invent synonyms for
session states, the queue, snooze, changes, checkpoints, or takeaways.

## Security that never bends

- Bind `127.0.0.1` only
- Host header must be loopback; otherwise 403
- `/api/` and `/agent/` need a credential
- Launch key never on disk, never logged, never in a query string
- File access never leaves a session workspace and never touches `.git`
