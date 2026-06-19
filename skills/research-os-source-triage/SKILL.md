---
name: research-os-source-triage
description: Rank and select search results for ingestion while preserving source-scope constraints and coverage-gap notes.
---

# Research-OS Source Triage

Deprecated for new plans. Use `research-os-doc-search`, which combines scoped search, triage, and ingest. This skill remains only for legacy run compatibility.

Read `runs/{run_id}/plan/plan.json` and `runs/{run_id}/search_results.json`. Write `runs/{run_id}/selected_sources.json`.

## Rules

- Rank by relevance, credibility, recency, diversity, expected usefulness, and source type.
- Do not prefer recency blindly over foundational importance.
- Do not summarize the field without reading selected sources.
- Do not mutate `wiki/`.
- Reject out-of-scope sources even if they look useful.
- Preserve search provenance fields when present, including `discovery_method` and `official_confirmation_url`.
- Do not spend effort discovering final PDF/file links. If Search preserved an incidental `pdf_url`, pass it through unchanged, but the Ingest agent owns targeted full-text/PDF discovery and download.

## Output Shape

```json
{
  "run_id": "run-id",
  "selected_sources": [
    {
      "source_id": "stable-id",
      "title": "...",
      "url_or_path": "...",
      "pdf_url": "...",
      "discovery_method": "web_search|static_fetch|browser_rendered_dom|local_library",
      "official_confirmation_url": "...",
      "rank": 1,
      "selection_rationale": "...",
      "expected_use": "..."
    }
  ],
  "not_selected": [],
  "coverage_gaps": []
}
```
