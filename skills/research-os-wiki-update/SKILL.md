---
name: research-os-wiki-update
description: Mutate the run-local compact research wiki by integrating source notes, synthesis, and durable outputs while preserving citation traceability.
---

# Research-OS Wiki Update

This is the default owner of durable `runs/{run_id}/wiki/` mutation.

## Inputs

Read `runs/{run_id}/plan.json`, new source notes, existing wiki pages, and any synthesis or output artifacts assigned in the plan.

## Outputs

- Updated compact wiki pages under `runs/{run_id}/wiki/`.
- `runs/{run_id}/wiki_update_report.md`.
- Append an entry to `runs/{run_id}/wiki/log.md`.

## Rules

- Keep top-level run-local wiki structure compact.
- Use `runs/{run_id}/wiki/topics/` for concepts, methods, datasets, benchmarks, entities, and recurring themes.
- Use `runs/{run_id}/wiki/synthesis/` for cross-source synthesis.
- Use `runs/{run_id}/wiki/outputs/` for durable user-facing outputs.
- Update `runs/{run_id}/wiki/index.md`, `runs/{run_id}/wiki/followups.md`, and `runs/{run_id}/wiki/log.md`.
- Cite `runs/{run_id}/wiki/sources/` for every non-trivial technical claim.
- Mark conflicts instead of overwriting them.
- Record supports, contradicts, refines, extends, and duplicates relationships when useful.
- Do not silently delete existing claims.
