---
name: research-os-wiki-update
description: Maintain the active session Karpathy-style research wiki by ingesting acquired documents, creating source summaries, and updating cross-linked knowledge pages.
---

# Research-OS Wiki Update

This is the default owner of durable `sessions/{session_id}/wiki/` mutation. It follows the Karpathy-style wiki philosophy: raw sources are immutable evidence, while the wiki is the LLM-maintained compiled knowledge base.

## Inputs

Read `sessions/{session_id}/doc_search_manifest.json` when present, session raw source files, session extracted artifacts, session visual assets, existing session wiki pages, and any synthesis or output artifacts assigned by the orchestrator prompt.

## Outputs

- Updated compact wiki pages under `sessions/{session_id}/wiki/`.
- Source summary pages under the active wiki `sources/` directory.
- `sessions/{session_id}/wiki_update_report.md`.
- Append an entry to the active wiki `log.md`.

## Rules

- Keep top-level session wiki structure compact.
- Use `sessions/{session_id}/wiki/sources/` for source-specific summaries. This replaces the old Reader Agent output.
- Use `sessions/{session_id}/wiki/topics/` for broad research themes that cut across multiple sources, methods, datasets, or entities.
- Use `sessions/{session_id}/wiki/concepts/`, `entities/`, `methods/`, `datasets/`, and `comparisons/` for reusable knowledge pages.
- Use `sessions/{session_id}/wiki/synthesis/` only for explicit output-oriented synthesis pages created or integrated by an assigned synthesis/output stage.
- Use `sessions/{session_id}/wiki/outputs/` for durable user-facing outputs.
- Update `sessions/{session_id}/wiki/index.md`, `sessions/{session_id}/wiki/followups.md`, and `sessions/{session_id}/wiki/log.md`.
- Treat `index.md` as the content catalog with links and one-line summaries.
- Treat `log.md` as append-only chronological history.
- Cite `sessions/{session_id}/wiki/sources/` or raw provenance for every non-trivial technical claim.
- Mark conflicts instead of overwriting them.
- Record supports, contradicts, refines, extends, and duplicates relationships when useful.
- Do not silently delete existing claims.
- Do not use `wiki/topics/` as a catch-all. Put specific algorithms or procedures in `methods/`, datasets and benchmarks in `datasets/`, named models/projects/institutions in `entities/`, terminology in `concepts/`, and side-by-side analyses in `comparisons/`.
- When `doc_search_manifest.json` includes `code_status`, carry it into the source note. Distinguish official code/model/demo/dataset artifacts from unofficial reproductions, and preserve lookup uncertainty rather than implying that missing code is definitively unavailable.
- When `doc_search_manifest.json` includes `visual_assets`, carry important figures and tables into the source note. Embed successful figure/table images with Markdown image syntax, and summarize why each visual matters. Do not hide failed or uncertain visual extraction when the visual is important.

## Figures and Tables

Every paper/report source note must include `## Key Figures and Tables`.

- Include 1-5 important figures/tables when available.
- Embed successful images with paths relative to the source note, for example:
  `![Figure 1: Method overview](../../assets/figures/source_slug__fig1_method_overview.png)`.
- For text-extractable tables, prefer a Markdown table in the source note. If only an image was extracted, embed the table image.
- For each visual, include short `Caption`, `Why it matters`, and `Evidence use` lines.
- If important visuals were not extracted, add a short bullet such as `Figure 2: not extracted (uncertain crop); caption ...`.

## Source Summary Template

```markdown
---
source_id: stable-id
title: ...
source_type: paper
date: ...
raw_path: sessions/{session_id}/raw/sources/...
extracted_path: sessions/{session_id}/artifacts/extracted/... or null
---

# Source Title

## Main Claims

## Evidence and Locations

## Methods / Approach

## Datasets, Benchmarks, Metrics

## Findings

## Limitations

## Future Work

## Code / Artifacts

## Key Figures and Tables

## Relevance to Current Session

## Links
```
