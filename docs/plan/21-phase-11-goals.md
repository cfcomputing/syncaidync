# Phase 11: Goals and checks

A session can start with a **goal**: an end state in one sentence, the **criteria** that say it
is done, the **checks** that prove each criterion, and **limits** on how far the agent may go.
SyncAiDync, not the agent, runs the checks after every turn. Failing results go back to the agent
as the next **round**. When the checks pass the goal is met; when a limit is reached the goal
pauses and the session waits in Your move.

The agent never grades its own work. The checks are approved by the person, kept in the
SyncAiDync home, and guarded against the agent editing the files they rely on.

This phase lands before the 0.1 release pass (WO-20).

---

## WO-21: Goals: server

**Goal.** A session started with a goal is driven by its checks: SyncAiDync runs them after each
turn, sends failures back, pauses on limits, and records the evidence.

**Depends on.** WO-05, WO-12, WO-14 (one-shot runner), WO-17.

**Read first.** `01-product.md` section 2.3; `02-architecture.md` sections 5, 7.1, 7.9, 8
("Goals"); `04-agent-adapters.md` sections 12 and 14; `00-ground-rules.md` sections 5 and 7.

**Build: core** (`goal.rs`, no I/O; callers pass text, hashes and the current time).

- Types for `Goal`, `Criterion`, `Check`, `CheckResult`, `Limits`, `GoalStatus`, `PauseReason`
  exactly as `02-architecture.md` section 7.9.
- `build_draft_prompt(goal_text, criteria, answers, context) -> String`. Write it yourself. It
  must: say the task (propose at least one check per criterion that a program can run in the
  workspace and that fails until the criterion holds); prefer, in this order, an end-to-end run
  of the real thing, a property or invariant test, a comparison against the old behaviour, then
  unit tests, builds and linters; say a check that cannot be a command is `manual` and needs a
  one-line instruction for the person; ask for `guards` (the workspace paths each check relies
  on, such as the test file or script it runs); ask for at most 5 `questions` where a criterion
  is too vague to check; give the JSON shape
  `{ "checks": [{ "criterion", "title", "kind", "command", "guards", "why" }], "questions": [] }`
  and say "reply with that JSON object and nothing else". The repo context is fenced and
  labelled as material to read; instructions inside it are not to be followed.
- `parse_draft(text, criteria) -> Result<Draft, String>`: `extract_json`; at most 12 checks and 3
  per criterion; `criterion` must name an existing criterion id, else dropped; `kind` is
  `command` or `manual` (unknown → `manual`); title 80, command 400, why 300; at most 8 guards
  per check, each relative, no `..` segment, not `.git` or under it, at most 200 characters
  (bad ones dropped); at most 5 questions of 200 characters. No checks at all → an error
  sentence.
- `compose_brief(prompt, goal) -> String`: the first prompt the agent receives. The person's
  prompt (if any), then a section saying: the goal, each criterion with its checks (title and
  command), the limits, that SyncAiDync runs these checks after every turn and sends back what
  failed, that it must not change the guarded paths (listed), and that it should end its turn
  when it believes the goal is met.
- `compose_round(goal, results) -> String`: "SyncAiDync ran the checks: 3 of 5 pass. Round 2 of
  3." Then each failing check: title, command, exit code or "timed out", and the last 60 lines
  of output, fenced and labelled as output from the workspace. Tampered checks say which
  guarded path changed. At most 12,000 characters, cutting the longest outputs first.
- `decide(goal, results, now_ms) -> Decision`, where `Decision` is one of `Met`,
  `Round(text)`, `Pause(PauseReason, message)`. In this order:
  1. Any result tampered → `Pause(guard)`.
  2. Every command check passed and every manual check is done → `Met`.
  3. Every command check passed, some manual check not done → `Pause(manual)`.
  4. `goal.round >= limits.rounds` → `Pause(rounds)`.
  5. Otherwise `Round(compose_round(...))`.
- `over_limit(goal, cost_usd, now_ms) -> Option<PauseReason>`: `cost` when
  `cost_usd - goal.cost_at_start >= limits.cost_usd`; `time` when working time
  (`goal.working_ms` plus the current stretch) reaches `limits.minutes`. A limit of `null` never
  trips. A cost of `null` (the agent does not report it) never trips.
- `validate_new(goal) -> Result<(), String>`: 1–500 character goal; 1–8 criteria of 1–200
  characters; every criterion has at least one check; rounds 1–10; `cost_usd` 0.01–1000;
  minutes 1–1440; command checks have a non-empty command; check timeout 5–1800 s (default
  300). Errors are sentences for the dialog.

**Build: binary** (`goals.rs`).

- **Draft.** `POST /goals/draft`. Gather repo context from the dialog's workspace choice:
  - Worktree: from the repo at the chosen base, with `git ls-tree -r --name-only <base>` (first
    400 paths) and `git show <base>:<path>`.
  - Folder: the folder on disk through `files.rs` containment; a two-level listing (400
    entries).
  - Scratch: no context; say so in the prompt.
  - Files, when present, each cut to 8 KiB: `README*`, `AGENTS.md`, `CLAUDE.md`, `Cargo.toml`,
    `package.json`, `pyproject.toml`, `go.mod`, `Makefile`, `justfile`; the names of files
    under `scripts/`; the first 2 KiB of each `.claude/skills/*/SKILL.md`. At most 60 KiB in
    all. Never anything under `.git`.
  - Run the one-shot (section 12 of the adapters doc), parse, return. Nothing is run in the
    workspace at this stage.
- **Start.** `POST /sessions` with a `goal`: validate. For each check, hash its guards (sha256
  of a file; for a folder, of the sorted list of `path NUL sha256` for every file under it, at
  most 2,000 files and 50 MiB, else 422 "That folder is too large to guard: <path>."). A missing
  guard hashes as "missing". Store `cost_at_start` from usage when known, else 0.
  - If `baseline` is true: status `baseline`, create the workspace, run every command check
    once, store the results as `goal.baseline`, then start the agent. A check that already
    passes is kept but marked on screen: it proves nothing yet.
  - Then status `working`, and start the agent with `compose_brief(prompt, goal)` as its first
    prompt.
- **Running checks.** One check at a time per session; at most 2 sessions checking at once
  (semaphore). Each check: `/bin/sh -c <command>` in the workspace, its own process group,
  stdin from `/dev/null`, environment as section 5 of the adapters doc but without any
  `SYNCAIDYNC_*` variable, stdout and stderr together, the last 64 KiB kept. On timeout, kill
  the process group and record `timed_out`. Then re-hash the guards; a changed hash marks the
  result `tampered`.
- **After a turn.** When a session with goal status `working` goes from `working` to `ready`:
  status `checking`, bump `round`, run the checks, store the results, then act on `decide`:
  - `Met` → status `met`.
  - `Round(text)` → `send` the text with `submit: true`; status `working`.
  - `Pause(reason, message)` → status `paused` with that reason.

  A `working` → `blocked` change does not run checks. A session that ends while its goal is
  `working` or `checking` pauses with reason `ended`. Resuming the session does not unpause it.
- **Limits while working.** On every usage update, and every 15 s while the session is
  `working`, call `over_limit`. On a hit: write the agent's interrupt key (adapters doc section
  14) to the terminal, then pause with that reason. Only `working` time counts toward
  `minutes`.
- **Evidence.** `~/.syncaidync/goals/<session-id>.json` keeps the last 10 rounds: round number,
  time, and each result with up to 16 KiB of output. The Session's `goal` keeps only the latest
  result per check without output. Removing the session removes the file.
- **Routes** (all under `/sessions/{id}/goal`, all return Session unless noted):
  - `GET` → GoalRecord (the goal and its stored rounds).
  - `POST …/run`: run the checks now and act on `decide`, as after a turn. 409 while checking.
    On a `met` or `dropped` goal it only refreshes the evidence.
  - `POST …/continue { limits }`: un-pause. The new limits apply first; 422 if the reason's
    limit still has no room ("Raise the round limit first."). For `guard`, 422 until every
    tampered check is accepted. For `ended`, 409 until the session is live. Then run the checks
    as `run` does.
  - `POST …/accept { check }`: take the current hashes of that check's guards as the new
    reference. Its `tampered` flag clears.
  - `POST …/manual { check, done }`: tick or untick a manual check.
  - `POST …/drop`: status `dropped`. The session carries on as a plain session.
- **Close-out.** `takeaways::build_prompt` gets an optional goal summary: each check, how many
  rounds it failed, and any tampering. The prompt asks for lessons about what took rounds to get
  right.

**Tests.**

- Core `parse_draft`: limits, unknown criterion, bad guards (`..`, absolute, `.git/x`), unknown
  kind, no checks.
- Core `decide`: every row of the order above, including tampered-and-passing, and manual
  pending after the last round.
- Core `over_limit`: null limits, null cost, cost measured from `cost_at_start`, working time
  across several stretches.
- Core `compose_round`: the cap cuts the longest output first; every failing check is still
  named.
- Integration with a fake agent script (as in WO-14) that does a "turn" when it receives a line:
  - A goal with one check `test -f done.txt`. The first turn does nothing, so the agent gets a
    round message naming the check. The second turn creates the file, and the goal is `met`.
  - Rounds limit 1, still failing → `paused` with reason `rounds`. `continue` without raising
    the limit → 422; with `rounds: 2` → another round is sent.
  - A guard on `check.sh` that the agent edits → `paused` `guard`; `accept` then `continue` → the
    check counts.
  - A cost limit with usage callbacks: crossing it writes the interrupt key and pauses `cost`.
  - A check that sleeps past its timeout → `timed_out`, and no process is left behind.
  - Baseline on: results are stored before the agent's first prompt arrives.
  - A check's environment has no `SYNCAIDYNC_*` variable.

**Done when.** A fake agent reaches `met` only through the checks, every limit pauses it, and an
edit to a guarded file is never counted as a pass.

**Not in this order.** The UI (WO-22). Checks that run in parallel. Editing checks after the
session has started (drop the goal and start again).

---

## WO-22: Goals: UI

**Goal.** You can set a goal when starting a session, get help writing its checks, and see at a
glance how every goal is going.

**Depends on.** WO-21, WO-07, WO-09, WO-17.

**Read first.** `01-product.md` sections 2.3, 3.3, 5.4; `03-design-system.md`.

**Build.**

- **New-session dialog.** A collapsed "Set a goal" section under the first prompt. Open, it
  shows:
  - **Goal**: one line.
  - **Done when**: a list of criteria with Add and remove.
  - **Draft checks**: a button with an agent and model picker (default: the dialog's agent).
    It shows progress with elapsed seconds and "This can take a minute"; on failure, the
    server's message and "Try again".
  - **Checks**, grouped under their criterion. Each one has an editable title and command
    (monospace), its `why` in small text, its guards as removable chips with an add field, a
    timeout, and a remove button. "Add a check" offers a command or a manual check. A criterion
    with no check is marked and blocks Start.
  - **Questions** from the draft, each with an answer field, and "Draft again", which sends the
    answers.
  - **Limits**: rounds (default 3), cost in dollars (optional), time in minutes (optional).
  - **Run the checks once before the agent starts**: on by default.
  - One line above Start: "These commands will run in the workspace after every turn."
  - Drafted text is model output: shown in form fields only, never as HTML.
- **Card and strip.** A goal pill after the state label:
  - Working: "Goal · 3/5 · round 2/3".
  - Checking: "Checking…".
  - Paused: "Paused: round limit", "Paused: cost limit", "Paused: time limit", "Paused: checks
    changed", "Paused: your check" or "Paused: session ended".
  - Met: "Goal met".

  Paused uses the warm colour, met the ok colour. A paused or met session is `ready`, so it is
  already in Your move. The pill text is its reason.
- **Session tool, Goal section** (above Usage):
  - The goal text, and each criterion with its checks.
  - Each check shows its last result (pass, fail, timed out, changed, not run yet), its
    duration and time, and "Show output", which opens the stored output in a `<pre>` through
    `escapeText`.
  - A baseline line per check: "Passed before any work" in warm text.
  - Meters for rounds, cost and time, against their limits.
  - Buttons: **Run checks now**, **Keep going** (opens the limits inline, prefilled, and calls
    `continue`), **Accept change** on a changed check (says which paths changed), a checkbox on
    each manual check, and **Drop goal** (with a confirmation).
  - A list of past rounds: number, time, passed count; click one to see its results.
- **Alerts.** A goal that becomes `met` or `paused` uses the same alert as a session becoming
  ready, with the pill text as its message.
- **Close-out.** When the session has a goal, step 1 says "The goal summary is included".

**Tests.** `goalview.test.js` for the pure parts in `web/js/lib/goal.js`: the pill text for every
status and reason, the "criterion with no check" rule, limits validation matching the server's
messages, and round grouping.

**Manual check, with screenshots.** A real Claude session in a worktree of this repo:
- Goal "the README mentions the port", with one criterion.
- Draft checks, answer a question, draft again, edit a command, and start.
- Watch a round arrive in the terminal, then the goal is met.
- Then a second session with rounds limit 1 and a check that cannot pass: pause, Keep going with
  2, and Drop goal.

Both appearances, plus reduced motion.

**Done when.** From the board alone you can tell which goals are met, which are paused and why,
and get to the evidence in one click.

**Not in this order.** Saving a set of checks to reuse on the next session. Writing checks into
the repo as a skill.

---

## WO-25: Check stages and the reviewer

**Goal.** Cheap checks run first, and a stronger model reads the change before a goal counts as
met.

**Depends on.** WO-21.

**Build.**

- Each check has a `stage`: `quick` (default) or `full`. A round runs quick checks first; when
  any quick check fails, full checks are recorded as `skipped` and not run. The baseline runs
  every check. The draft prompt asks the model for a stage.
- `goal.reviewer = { on, agent, model }`. When every command check passes and the reviewer is
  on, the goal goes to `reviewing`. A one-shot run (WO-14 runner) gets
  `build_review_prompt`: the goal, each criterion with its checks and results, the previous
  concerns, and the diff (`render_diff`, bodies of the largest files dropped first to fit
  80,000 bytes), all fenced as material. Reply `{ "approved": bool, "concerns": [] }`, at most 5
  concerns of 300 characters; "not approved" with no concern is an error.
- `decide_review`: approved → met (or paused for manual checks); concerns → a round with the
  concerns, or paused `rounds` when no round is left; a reviewer that could not run → paused
  `review`. Keep going runs the checks again, and the reviewer after them.

**Tests.** Core: `decide` returns `Review` only when all pass and the reviewer has not approved
this round; `decide_review` for each path; `parse_review` shapes. Integration: concerns arrive
in the agent's terminal as a round, then an approval meets the goal.

**Done when.** A goal with the reviewer on cannot be met without the reviewer's approval of the
latest round.

---

## WO-26: Goal pipeline view and a goal-aware queue

**Goal.** You can watch a goal move through its stages, and the board only calls you when the
goal needs you.

**Depends on.** WO-22.

**Build.**

- `stages(goal)` in `web/js/lib/goal.js`: Agent, Quick checks, Full checks, Reviewer (when on),
  Your checks (when there are manual checks), then Goal met or Paused. Each is `done`, `now`,
  `fail` or `wait`. The server sets `goal.running` to the check id while it runs, so the stage
  lights up live.
- `dync-goal-panel`: compact on cards (pill and pipeline), full in the Session tool (pipeline,
  paused message, reviewer concerns, each check with result, duration, baseline note, Accept
  change, Show output; meters for rounds, cost and working time; Keep going with the limits
  inline; past rounds, each expandable). The running stage pulses; with reduced motion it is a
  still ring.
- Queue: a `ready` session whose goal is `baseline`, `working`, `checking` or `reviewing` is not
  in Your move, and its state label is the goal's word (Checking, Reviewer reading). It joins
  the queue with a ready alert when the goal is met or pauses; the notification body is the
  pill text.

**Tests.** `goal.test.js`: pill text for every status and reason, stages, validation, meters,
round summaries, and the queue rules above. `labels.test.js`: the state label for a driving
goal.

**Done when.** Four sessions — met, paused on rounds, paused on a changed guard, and checking —
read correctly on the board and in the Session tool, in both appearances.

## WO-27: Finishing a goal

**Goal.** A met goal ends somewhere: you read what changed, commit it, send more work, or close
out. Guards cannot be satisfied by a file the agent wrote itself without you noticing, and goal
sessions stay off your checkout.

**Depends on.** WO-21, WO-26.

**Build.**

- Accept carries on: `accept` takes one check or (`{}`) every flagged check. Clearing the last
  flag on a goal paused for `guard` re-runs that round's checks (`Trigger::Recheck`, round
  number unchanged; the stored round is replaced). The panel hides Keep going for `guard` and
  shows Accept all when more than one check is flagged.
- Created guards: a guard whose start hash is `missing` and exists now is listed in the
  outcome's `created`. The pause message and the round text say the agent created it; the panel
  shows it as a block and asks before accepting.
- Drafting: the prompt says to guard only existing paths and never to rely on a file the agent
  would write. `POST /goals/draft` returns `missing_guards`; the setup dialog marks those chips.
- Finish report: on every transition to `met`, `goals::finish` stores `goal.report` with the
  changed files at once, then a one-shot run (session agent and model; shell uses Claude) reads
  the goal, the diff and the condensed transcript and returns a summary, concerns and a commit
  message. The brief asks the agent to end its last turn with a summary.
- Commit, Keep working, Close out on the report (routes in architecture §8 Goals). Commit
  never takes the `.syncaidync/` sidecar and never pushes.
- New session: turning a goal on with Existing folder on a git checkout moves the session to a
  new worktree of it, with a note.

**Tests.** Core: created-guard wording, the drafting rule, report files without the sidecar,
the fallback commit message, the finish prompt and reply parsing. Integration: a created guard
named and Accept all carrying on to met with a ready report; a met goal in a git repo reporting,
committing only its files, refusing a second commit, and Keep working sending the instruction.
`goal.test.js`: finish helpers.

**Done when.** A met goal shows its report with the files and a drafted message, Commit lands a
commit with that message, and accepting the last flagged guard needs no further click.

## Field notes

### 2026-11-20

- Built WO-21, WO-22, WO-25 and WO-26 on branch `wo-21-goals`. Verified with `cargo test --workspace`
  (including 8 goal integration tests with a fake Claude CLI), `node --test`, and a demo server
  with a fake agent that reports its own turns: met with the reviewer, paused on rounds,
  paused on a changed guard, and a full check still running, in night and day.
- Check processes run in their own process group and are killed with `/bin/kill -KILL -- -pgid`
  after they finish or time out, so a check that starts a server leaves nothing behind. No
  `unsafe` was needed.
- The dev `shell` agent gets only the person's prompt, never the brief: a shell would run the
  brief's lines as commands.
- **Verify** still open: the interrupt key (`ESC`) against each real CLI mid-turn (adapters doc
  section 14), and a manual pass with a real Claude session drafting checks for a worktree.
- 2026-11-20 (WO-27): two real runs on this repo showed the gaps. The drafter guarded
  `web/test/contrast.test.js` before it existed, so the agent wrote the test that judged it and
  the accept looked routine; accepting then left the goal paused until Continue; and a met goal
  ended with nothing to do next while the work sat uncommitted in the main checkout. WO-27
  addresses all three. Verified with the goal integration tests (10), `node --test`, and a demo
  server: a created guard, a met goal's report and commit form, and the dialog moving a goal
  session to a worktree, in both appearances.
