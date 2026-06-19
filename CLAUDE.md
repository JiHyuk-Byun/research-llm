# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

research-os is a local research orchestrator. A Rust TUI (the only compiled code) drives a sequence of predefined research **skills**, each executed as a separate `codex exec` subprocess. Skills turn raw sources into a citation-aware, LLM-maintained **wiki**, then reuse that wiki for downstream research tasks (Q&A, synthesis, writing, ideation, visualization, coding, experiment planning).

The product has two compiled commands plus a TUI; nearly all behavior lives in (a) the orchestration logic in `crates/research-os-cli/src/main.rs` and (b) the natural-language skill contracts under `skills/*/SKILL.md`. Editing a skill changes agent behavior without recompiling.

## Commands

```sh
cargo build                 # build the workspace
cargo build -p research-os-cli
cargo clippy
cargo run -p research-os-cli -- init            # scaffold workspace dirs + seed wiki/schemas
cargo run -p research-os-cli                    # no subcommand -> opens the TUI
cargo run -p research-os-cli -- tui             # open TUI explicitly
cargo run -p research-os-cli -- install-skills  # copy skills/ into $CODEX_HOME/.codex/skills
```

The built binary is named `research-os` (not `research-os-cli`). There are no tests in the repo yet; `cargo test` runs clean but covers nothing.

Browser-rendering support (optional, for JS-rendered source pages, used by the doc-search stage):

```sh
npm install
npm run install:browsers            # playwright install chromium
npm run render-source -- "<url>" --out <path>   # scripts/render_source_page.mjs
```

PDF figure/table extraction helper: `scripts/extract_pdf_assets.py` (needs PyMuPDF `fitz` + Pillow).

## Runtime dependency: an agent CLI backend (codex or claude)

The TUI does **not** run LLM logic itself. Each pipeline stage shells out to an external agent CLI. There are two interchangeable backends, selected by the `Backend` enum; `build_agent_command` constructs the right invocation:

```
# codex (default)
codex [--search] exec --cd <root> --sandbox <mode> --skip-git-repo-check --json "<prompt>"

# claude (Claude Code), run with cwd at <root> (no --cd flag exists)
claude -p "<prompt>" --output-format stream-json --verbose --dangerously-skip-permissions
```

So a working `codex` **or** `claude` install is required to run anything beyond the UI. **Selecting the backend:** the TUI command `/agent codex|claude` switches it live (blocked mid-run); the env var `RESEARCH_OS_AGENT=claude|codex` sets the initial value (default `codex`, so existing behavior is unchanged). `RESEARCH_OS_CLAUDE_MODEL` optionally passes `--model` to claude.

Backend-specific notes:
- **codex** — two stage properties are decided in code: `agent_sandbox_mode` (`doc-search`/`search`/`ingest` → `danger-full-access` so Chromium can launch and sources can download; others → `workspace-write`) and `agent_needs_search` (only those acquisition skills get `--search`).
- **claude** — every stage uses `--dangerously-skip-permissions` (bypassPermissions), because `-p` print mode cannot answer interactive permission prompts. WebSearch is a built-in tool, so no search flag is needed. Skills are **not** reinstalled: the prompt names a local path (`Use the local skill at skills/{skill}/SKILL.md`) and claude reads it from the workspace cwd.

Stage output is streamed as JSON lines into `runs/{run_id}/transcripts/{skill}.jsonl` (or `sessions/{id}/transcripts/{turn}-{skill}.jsonl`). `compact_agent_line` content-detects which backend produced each line (codex `thread.`/`turn.`/`item.*` vs claude `assistant`/`system`/`result`) and maps both to a shared `thinking:`/`answer:`/`wrote …` display vocabulary — so historical transcripts from either backend render correctly.

After each stage, `validate_stage_output` checks that the skill produced its contracted artifact (e.g. `doc_search_manifest.json`, `wiki_update_report.md`, `final_answer.md`) before the pipeline continues. Validation is file-based and backend-agnostic.

## Architecture

- **`crates/research-os-cli/src/main.rs`** (~5k lines, single file) — everything: CLI dispatch, the ratatui/crossterm TUI (panes: Conversation / Pipeline / Artifacts), session + run lifecycle, plan-mode conversation, agent subprocess spawning/streaming, and per-stage artifact validation. Functions are grouped roughly: `run`/`run_tui*` (entry + event loop), `draw_tui`/`build_*_lines` (rendering), `run_pipeline*`/`execute_*_plan*` (orchestration), `build_agent_command`/`invoke_agent*` (backend spawn), `compact_agent_line`/`compact_claude_line` (output parsing), `validate_*` (artifact contracts), `init_*_workspace` (scaffolding).
- **`skills/research-os-*/SKILL.md`** — the agent contracts. Each is a markdown skill with YAML frontmatter (`name`, `description`) that codex loads. The 16 skills in `SKILLS` (see top of `main.rs`) split into knowledge-building (planner, doc-search, wiki-update, synthesis, lint-critic) and knowledge-using (qa, discussion, ideation, writing, visualization, coding, experiment-planning). `DEPRECATED_SKILLS` (`search`, `source-triage`, `ingest`, `reader`) are the older run-era acquisition pipeline, superseded by the single `doc-search` skill — don't select them for new work.
- **`schemas/*.schema.json`** — JSON Schemas for stage artifacts (`plan`, `doc_search_manifest`, `ingest_manifest`, `search_results`, `selected_sources`, `stage_status`). Snapshotted into each session/run at init.
- **`scripts/`** — out-of-process helpers a skill may call (`render_source_page.mjs`, `extract_pdf_assets.py`).

### Sessions vs. runs (important terminology)

The codebase carries two overlapping workspace concepts; be aware of the drift:

- **`sessions/{session_id}/`** — the current, user-facing persistent workspace (README + skills speak in these terms). One session is a living research workspace holding `turns.jsonl`, `plan/`, `transcripts/`, `raw/sources/`, `artifacts/{extracted,discovery,implementation}/`, `assets/{figures,tables,pages}/`, `schemas/`, and `wiki/`.
- **`runs/{run_id}/`** — the per-pipeline-execution unit underneath, and the vocabulary `AGENTS.md` is written in. Run wiki is synced back to the session (`sync_session_wiki_from_run`).

`AGENTS.md` is the authoritative agent rule set but still uses `runs/{run_id}/` and a slightly older wiki layout (`wiki/topics/`); the session layout adds split folders (`concepts/`, `methods/`, `datasets/`, etc.). When in doubt, follow `sessions/` paths for new work and treat repo-level `wiki/`, `raw/`, `schemas/` as seed/template state only.

### TUI turn model

- Plain prompt → Discussion turn.
- `/search <prompt>` → Doc Search → Wiki Update → Discussion (fixed pipeline).
- `Tab` → plan mode: the planner proposes a session-local pipeline and asks only blocking questions, then waits for confirmation before execution. `Esc`/`Tab` exits.
- Other slash commands: `/init`, `/skills`, `/scope`, `/resume`, `/artifacts`, `/open`, `/clear`, `/exit` (see `SLASH_COMMANDS`).

## Invariants to preserve (from AGENTS.md)

These are core to the product's purpose — don't let changes violate them:

- Never produce a final research answer directly from raw search results; answers derive from source notes → wiki → synthesis.
- Raw sources are immutable after ingest.
- Only the `research-os-wiki-update` skill mutates durable wiki pages by default (reader may create source notes; synthesis may create synthesis pages when planned).
- Every non-trivial technical claim in `wiki/topics|concepts|synthesis|outputs` must cite a `wiki/sources/` note.
- Knowledge-using skills must read `wiki/index.md` first; if coverage is insufficient, request a knowledge-building/hybrid run rather than answering blind.
- User source scope is binding. The planner records it in `plan.json`; doc-search must not broaden scope unless the plan explicitly allows it.

## Generated state

`runs/`, `sessions/`, and runtime `raw/`/`artifacts/`/`wiki/` are gitignored generated output. The repo-tracked `wiki/`, `raw/`, `schemas/` directories are templates seeded into new sessions, not live data.
