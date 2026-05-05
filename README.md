# research-os

research-os is a local research orchestrator. It runs predefined Codex agent skills in sequence, turns sources into structured source notes, maintains a compact citation-aware wiki, and uses that wiki for downstream research work.

## Quick Start

```sh
cargo run -p research-os-cli -- init
cargo run -p research-os-cli
```

Running without a subcommand opens the Codex-style TUI. Type a research instruction, press Enter, and the TUI will:

- run the Planner first
- read `runs/{run_id}/plan.json`
- execute the planned agent skills in order
- show the active agent conversation
- show pipeline stages and produced artifacts
- save transcripts under `runs/{run_id}/transcripts/`
- save run-local raw, parsed, schema, and wiki artifacts under `runs/{run_id}/`

You can also start the TUI explicitly:

```sh
cargo run -p research-os-cli -- tui
```

For non-interactive command mode:

```sh
cargo run -p research-os-cli -- run "Please research recent semiconductor HBM research trends"
```

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
npm run render-source -- "https://example.org/papers.html" --out runs/example/raw/sources/rendered-source.json
```

The Search stage runs with broader sandbox permissions than downstream stages so Chromium can launch for rendered-page extraction. Source-scope and wiki-mutation checks still apply after the stage.

## Pipeline

```text
Planner
→ Search
→ Source Triage
→ Ingest
→ Reader
→ Wiki Update
→ Synthesis
→ Wiki Update
→ Lint / Critic
```

Knowledge-using tasks read the wiki first. If coverage is insufficient, the planner should select a hybrid pipeline that builds missing knowledge before answering.

## Run Artifacts

Each run is self-contained:

```text
runs/{run_id}/
  plan.json
  plan.md
  transcripts/
  raw/sources/
  parsed/sources/
  schemas/
  wiki/
```

Repo-level `wiki/`, `raw/`, `parsed/`, and `schemas/` are seed/template state. New artifacts for a run should be written inside that run directory.

## Search Scope

Users may constrain source scope:

- only CVPR 2026 papers
- only a local folder
- arXiv only, no blogs
- a fixed paper library

The planner records this in `plan.json`; the Search agent must not broaden scope unless explicitly allowed.
