---
name: research-os-reader
description: Read ingested raw sources and run-only extracted artifacts, then create structured citation-aware source notes under run-local wiki/sources.
---

# Research-OS Reader

Deprecated for new plans. `research-os-wiki-update` now performs Karpathy-style wiki ingest and creates source summary pages directly. This skill remains only for legacy run compatibility.

Read `runs/{run_id}/plan/plan.json`, `runs/{run_id}/ingest_manifest.json`, raw source files, extracted artifacts when present, and source metadata. Create or update `runs/{run_id}/wiki/sources/{source_id}.md`.

## Rules

- Separate source claims from reader inference.
- Prefer source content from `runs/{run_id}/artifacts/extracted/` when Ingest created it; otherwise extract/read directly from `runs/{run_id}/raw/sources/`.
- Use `raw_path` as provenance for the original downloaded/imported file. Do not create or rely on `parsed/`.
- Preserve uncertainty.
- Include source locations when available: page, section, figure, table, paragraph.
- Do not generalize across multiple sources.
- Do not update topic or synthesis pages.

## Source Note Template

```markdown
---
source_id: stable-id
title: ...
source_type: paper
date: ...
raw_path: runs/{run_id}/raw/sources/...
extracted_path: runs/{run_id}/artifacts/extracted/... or null
---

# Source Title

## Main Claims

## Evidence and Locations

## Methods / Approach

## Datasets, Benchmarks, Metrics

## Findings

## Limitations

## Future Work

## Relevance to Current Run

## Reader Inference

## Links to Topics
```
