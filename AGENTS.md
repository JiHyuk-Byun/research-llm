# research-os Agent Rules

research-os is a durable research-state orchestrator. Do not answer research questions directly from raw search results. Build or inspect the wiki first.

## Persistent Layers

- `runs/{run_id}/`: auditable, self-contained per-run artifacts.
- `runs/{run_id}/raw/sources/`: immutable source files for this run.
- `runs/{run_id}/parsed/sources/`: extracted text and metadata for this run.
- `runs/{run_id}/schemas/`: schema snapshot used by this run.
- `runs/{run_id}/wiki/`: compact LLM-maintained wiki snapshot/output for this run.
- Repo-level `wiki/`, `raw/`, `parsed/`, and `schemas/` are templates or seed state only. New run artifacts belong under `runs/{run_id}/`.

## Compact Wiki

Use only these durable wiki locations by default:

- `runs/{run_id}/wiki/index.md`: catalog of pages with one-line summaries.
- `runs/{run_id}/wiki/log.md`: append-only chronological updates.
- `runs/{run_id}/wiki/followups.md`: open questions, weak evidence, missing coverage, and future work.
- `runs/{run_id}/wiki/sources/`: one structured note per source.
- `runs/{run_id}/wiki/topics/`: concepts, methods, datasets, benchmarks, entities, and recurring themes.
- `runs/{run_id}/wiki/synthesis/`: cross-source trends, conflicts, gaps, taxonomies, and research maps.
- `runs/{run_id}/wiki/outputs/`: durable user-facing outputs worth preserving.

Do not create new wiki top-level folders unless the user explicitly asks.

## Invariants

- Raw sources are immutable after ingest.
- Every important technical claim in run-local `wiki/topics/`, `wiki/synthesis/`, or `wiki/outputs/` must cite run-local `wiki/sources/`.
- Knowledge-using agents must read `runs/{run_id}/wiki/index.md` before answering, coding, writing, visualizing, discussing, ideating, or planning experiments.
- If wiki coverage is insufficient, request a knowledge-building or hybrid pipeline.
- Only `research-os-wiki-update` mutates durable wiki pages by default. Reader may create source notes, and Synthesis may create synthesis drafts/pages when planned.
- User source constraints are binding. Search may broaden only when the plan explicitly allows it.

## Source Scope

Planner must encode source scope in `runs/{run_id}/plan.json`. Search must obey it exactly. If a user says “only CVPR 2026,” rejected out-of-scope sources may be logged but must not be used as evidence.
