# Phase 7: Pull requests

## WO-15: Pull requests: server

**Goal.** Open a GitHub pull request by reference, get its ChangeSet, keep a list, save notes as
a pending review, and start a session on it.

**Depends on.** WO-12, WO-14 (and WO-06 for worktrees and repo discovery).

**Read first.** `01-product.md` section 7; `02-architecture.md` sections 7.6, 8 ("Pull
requests"); `00-ground-rules.md` section 7.

**Build: core** (`pulls.rs`, pure).

- `parse_ref(text) -> Result<PullRef { owner, repo, number }, String>`. Accept
  `https://github.com/<owner>/<repo>/pull/<n>` (with or without a trailing path, query or
  fragment, `http` or `https`, optional `www.`) and `<owner>/<repo>#<n>`. Owner and repo may
  contain letters, digits, `-`, `_`, `.`. Anything else → a sentence saying what is accepted.
  Only `github.com`.
- `remote_matches(url, owner, repo) -> bool` for `https://github.com/o/r(.git)`,
  `git@github.com:o/r(.git)`, `ssh://git@github.com/o/r(.git)`, case-insensitive.
- `draft_payload(head_commit, review, change_set) -> serde_json::Value`: the body for creating
  a pending review:
  `{ "commit_id", "body", "comments": [{ "path", "line", "side", "start_line"?, "start_side"?, "body" }] }`
  with no `event` field (that is what makes it pending). `side` is `RIGHT` for new, `LEFT` for
  old. `start_line` only when the note covers more than one line. A note becomes a line comment
  only if every line of it lies inside a hunk of that file on that side in the ChangeSet;
  otherwise (file-level notes, outdated notes) it is added to `body` under a line naming the
  file. `body` starts with the overall note. Nothing to send → an error.

**Build: binary.**

- `gh.rs`: run `gh` with arguments as a list, a timeout (30 s), `GH_PROMPT_DISABLED=1`,
  `NO_COLOR=1`. Map "not installed" and "not signed in" (exit status and stderr from
  `gh auth status`) to 503 `unavailable` with "Install the GitHub CLI (gh) to use pull
  requests." or "The GitHub CLI is not signed in. Run: gh auth login".
- `pulls.rs`:
  - **open:** parse the ref; if already listed, return it. Otherwise
    `gh pr view <n> --repo <owner>/<repo> --json number,title,author,body,state,isDraft,baseRefName,headRefName,url`.
    Reuse a discovered clone with a matching remote. If none exists, use `gh repo clone` to
    create a reusable shallow clone of the base branch under the app home. Fetch (below), store,
    and publish a `pull` event.
  - **fetch:** in the clone,
    `git fetch --no-tags --no-write-fetch-head <remote> +refs/pull/<n>/head:refs/syncaidync/pull/<n>/head +refs/heads/<base>:refs/syncaidync/pull/<n>/base`.
    These private refs keep the commits alive without touching the user's branches.
    `head_commit` = the fetched head ref and `base_commit` = `git merge-base` of the two.
    Deepen an app-managed shallow clone when Git cannot find the merge base.
  - **changes:** add the `Commits { repo, base, head }` source to `changes.rs`: raw diff from
    `git diff … <base> <head>`; no untracked files; all paths from
    `git ls-tree -r --name-only -z <head>`; file text from `git show <head>:<path>`; importer
    candidates from `git grep -l -F -e <stem> <head>` (strip the `<head>:` prefix from
    results). Cache by head commit. `?refresh=1` re-runs `gh pr view` and the fetch first.
  - **review and plan:** same as sessions, under the key `p:<pull id>`. `progress` on the Pull
    is recomputed whenever the review or the ChangeSet changes.
  - **draft:** build the payload and run
    `gh api --method POST repos/<owner>/<repo>/pulls/<n>/reviews --input -` with the JSON on
    stdin. Return the review's `html_url`. Then mark the included notes sent. If GitHub says a
    pending review already exists, answer 409 "You already have a pending review on this pull
    request. Submit or discard it on GitHub, then try again." Other failures: 422 with GitHub's
    message.
  - **session:** `git worktree add -b dync/pr-<n> <home>/worktrees/<repo>/pr-<n> <head_commit>`
    (suffix the branch and folder if taken), then create a session there: `base_commit` is the
    pull's merge base, `pull` is set, the title is `PR #<n>: <title>`, and the first prompt is
    empty. Set the Pull's `session`. If the Pull already has a session that still exists, return
    that one.
  - **patch:** `status` only. **delete:** remove from the list, delete its review, and delete
    the two private refs (`git update-ref -d`). Sessions started from it are left alone.
- `markdown.rs`: the safe renderer from the architecture doc, used for `body_html`.
- `api/pulls.rs`: the routes. Store in `pulls.json`.

**Tests.**

- Core: `parse_ref` (every accepted form; rejects other hosts, missing numbers, extra `#`),
  `remote_matches`, `draft_payload` (single line, range, old side, file-level note folded into
  the body, a note outside every hunk folded into the body, nothing to send).
- `markdown.rs`: raw HTML dropped; `javascript:` and `data:` links neutralized; tables and task
  lists render; an image with an `https` source stays.
- Integration without GitHub: put fake `gh` on the `PATH` (a script the test writes that prints
  canned JSON for `pr view`, records its stdin for `api`, and can be told to fail), and use a
  local bare repository as the "remote" with a `refs/pull/7/head` ref made by the test. Then:
  open → Pull stored, private refs exist; changes → the expected files; refresh after pushing
  a new commit to the pull ref → new head; draft → the recorded payload has no `event` and the
  right comments; delete → refs gone. `gh` missing → 503 with the install sentence.
- A ref string with shell metacharacters is rejected by the parser and never reaches a command.

**Done when.** All of the above passes with the fake `gh`, and one real pull request on a repo
you can read has been opened by hand as a final check (say which one in the report, or say that
you could not).

**Not in this order.** The UI. Submitting reviews (always left to GitHub).

---

## WO-16: Pull requests: UI

**Goal.** The pull request list and review screen.

**Depends on.** WO-13, WO-15.

**Read first.** `01-product.md` section 7; `02-architecture.md` sections 7.6, 13.2.

**Build.**

- Top bar "Pull requests" button → `#/pulls`.
- `dync-pulls`: the list. A field at the top: "Paste a pull request URL or owner/repo#123" with
  an Open button (`Enter` works). Rows grouped Active then Done: `owner/repo #123`, title,
  author, state badge (Open, Draft, Merged, Closed), progress ("4 of 12 files"), and a menu
  (Mark done / Mark active, Remove). Clicking a row goes to `#/p/<id>`. Empty state explains
  what this is for in one sentence. Errors from Open appear under the field, in the server's
  words.
- `dync-pull`: the review screen for one pull.
  - Header: back to the list, title, `owner/repo #123` linking to GitHub (opens in a new tab,
    `rel="noopener noreferrer"`), author, state badge, `base ← head` branch names, "Refresh",
    "Mark done", "Start review session", "Save as draft review on GitHub".
  - The description: `body_html` inserted through `markdown.js`, collapsed to about eight lines
    with "Show more". Links inside it open in a new tab with `rel="noopener noreferrer"`; add
    those attributes after inserting.
  - Below: `dync-changes-view` with `source = { kind: "pull", id }`. For a pull source the view
    hides "Send notes to the agent" and the turn-finished prompt.
  - "Save as draft review on GitHub": a confirm dialog that says how many line comments and
    whether there is an overall note, and that the review stays pending until you submit it on
    GitHub. On success, a toast with "Open on GitHub". Disabled when there are no unsent notes.
  - "Start review session" picks the agent, model and autonomy, then opens the session
    workspace with Terminal and a wider Changes panel showing the pull request diff.
    "Open review session" opens the existing session in the same review layout.
- Pulls update live from `pull` and `pull_gone` events.

**Tests.** A pure helper `web/js/lib/pulls.js` with tests: `stateBadge(pull)`,
`progressLabel(progress)`, `sortPulls(pulls)` (active first, then most recently added).

**Manual check, with screenshots.** Open a real pull request; read with the map and a
read-through plan; add two line notes and an overall note; save as a draft review and confirm on
GitHub that it is pending, with the comments on the right lines; start a session on it; mark it
done; remove it. Try a pull request for a repo with no local clone and verify the managed clone, then try with `gh` signed out
(rename it on the `PATH` temporarily): both give clear messages.

**Done when.** Someone can review a pull request end to end without opening GitHub until it is
time to submit.

**Not in this order.** Listing your open pull requests automatically. Other git hosts.

## Field notes

### 2026-11-18

- WO-15: core `pulls` (`parse_ref`, `remote_matches`, `draft_payload`); `markdown`, `gh`,
  binary `pulls` (open/fetch/changes/review/plan/draft/session/patch/delete); `ChangeSource::Commits`;
  fake-`gh` integration tests. Manual real-PR check deferred (no signed-in `gh` in this pass).
- WO-16: `pulls.js` helpers; `dync-pulls` / `dync-pull`; top bar → `#/pulls`; changes view plan
  path for pulls; draft toast link. Manual real-PR UI pass deferred with WO-15.
