# Phase 13: Margins

## WO-24: Line-tied margins beside the code

**Goal.** You can attach rich context (markdown, pictures, diagrams) to lines of code without
putting it in the shipped source. Agents get a short index and load a margin only when needed.

**Depends on.** WO-23.

**Read first.** `01-product.md` section 5.3; `02-architecture.md` section 8 (Margins);
`00-ground-rules.md` sections 5 and 7; `03-design-system.md` sections 1–2.

**Build: server.**

- `margins.rs`: ensure `.syncaidync/` with a default `.gitignore` (`*` / `!.gitignore`);
  read/write `margins/index.json` and `margins/entries/<id>.json`; regenerate
  `margins/INDEX.md` after every write or delete (title, path, lines, id — no body).
- Media under `margins/media/<id>/` with the same image types and 2 MiB cap as session media.
- API routes from the architecture table. Path containment: everything stays under
  `<workspace>/.syncaidync/margins/` and never touches `.git`.
- When rendering a margin body for the UI, rewrite `media/<name>` image srcs to the margin
  media URL (extend the markdown context or a dedicated helper).

**Build: UI.**

- Vendor CodeMirror: re-export `Decoration`, `gutter`, `GutterMarker`, `RangeSetBuilder`,
  `StateField` (still MIT-gated).
- `dync-tool-files`: load margins for the open path; gutter marks on anchored lines; click a
  mark to open the margin pane; "Add margin" uses the current selection (or cursor line) plus a
  quote. Pane: title, rendered body (View) / source edit, attach image, Save, Delete.
- Preference: pane open/closed is in-memory for the tool instance (no new prefs key required).

**Not in this order.** Mermaid rendering.

**Tests.**

- Unit: index round-trip; INDEX.md lists titles; delete removes entry and media folder;
  path escape refused.
- Integration: PUT then GET margin; media upload and fetch; INDEX.md present on disk.
- `node --test` for any pure helpers.

**Manual check.** Open a Rust file, add a margin with a short paragraph and a png, see the
gutter mark, reload the session, confirm the mark and body return. Confirm `.syncaidync/` is
ignored by git in a fresh repo.

**Done when.** `scripts/check.sh` is green and a margin never has to live in a source comment.

## Field notes

### 2026-11-20

- Sidecar under `<workspace>/.syncaidync/margins/` with default gitignore, `INDEX.md` for
  agents, JSON entries, and media attachments.
- Files tool: gutter marks (CodeMirror gutter re-exported), Add margin, side pane with
  View/Edit and image attach.
- Changes Diff: ¶ marks on new-side lines, peek pane; quote-based relocate (moved /
  outdated) in Files and Changes. Index entries carry `quote`.
- Mermaid is out: dependency tree is not MIT-only (Apache Chevrotain, ISC/BSD d3). Use
  attached SVG/PNG in margins for diagrams.
- Relative markdown links rewrite to `#dync-open:<path>` and open in Files View.
- Review notes in Changes render as markdown via `POST /markdown`.
- PR-clone margins: same sidecar on the pull's local clone; `/pulls/{id}/margins*` mirrors
  sessions; Changes Diff shows ¶ marks and peek for pull sources too.
- Re-anchoring: exact quote, whitespace-normalized match, then fuzzy similarity (moved vs
  outdated when ties or low score).
- Mermaid remains out (not MIT-only).
