# research-os

research-os is a local research orchestrator. It runs predefined Codex agent skills in sequence, turns sources into structured source notes, maintains a compact citation-aware wiki, and uses that wiki for downstream research work.

## Quick Start

```sh
cargo run -p research-os-cli -- init
cargo run -p research-os-cli
```

Running without a subcommand opens the Codex-style TUI. Type a research instruction, press Enter, and the TUI will:

- keep the current `sessions/{session_id}/` workspace active
- answer normal prompts with a lightweight discussion turn
- run `/search <prompt>` for source acquisition
- switch to plan mode with Tab for a planner-proposed pipeline
- show the active agent conversation
- show current turn stages and produced artifacts
- append turn metadata to `sessions/{session_id}/turns.jsonl`
- save transcripts under `sessions/{session_id}/transcripts/`
- save session raw, artifact, schema, and wiki files under `sessions/{session_id}/`

You can also start the TUI explicitly:

```sh
cargo run -p research-os-cli -- tui
```

## Agent Backend (Codex or Claude)

Each stage runs on an external agent CLI. research-os supports two interchangeable backends:

- `codex` (default) — `codex exec --json`
- `claude` — Claude Code's `claude -p --output-format stream-json`

Switch backends live inside the TUI with `/agent codex` or `/agent claude` (shows the current backend with no argument). Set the initial backend for a process with the `RESEARCH_OS_AGENT=claude|codex` environment variable (defaults to `codex`); `RESEARCH_OS_CLAUDE_MODEL` optionally selects the claude model. A working install of the chosen CLI is required. Skills are read from the local `skills/` path, so the `claude` backend needs no separate skill install.

Install local skills into Codex skill storage:

```sh
cargo run -p research-os-cli -- install-skills
```

Optional browser-rendering support for JavaScript-rendered source pages:

```sh
npm install
npm run install:browsers
```

Search agents can then render an allowed source page and extract paper-like records:

```sh
npm run render-source -- "https://example.org/papers.html" --out sessions/example/artifacts/discovery/rendered-source.json
```

The Doc Search stage runs with broader sandbox permissions than downstream stages so Chromium can launch for rendered-page extraction and source downloads. Source-scope and wiki-mutation checks still apply after the stage.

## Session Turns

```text
normal prompt
→ Discussion

/search <prompt>
→ Doc Search
→ Wiki Update
→ Discussion

Tab, then prompt
→ Planner proposes a session-local pipeline
→ Planner asks for confirmation or missing constraints
→ Plain replies continue plan mode until Tab or Esc exits it
```

Knowledge-using tasks read the session wiki first. If coverage is insufficient, run `/search` for the fixed search pipeline or switch to plan mode with Tab when the planner should choose the pipeline.

## Session Artifacts

Each session is a living research workspace:

```text
sessions/{session_id}/
  turns.jsonl
  plan/
  transcripts/
  raw/sources/
  artifacts/extracted/
  artifacts/discovery/
  artifacts/implementation/
  assets/figures/
  assets/tables/
  assets/pages/
  schemas/
  wiki/
```

`raw/sources/` is for source-native downloaded/imported files only, such as PDFs, official HTML/JSON, original text files, code archives, slides, or datasets. Extracted text, rendered DOM records, snippets, abstracts, candidate lists, and other session-derived files belong under `artifacts/extracted/` or `artifacts/discovery/`. Research prototypes, demos, analysis scripts, parsers, simulations, and generated code artifacts belong under `artifacts/implementation/`. Durable interpreted knowledge belongs in `wiki/`.

When a coding agent changes the research-os product itself, it edits the assigned repository files directly and writes an implementation note under `sessions/{session_id}/artifacts/implementation/{turn_id}/`. When it creates research code for the active session, the code and run instructions stay inside that implementation artifact subtree.

Important paper figures and table images are stored under `assets/` and embedded in source notes with Markdown image links. The files stay outside the markdown so the wiki remains diffable, but Markdown viewers render them inline:

```markdown
![Figure 1: Method overview](../../assets/figures/source_slug__fig1_method_overview.png)
```

Source files should use readable filenames when title metadata is available:

```text
raw/sources/cvpr_2026__laser_layer_wise_scale_alignment_for_training_free_streaming_4d_reconstruction.pdf
artifacts/extracted/cvpr_2026__laser_layer_wise_scale_alignment_for_training_free_streaming_4d_reconstruction.txt
```

`source_id` remains the stable identifier in manifests and wiki links.

The wiki follows a Karpathy-style markdown knowledge-base layout:

```text
wiki/
  index.md
  log.md
  followups.md
  sources/
  concepts/
  entities/
  methods/
  datasets/
  comparisons/
  synthesis/
  outputs/
```

Repo-level `wiki/`, `raw/`, and `schemas/` are seed/template state. New artifacts for a run should be written inside that run directory.

## Search Scope

Users may constrain source scope:

- only CVPR 2026 papers
- only a local folder
- arXiv only, no blogs
- a fixed paper library

The planner records this in `plan.json`; the Doc Search agent must not broaden scope unless explicitly allowed.
