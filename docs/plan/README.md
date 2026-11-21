# SyncAiDync build plan

SyncAiDync is one local board for AI coding agents. It starts Claude Code, Codex, Pi and Cursor
Agent sessions in real terminals, shows which ones need you, and gives each session a workspace
with a terminal, a file editor, a change review and a close-out step.

This folder is the whole spec. It is written so that one work order can be handed to one agent
with no other context.

## Read in this order

| File | What it holds |
|---|---|
| [00-ground-rules.md](00-ground-rules.md) | Rules every work order follows: originality, licence, stack, names, definition of done |
| [01-product.md](01-product.md) | What the tool does, screen by screen, in the product's own words |
| [02-architecture.md](02-architecture.md) | Repo layout, dependencies, data shapes, HTTP API, terminal socket, storage, security |
| [03-design-system.md](03-design-system.md) | Tokens, components, panel drag/resize/tab behaviour, brand mark |
| [04-agent-adapters.md](04-agent-adapters.md) | How each agent CLI is started, resumed, observed and asked one-shot questions |
| `10-` to `20-` | The work orders, grouped by phase |

## Handing out a work order

Paste this, with the work order filled in:

> Read `docs/plan/README.md` and `docs/plan/00-ground-rules.md`. If `.local/HANDOFF.md` exists,
> read it before anything else. Then do work order **WO-NN** in `docs/plan/<file>`, reading the
> sections of `01`–`04` it points to. Do only that work order. Finish with `scripts/check.sh`
> green and the report the ground rules ask for.

## Work orders

Mark the status column as work lands: `todo`, `doing`, `done`. Add a one-line note when something
was deferred.

| WO | Title | File | Needs | Lane | Status |
|---|---|---|---|---|---|
| WO-00 | Repo scaffold and checks | [10](10-phase-0-foundation.md) | — | — | done |
| WO-01 | Core model and signal reducer | [11](11-phase-1-server-spine.md) | 00 | server | done |
| WO-02 | HTTP spine: auth, static files, event stream | [11](11-phase-1-server-spine.md) | 00 | server | done |
| WO-03 | Terminals, session registry, storage | [11](11-phase-1-server-spine.md) | 01, 02 | server | done |
| WO-04 | Web spine: tokens, shell, terminal element | [12](12-phase-2-web-spine.md) | 02 (03 to see a live terminal) | web | done |
| WO-05 | Agent adapters and callbacks | [13](13-phase-3-agents.md) | 03 | server | done |
| WO-06 | Workspaces, repo discovery, new-session dialog | [13](13-phase-3-agents.md) | 03, 04 | both | done |
| WO-07 | Session card and strip | [14](14-phase-4-board.md) | 04, 06 | web | done |
| WO-08 | Session canvas: rows, grid, free placement | [14](14-phase-4-board.md) | 07 | web | done (includes session archiving) |
| WO-09 | Your-move queue, snooze, alerts, settings, themes | [14](14-phase-4-board.md) | 07 | web | done |
| WO-10 | Workspace view and tool panels | [15](15-phase-5-workspace.md) | 08 | web | done |
| WO-11 | Files: tree and editor | [15](15-phase-5-workspace.md) | 03, 10 | both | done |
| WO-12 | Changes: diff parser, change map, endpoint | [16](16-phase-6-changes.md) | 03 | server | done |
| WO-13 | Changes tool UI | [16](16-phase-6-changes.md) | 10, 12 | web | done |
| WO-14 | One-shot runner and read-through plan | [16](16-phase-6-changes.md) | 05, 12, 13 | both | done |
| WO-15 | Pull requests: server | [17](17-phase-7-pull-requests.md) | 12, 14 | server | done |
| WO-16 | Pull requests: UI | [17](17-phase-7-pull-requests.md) | 13, 15 | web | done |
| WO-17 | Session tool, takeaways, close-out | [18](18-phase-8-close-out.md) | 10, 14 | both | done |
| WO-18 | History import, session links, update check | [18](18-phase-8-close-out.md) | 06 | both | done |
| WO-19 | macOS app | [19](19-phase-9-mac-app.md) | 09 | mac | done |
| WO-21 | Goals: server | [21](21-phase-11-goals.md) | 05, 12, 14, 17 | server | done |
| WO-22 | Goals: UI | [21](21-phase-11-goals.md) | 21, 07, 09, 17 | web | done (real-CLI manual pass pending) |
| WO-23 | Rich markdown view | [22](22-phase-12-rich-markdown.md) | 11, 13 | both | done |
| WO-24 | Margins | [23](23-phase-13-margins.md) | 23 | both | done |
| WO-25 | Check stages and the reviewer | [21](21-phase-11-goals.md) | 21 | server | done |
| WO-26 | Goal pipeline view and a goal-aware queue | [21](21-phase-11-goals.md) | 22 | web | done |
| WO-27 | Finishing a goal | [21](21-phase-11-goals.md) | 21, 26 | both | done |
| WO-20 | Release pass | [20](20-phase-10-release.md) | all, including 21–27 | — | doing |

## What can run side by side

- After WO-00: WO-01 and WO-02 together.
- After WO-03: the server lane (WO-05, WO-12) and the web lane (WO-04, then WO-07 to WO-10) do
  not touch the same files. The contract between them is `02-architecture.md`. Neither lane
  changes that file without the other knowing.
- WO-18 and WO-19 are independent of the review work and can be done any time after their needs.

One work order, one agent, one branch. Merge in table order when two are ready at once.

## If the spec and reality disagree

The agent CLIs change often. Where a work order says **Verify**, check the CLI on this machine
(`--help`, a real run) before writing code. If what you find differs from this plan, follow
reality, and add a dated note under "Field notes" in the work order's file saying what changed.
Do not silently work around it.
