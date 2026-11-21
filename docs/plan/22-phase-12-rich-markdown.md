# Phase 12: Rich markdown view

## WO-23: View-first markdown in Files and Changes

**Goal.** Markdown opens as a beautiful rendered view first. Source and Diff remain one click away.
Relative images in the workspace show through a safe media route.

**Depends on.** WO-11, WO-13.

**Read first.** `01-product.md` sections 5.3 and 6.1; `02-architecture.md` sections 8
(Files), 11, 14 (Markdown); `00-ground-rules.md` sections 6–7; `03-design-system.md`
sections 1–2.

**Build: server.**

- Extend `markdown.rs`: keep `to_safe_html`; add a context-aware render that, given a session
  id and `base_path`, rewrites relative **image** destinations to
  `/api/v1/sessions/{id}/media?path=<resolved>`. Absolute `http(s)` / `mailto:` / `#` stay as
  today. Bad schemes and unresolvable relatives become `#`. Relative **links** are not rewritten
  to media (they stay `#` unless absolute-safe).
- `files.rs`: `read_media(root, rel) -> Result<(bytes, content_type), FileError>` for
  `png`, `jpeg`/`jpg`, `gif`, `webp`, `svg` only, at most 2 MiB, same `resolve` containment as
  text files (never leave the workspace, never `.git`).
- `POST /sessions/{id}/markdown` body `{ text, base_path }` → `{ html }`. `base_path` may be
  null; when set, relative images rewrite against that path’s folder.
- `GET /sessions/{id}/media?path=` returns the bytes with the right `Content-Type`.

**Build: UI.**

- `web/css/markdown.css`: shared `.md-body` prose (headings, lists, tables, blockquotes, code
  fences, task lists, images, hr) from design tokens. Link it from `index.html`. Use it for
  Files View, Changes markdown View, and the PR description body.
- `dync-tool-files`: for `.md` / `.markdown`, default **View**; segmented **View | Source**;
  Source is CodeMirror; View POSTs the current buffer (including unsaved) to `/markdown` and
  inserts via `fromServerHtml`. Preference `syncaidync:files-md-mode` (`view` | `source`),
  default `view`. Save and Reload keep working from either mode.
- `dync-changes-view`: when the selected file is markdown, **View | Diff** (default View).
  View loads the new-side text (`GET …/file` for a session source; deleted → empty state) and
  renders the same way. Notes stay on Diff only.

**Not in this order.** Margins / sidecar annotations, Mermaid, pull-clone media URLs, creating
files from the UI.

**Tests.**

- Unit: relative image rewrite; `javascript:` and `data:` die; absolute https kept; path join
  with `../`.
- Unit / integration: media containment (escape, `.git`, symlink out), wrong type, oversize,
  happy png.
- Integration: `POST /sessions/{id}/markdown` round-trip with a relative image.

**Manual check.** Open a README with headings, a table, a code fence, and a relative image;
confirm View is default; edit in Source and flip back to View; in Changes, View then Diff a
modified `.md` and leave a line note on Diff.

**Done when.** `scripts/check.sh` is green and markdown feels like a reading surface first,
not an afterthought panel.

## Field notes

### 2026-11-19

- Shipped View-first markdown in Files (`View | Source`, default View) and Changes
  (`View | Diff`, default View for `.md`).
- Server: `POST /sessions/{id}/markdown` with relative image rewrite,
  `GET /sessions/{id}/media`, plain `POST /markdown`, and `GET /pulls/{id}/file` for PR head
  text. Shared `.md-body` in `web/css/markdown.css` (also used for PR descriptions).
- Margins / sidecar annotations remain out of scope for this order.

### 2026-11-20

- Changes markdown View shows file notes and line notes on the rendered prose (`source_lines`
  tags blocks with `data-source-line`; hover + to add a note). Diff still has hunk notes.
- Still out of this order: Mermaid, pull-clone media URLs. Creating files from the UI shipped
  later (Files **New file**).
