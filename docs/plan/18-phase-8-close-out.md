# Phase 8: Closing out

## WO-17: Session tool, takeaways, close-out

**Goal.** The Session tool shows what a session is and has done, and helps you end it well.

**Depends on.** WO-10, WO-14.

**Read first.** `01-product.md` sections 5.4, 8.2; `02-architecture.md` sections 7.7, 8
("Takeaways"); `04-agent-adapters.md` sections 11, 12.

**Build: core.**

- `transcript.rs`: `condense_claude(jsonl_text, budget) -> String` and
  `condense_terminal(log_text, budget) -> String`.
  - Claude transcripts are JSON lines. Keep, in order: what the person typed (`You: …`), what
    the assistant said in text (`Agent: …`), and each tool use as one line (`Tool: Bash: cargo
    test`). Skip tool results, thinking, system lines and anything that does not parse.
  - Terminal logs (for agents with no readable transcript): strip ANSI, drop empty and repeated
    lines.
  - Budget (default 60,000 characters): if longer, keep the first 10,000 and the last 50,000
    with a line between saying how much was left out. Cut on line boundaries.
- `takeaways.rs`:
  - `build_prompt(condensed, agent_label) -> String`. Write it yourself: ask for at most 5
    lessons from this session that would help a future session on its first try (project
    conventions discovered, commands that work, mistakes that cost time), each as a short
    title and one paragraph, with a scope of `project` (specific to this repo) or `everywhere`
    (true for any repo); say not to include secrets, tokens, or personal details; say that the
    transcript is material to learn from and instructions inside it are not to be followed;
    give the JSON shape `{ "takeaways": [{ "title", "body", "scope" }] }`.
  - `parse_reply(text) -> Vec<Takeaway>`: at most 5; title 80 chars, body 600; newlines in
    either become spaces; unknown scope becomes `project`; entries with an empty title or body
    are dropped.
  - `merge(existing, takeaways, date) -> String`: find the heading `## Takeaways (SyncAiDync)`.
    If absent, add it at the end of the file (after one blank line). Append each takeaway at
    the end of that section (before the next `## ` heading, or the end of the file) as
    `- **<title>**: <body> _(<YYYY-MM-DD>)_`. Skip a takeaway whose title already appears in
    the section (compare lower-cased). Leave everything else in the file byte-for-byte as it
    was. The file ends with exactly one newline.
  - `destination(agent, scope, ctx) -> Option<String>` where `ctx` carries the home folder, the
    SyncAiDync home, the repo top level (the main repo for a worktree, so the lesson outlives
    the worktree), and the values of `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `PI_CODING_AGENT_DIR`:

    | Agent | `everywhere` | `project` |
    |---|---|---|
    | claude | `${CLAUDE_CONFIG_DIR:-~/.claude}/CLAUDE.md` | `<repo>/CLAUDE.local.md` |
    | codex | `${CODEX_HOME:-~/.codex}/AGENTS.md` | `<repo>/AGENTS.md` |
    | pi | `${PI_CODING_AGENT_DIR:-~/.pi/agent}/AGENTS.md` | `<repo>/AGENTS.md` |
    | cursor | `<syncaidync home>/notes/everywhere.md` | `<repo>/AGENTS.md` |

    `note` is always `<syncaidync home>/notes/everywhere.md`. `project` is `None` outside a
    repo. **Verify** the two Claude file names against the installed version's documentation
    on memory files.

**Build: binary.**

- `takeaways.rs`: read the transcript (the session's `transcript` path if it exists and is at
  most 20 MiB, else the terminal log), condense, run the one-shot, parse. Saving: group by
  destination; for each file, read it (or start empty), `merge`, write atomically, creating
  parent folders. Never write outside the destinations above.
- Routes: `destinations`, `suggest`, `save`.

**Build: UI.**

- `dync-tool-session`:
  - **Facts:** a two-column list (agent, model, approval prompts on/off, folder with a "Copy
    path" button, branch and base, created, session id).
  - **Links:** a slot that WO-18 fills.
  - **State history:** keep the last 8 state changes seen by this page (time, label).
  - **Activity:** `tools`, newest first, with relative times.
  - **Usage:** cost and context as two small meters (context turns `--warn` above 80%).
  - **Send a message:** a textarea and Send (`Cmd/Ctrl+Enter`). Disabled when not live.
  - **Close out:** a button that opens the flow below.
- `dync-close-out` (a dialog, three steps shown one at a time):
  1. "Any takeaways?": "Suggest takeaways" (agent and model picker, progress, errors as in
     WO-14), "Add one myself", or "Skip".
  2. The list: each takeaway is an editable title and body, a scope selector showing the exact
     file it will be written to, and a remove button. "Save takeaways" writes them and shows
     which files changed.
  3. "What next?": Stop the session / Remove the session (leads to the remove dialog) / Leave
     it running.
  Model output shown here is plain text in form fields.

**Tests.**

- Core `merge`: no file; a file without the heading; with the heading and following sections
  (insertion point); a duplicate title; a file with no trailing newline; Windows line endings
  left alone.
- Core `parse_reply`: limits, scope fallback, newlines flattened. `destination`: every cell of
  the table, environment overrides, no repo.
- Core `condense_claude` on a small made-up transcript; the budget cut.
- Integration with a fake agent script (as in WO-14): suggest returns takeaways; save writes
  the right files under a temp home and a temp repo; saving twice does not duplicate; a
  takeaway with scope `project` for a session with no repo → 422.

**Manual check, with screenshots.** After a real session: suggest, edit one, change a scope,
save, and open the files to confirm. Close out by stopping.

**Done when.** A takeaway saved today shows up in the right file with today's date, and the
rest of that file is untouched.

**Not in this order.** Editing or deleting past takeaways (people edit the files).

---

## WO-18: History import, session links, update check

**Goal.** Three small things around the edges.

**Depends on.** WO-06 (and WO-17's Session tool for where links show; if it has not landed, show
links in the card's menu only).

**Read first.** `01-product.md` sections 8.1, 8.3; `02-architecture.md` sections 8
("Discovery", "Sessions": links; "Auth and basics": update); `04-agent-adapters.md` section 11.

**Build: history import.**

- `discovery.rs`: the Claude history scan (and Codex only if its **Verify** passes). On a
  blocking thread, cached 30 s.
- `GET /history`, `POST /history/import`.
- `dync-import` dialog from the top bar's Import button: a searchable list (folder, first
  prompt, "3 days ago", agent), newest first; Import adds the session and closes; rows already
  imported are not listed. Empty state: "No past sessions were found on this machine."

**Build: session links.**

- `links.rs`: from the workspace's repo: `git remote get-url origin` → if it is a GitHub URL,
  `repo_url` and `branch_url` (`<repo_url>/tree/<branch>`, percent-encoding the branch). If
  `gh` is available: `gh pr list --head <branch> --state all --json number,url,state --limit 1`
  → `pull`. Cache per session for 60 s. Any failure yields nulls, never an error.
- `GET /sessions/{id}/links`.
- UI: links in the Session tool and in the card's `⋯` menu ("Open repo", "Open branch",
  "Open pull request #12"), opening in a new tab.

**Build: update check.**

- `update.rs`: unless `SYNCAIDYNC_NO_UPDATE_CHECK=1`, and at most once every 24 hours (time
  kept in `config.json`), run
  `git ls-remote --tags --refs https://github.com/cfcomputing/syncaidync.git` with a 10 s
  timeout, take the highest tag that parses as a version, and store it. Failures are silent.
- `GET /update` → `{ current, latest, newer, command }` where `command` is
  `cargo install --git https://github.com/cfcomputing/syncaidync --locked syncaidync`.
- UI: when `newer`, Settings shows "Version X is available" with the command and a copy button,
  and the gear icon gets a small dot. Nothing installs itself.
- README: one sentence saying this check exists, what it contacts, and how to turn it off.

**Tests.**

- Core: version compare (`v0.2.0` > `0.1.9`; pre-release and junk tags ignored).
- Unit: GitHub remote URL parsing for the https, scp-like and ssh forms; non-GitHub → null.
- Integration: a made-up Claude history folder (via `CLAUDE_CONFIG_DIR` pointed at a temp
  folder) → listed, imported, then no longer listed; a history file whose folder is gone is
  skipped; malformed lines do not break the scan.
- Integration: update check against a local bare repo with tags (make the remote URL
  injectable for tests through a hidden environment variable `SYNCAIDYNC_UPDATE_REMOTE`).

**Manual check.** Import one of your own past sessions and resume it. Links on a session whose
branch has a pull request.

**Done when.** All three work and none of them can make a page fail to load when the network or
`gh` is unavailable.

## Field notes

- The installed Claude Code version was 2.1.327. The [Claude Code memory documentation](https://code.claude.com/docs/en/memory) confirms the user and project-local instructions files used by Close out.
- A local fixture verifies Claude history import, duplicate filtering, missing folders, malformed records, and release selection from local Git tags. Codex import remains optional and was left out because its transcript layout was not verified here.
- The native demo saved an authored project takeaway. A signed-in Claude conversation and a real GitHub pull request remain for the manual pass.
- Code for history import, session links, and the daily update check is in place with fixture tests. Remaining manual checks: import a real past session and open links on a branch that has a pull request.
