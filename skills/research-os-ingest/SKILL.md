---
name: research-os-ingest
description: Import selected research sources by preserving only source-native downloads in run-local raw/sources, writing run-only extracted/discovery artifacts under artifacts, and writing ingest_manifest.json with provenance.
---

# Research-OS Ingest

Deprecated for new plans. Use `research-os-doc-search`, which combines scoped search, triage, and ingest. This skill remains only for legacy run compatibility.

Read the selected source artifact assigned in `runs/{run_id}/plan/plan.json`, such as `runs/{run_id}/selected_sources.json` or `runs/{run_id}/triage.json`. For each selected research source, discover the best available full-text/PDF file link, download and preserve the source-native raw file when possible, write run-only extracted or discovery records under `runs/{run_id}/artifacts/`, and write `runs/{run_id}/ingest_manifest.json`.

## Rules

- Preserve raw sources in immutable form.
- `runs/{run_id}/raw/sources/` is only for source-native downloads/imports: PDF, official HTML, official JSON, original text files, code archives, slide files, datasets, or local files copied without transformation.
- Do not put derived artifacts in `raw/sources/`. Browser-visible text fallbacks, extracted abstracts, rendered DOM records, normalized text, candidate lists, lookup notes, and other second-pass processing outputs belong in `runs/{run_id}/artifacts/extracted/` or `runs/{run_id}/artifacts/discovery/`.
- For paper/report sources, actively look for the original full-text/PDF file before falling back to visible text.
- Record provenance: original URL/path, access date, raw path, extracted artifact paths, extraction method, and failures.
- Use human-readable filenames for stored source files and extracted artifacts. Keep `source_id` as the stable manifest/wiki identifier, but do not use it as the primary filename unless title metadata is unavailable.
- The selected source is fixed by Triage. Do not add substitute research sources while searching for files; only find downloadable/full-text artifacts for the selected source.
- Do not create synthesis.
- Do not mutate durable wiki pages.

## PDF-First Policy

## Filename Policy

For selected research sources, store downloaded source-native files with a readable slug:

```text
runs/{run_id}/raw/sources/{venue_slug}__{title_slug}.{ext}
runs/{run_id}/artifacts/extracted/{venue_slug}__{title_slug}.txt
```

Rules:

- `venue_slug` should come from venue, conference, journal, publisher, collection, or source scope. Examples: `cvpr_2026`, `iclr_2026`, `arxiv`, `openreview`, `pmlr`.
- `title_slug` should come from the selected source title, lowercased, ASCII-normalized when possible, with non-alphanumeric runs collapsed to `_`.
- Keep filenames reasonably short. If the title is long, truncate `title_slug` to about 90 characters and append a short stable suffix from `source_id`, DOI, arXiv id, poster id, or URL hash.
- If two sources would produce the same filename, append a short stable suffix.
- If title metadata is missing, fall back to `{venue_slug}__{source_id}.{ext}`.
- Use the real source file extension when known: `.pdf`, `.html`, `.json`, `.txt`, `.zip`, etc.
- Discovery artifacts may still include the source id when useful, but should prefer the same readable base name plus a suffix such as `__rendered.json` or `__lookup.md`.

For each selected paper/report source:

1. Identify the best direct PDF or full-text file URL for the selected source.
   - If the selected source metadata includes `pdf_url`, use it.
   - If the source is an OpenReview forum URL like `https://openreview.net/forum?id=<id>`, derive `https://openreview.net/pdf?id=<id>`.
   - If the source is a CVF/OpenAccess HTML page, inspect the page or URL pattern for the linked PDF.
   - If the source is a JavaScript-rendered official venue detail page, render or inspect that detail page before falling back. Prefer the repo-local helper when useful:
     `npm run render-source -- '<source-url>' --out 'runs/{run_id}/artifacts/discovery/{source_id}-rendered.json'`.
   - If search preserved an official collection JSON as a direct download under `runs/{run_id}/raw/sources/`, or a rendered/discovery artifact under `runs/{run_id}/artifacts/discovery/`, inspect that artifact for fields such as `paper_pdf_url`, `pdf_url`, `paper_url`, `url_pdf`, or direct `.pdf` links matching the selected `source_id` or detail URL.
   - If the source is a PMLR/proceedings page, inspect the page for the linked PDF.
2. If the selected metadata and official pages do not expose a PDF/file URL, targeted web lookup is mandatory before any text fallback.
   - Run at least these exact-title lookups:
     - `"<title>" pdf`
     - `"<title>" arxiv`
     - `"<title>" <first-author-last-name>`
     - `"<title>" <venue-or-publisher>`
   - For arXiv matches, inspect the landing page and use the `/pdf/<id>` URL or direct `.pdf` URL when the title and authors match.
   - For author/project pages, inspect links named PDF, paper, arXiv, download, manuscript, or full text.
   - For venue/project pages with no explicit PDF link but an arXiv link, follow the arXiv link and download the arXiv PDF.
   - Accept a discovered file URL only when it clearly matches the selected source title and at least one additional identifier such as authors, venue/year, OpenReview id, poster id, DOI, arXiv id, or official/project page cross-link.
   - Prefer official venue/proceedings, OpenReview, arXiv, publisher, institutional, or author/project-hosted PDFs over third-party mirrors.
   - Do not use the discovered page as evidence for a different paper. If the match is uncertain, do not download it; record the uncertainty and fall back.
   - Keep a record of targeted lookup queries and accepted/rejected file candidates in `ingest_manifest.json`.
3. Try to download the PDF/file with a normal shell downloader, for example `curl -L --fail '<pdf_url>' -o 'runs/{run_id}/raw/sources/{venue_slug}__{title_slug}.pdf'`.
4. If the PDF/file download succeeds:
   - Preserve it as `runs/{run_id}/raw/sources/{venue_slug}__{title_slug}.pdf`.
   - Extract text only when needed for downstream reading and save it as a run-only artifact at `runs/{run_id}/artifacts/extracted/{venue_slug}__{title_slug}.txt` or `.md`.
   - Set `pdf_download_status` to `success`.
5. If the PDF/file URL is unavailable after selected metadata inspection, official detail inspection, run-local artifact inspection, and mandatory targeted lookup, or if download fails:
   - Do not save visible page text, copied abstracts, or browser-accessible extracted content under `raw/sources/`.
   - Save the visible page text, metadata extraction, or browser-accessible content as `runs/{run_id}/artifacts/extracted/{venue_slug}__{title_slug}.txt` or `.md`.
   - If the fallback used a rendered page or lookup notes, save that supporting derived record under `runs/{run_id}/artifacts/discovery/{venue_slug}__{title_slug}__*.json` or `.md`.
   - Record the exact fallback reason and download error.

Do not silently skip PDF attempts for paper/report sources. If no PDF URL is found, set `pdf_download_attempted` to false and explain which metadata/detail pages and exact-title targeted lookup queries were inspected. A fallback without recorded exact-title lookup queries is invalid. If shell networking fails, record that failure in the manifest and use the text fallback.

## Output Shape

```json
{
  "run_id": "run-id",
  "items": [
    {
      "source_id": "stable-id",
      "title": "...",
      "venue_or_publisher": "...",
      "file_stem": "venue_slug__title_slug",
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
      "raw_path": "runs/{run_id}/raw/sources/... or null when no source-native file was available",
      "extracted_path": "runs/{run_id}/artifacts/extracted/...",
      "derived_artifact_paths": ["runs/{run_id}/artifacts/discovery/..."],
      "extraction_method": "...",
      "known_failures": []
    }
  ]
}
```
