---
name: research-os-ingest
description: Import selected research sources into run-local raw/sources with PDF-first preservation, extract text into run-local parsed/sources, and write ingest_manifest.json with provenance.
---

# Research-OS Ingest

Read the selected source artifact assigned in `runs/{run_id}/plan.json`, such as `runs/{run_id}/selected_sources.json` or `runs/{run_id}/triage.json`. For each selected research source, discover the best available full-text/PDF file link, download and preserve the raw file when possible, write parsed text under `runs/{run_id}/parsed/sources/`, and write `runs/{run_id}/ingest_manifest.json`.

## Rules

- Preserve raw sources in immutable form.
- For paper/report sources, actively look for the original full-text/PDF file before falling back to visible text.
- Record provenance: original URL/path, access date, raw path, parsed path, extraction method, and failures.
- The selected source is fixed by Triage. Do not add substitute research sources while searching for files; only find downloadable/full-text artifacts for the selected source.
- Do not create synthesis.
- Do not mutate durable wiki pages.

## PDF-First Policy

For each selected paper/report source:

1. Identify the best direct PDF or full-text file URL for the selected source.
   - If the selected source metadata includes `pdf_url`, use it.
   - If the source is an OpenReview forum URL like `https://openreview.net/forum?id=<id>`, derive `https://openreview.net/pdf?id=<id>`.
   - If the source is a CVF/OpenAccess HTML page, inspect the page or URL pattern for the linked PDF.
   - If the source is a JavaScript-rendered official venue detail page, render or inspect that detail page before falling back. Prefer the repo-local helper when useful:
     `npm run render-source -- '<source-url>' --out 'runs/{run_id}/raw/sources/{source_id}-rendered.json'`.
   - If search preserved an official collection JSON or rendered page under `runs/{run_id}/raw/sources/`, inspect that artifact for fields such as `paper_pdf_url`, `pdf_url`, `paper_url`, `url_pdf`, or direct `.pdf` links matching the selected `source_id` or detail URL.
   - If the source is a PMLR/proceedings page, inspect the page for the linked PDF.
2. If the selected metadata and official pages do not expose a PDF/file URL, perform targeted web lookup for that exact selected source.
   - Search by exact title plus author/venue terms and `pdf`, `arxiv`, `openreview`, `proceedings`, `supplementary`, or `project`.
   - Accept a discovered file URL only when it clearly matches the selected source title and at least one additional identifier such as authors, venue/year, OpenReview id, poster id, DOI, arXiv id, or official/project page cross-link.
   - Prefer official venue/proceedings, OpenReview, arXiv, publisher, institutional, or author/project-hosted PDFs over third-party mirrors.
   - Do not use the discovered page as evidence for a different paper. If the match is uncertain, do not download it; record the uncertainty and fall back.
   - Keep a record of targeted lookup queries and accepted/rejected file candidates in `ingest_manifest.json`.
3. Try to download the PDF/file with a normal shell downloader, for example `curl -L --fail '<pdf_url>' -o 'runs/{run_id}/raw/sources/{source_id}.pdf'`.
4. If the PDF/file download succeeds:
   - Preserve it as `runs/{run_id}/raw/sources/{source_id}.pdf`.
   - Extract or create parsed text at `runs/{run_id}/parsed/sources/{source_id}.txt` or `.md`.
   - Set `pdf_download_status` to `success`.
5. If the PDF/file URL is unavailable after selected metadata inspection, official detail inspection, run-local artifact inspection, and targeted lookup, or if download fails:
   - Save visible page text, metadata, or browser-accessible content as `runs/{run_id}/raw/sources/{source_id}.txt`.
   - Save normalized parsed text as `runs/{run_id}/parsed/sources/{source_id}.txt` or `.md`.
   - Record the exact fallback reason and download error.

Do not silently skip PDF attempts for paper/report sources. If no PDF URL is found, set `pdf_download_attempted` to false and explain which metadata/detail pages and targeted lookup queries were inspected. If shell networking fails, record that failure in the manifest and use the text fallback.

## Output Shape

```json
{
  "run_id": "run-id",
  "items": [
    {
      "source_id": "stable-id",
      "original_url_or_path": "...",
      "pdf_url": "...",
      "pdf_discovery_method": "selected_metadata|derived_url|official_detail|official_collection_artifact|targeted_web_lookup|none",
      "pdf_discovery_queries": [],
      "pdf_candidate_urls": [],
      "pdf_match_rationale": "...",
      "pdf_download_attempted": true,
      "pdf_download_status": "success|failed|not_available|not_applicable",
      "pdf_download_error": "...",
      "fallback_reason": "...",
      "raw_format": "pdf|html|txt|metadata_only",
      "access_date": "YYYY-MM-DD",
      "raw_path": "runs/{run_id}/raw/sources/...",
      "parsed_path": "runs/{run_id}/parsed/sources/...",
      "extraction_method": "...",
      "known_failures": []
    }
  ]
}
```
