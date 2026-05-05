---
name: research-os-reader
description: Read ingested parsed source text and create structured citation-aware source notes under run-local wiki/sources.
---

# Research-OS Reader

Read `runs/{run_id}/plan.json`, `runs/{run_id}/ingest_manifest.json`, parsed source text, and source metadata. Create or update `runs/{run_id}/wiki/sources/{source_id}.md`.

## Rules

- Separate source claims from reader inference.
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
parsed_path: runs/{run_id}/parsed/sources/...
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
