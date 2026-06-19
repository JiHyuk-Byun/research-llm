---
name: research-os-doc-search
description: Find, triage, download, and preserve scoped research documents in the active session, writing a single doc_search_manifest.json plus raw and session artifact files.
---

# Research-OS Doc Search

Read the orchestrator prompt and existing `sessions/{session_id}/wiki/index.md` first. This agent owns the full document acquisition loop for the active session: scoped search, source triage, PDF/full-text discovery, source-native file download/import, and derived extraction/discovery artifacts. Write `sessions/{session_id}/doc_search_manifest.json`.

Do not mutate `sessions/{session_id}/wiki/`.

## Rules

- Search only within `source_scope`. User-confined scope is binding.
- Record exact queries, searched locations, browser-render attempts, rejected out-of-scope candidates, and coverage gaps.
- Rank and select sources by relevance, credibility, recency, diversity, expected usefulness, and source type.
- For multi-venue scopes, cover every requested venue independently before final selection. Do not stop after one venue has good hits. Record `venue_coverage` with the official URLs or collections searched, rendered status, candidate count, selected count, and gap reason for each requested venue/year.
- For selected paper/report sources, actively find the best original PDF or full-text file before falling back to visible text.
- Once a source has been selected from the allowed scope, a matching file URL from arXiv, publisher, author/project, or institutional hosting is allowed as provenance for that selected source. This does not expand the research source scope when title plus another identifier confirms the match.
- For selected paper/report sources, check whether source code, models, demos, or datasets are publicly available. GitHub, GitLab, Hugging Face, project pages, Papers With Code, OpenReview artifact links, and author/institution pages may be inspected only to verify implementation/artifact availability for an already selected source; this is artifact-status discovery, not source-scope expansion.
- Store only source-native downloads/imports under `sessions/{session_id}/raw/sources/`: PDF, official HTML, official JSON, original text files, code archives, slides, datasets, or local files copied without transformation.
- Store derived session files under `sessions/{session_id}/artifacts/extracted/` or `sessions/{session_id}/artifacts/discovery/`: extracted text, rendered DOM records, abstracts/snippets, lookup notes, candidate lists, and failure notes.
- Store important visual assets under `sessions/{session_id}/assets/figures/` or `sessions/{session_id}/assets/tables/`. These are derived assets intended for wiki inline display.
- Use readable filenames for source files and extracted artifacts: `{venue_slug}__{title_slug}.{ext}`. Keep `source_id` as the stable manifest/wiki identifier.
- Do not treat search snippets as verified knowledge. The Wiki Agent owns durable knowledge integration.

## Search and Browser Rendering

If an allowed source page is JavaScript-rendered and static fetch does not expose source records, use a browser-rendering fallback before declaring a coverage gap. Prefer:

```text
npm run render-source -- '<allowed-url>' --out 'sessions/{session_id}/artifacts/discovery/rendered-source.json'
```

Keep rendered enumeration inside the declared source scope. Do not follow unrelated venue years, sponsor pages, personal project pages, or general web results unless the plan explicitly allows them.

## Venue-Scoped Discovery Notes

Use official venue collection pages and official data endpoints when the scope names a venue/year. A single failing URL is not enough to declare a venue empty if a known official virtual site or data endpoint exists.

- CVPR 2026: prefer `https://cvpr.thecvf.com/virtual/2026/papers.html` and `https://cvpr.thecvf.com/static/virtual/data/cvpr-2026-orals-posters.json`. Do not rely on `https://cvpr.thecvf.com/Conferences/2026/AcceptedPapers` as the only CVPR source.
- ICLR 2026: prefer OpenReview venue/group records and PDF URLs derived from accepted OpenReview IDs.
- ICML 2026: prefer `https://icml.cc/virtual/2026/papers.html` and official poster/detail pages under `https://icml.cc/virtual/2026/poster/`.

When an official venue page is large or rendered, search within the rendered collection and detail pages using domain terms as well as the literal user query. For 3D reconstruction style requests, include relevant synonyms such as `pointmap`, `point map`, `3D reconstruction`, `4D reconstruction`, `online`, `streaming`, `incremental`, `dynamic reconstruction`, `Gaussian Splatting`, and `feed-forward reconstruction`.

If a requested venue has zero selected sources, `coverage_gaps` must name that venue/year, list every official URL or API tried, explain why candidates were rejected, and state whether browser rendering succeeded.

## PDF and Full-Text Discovery

For each selected paper/report source:

1. Use selected metadata `pdf_url` if present.
2. Derive obvious official URLs when safe, such as OpenReview `https://openreview.net/pdf?id=<id>`.
3. Inspect official detail pages and session-local official collection/render artifacts for `paper_pdf_url`, `pdf_url`, `paper_url`, `url_pdf`, or direct `.pdf` links.
4. If no file URL is exposed, run targeted exact-title lookups before fallback:
   - `"<title>" pdf`
   - `"<title>" arxiv`
   - `"<title>" <first-author-last-name>`
   - `"<title>" <venue-or-publisher>`
5. Accept and download a discovered file when it clearly matches title plus at least one additional identifier: authors, venue/year, OpenReview id, poster id, DOI, arXiv id, or official/project-page cross-link.
6. Prefer official venue/proceedings, OpenReview, arXiv, publisher, institutional, or author/project-hosted PDFs over third-party mirrors.
7. If no matching file is available or download fails, save visible/extracted text under `artifacts/extracted/` and record the exact fallback reason.

## Source Code / Artifact Status Discovery

For each selected paper/report source, record implementation and artifact availability in `code_status`.

1. Inspect official metadata and detail pages for links named code, project, software, supplementary, artifact, implementation, GitHub, Hugging Face, demo, dataset, or model.
2. Run targeted exact-title lookups when official pages do not expose code:
   - `"<title>" github`
   - `"<title>" code`
   - `"<title>" huggingface`
   - `"<title>" project`
   - `"<title>" papers with code`
3. Accept a repository or artifact link only when the title plus at least one additional identifier matches: author, venue/year, arXiv/OpenReview id, project page cross-link, README citation, model card citation, or paper URL.
4. Do not clone repositories or download model weights by default. Record links and status only, unless the plan explicitly asks for code import.
5. If code is unavailable or uncertain, record the lookup queries, candidate URLs, and the reason the status is `not_found` or `uncertain`.

Use this status vocabulary:

- `official`: linked from the paper, official venue page, OpenReview, author/project page, or repository/model card that clearly cites the selected paper.
- `unofficial`: third-party reproduction or implementation that clearly identifies the selected paper but is not author/venue linked.
- `not_found`: targeted lookups found no matching code/artifact.
- `uncertain`: candidates exist but identity or authorship could not be verified.

## Visual Asset Extraction

For each selected paper/report source with a PDF, identify 1-5 important figures or tables that materially explain the method, dataset, benchmark, or core result.

1. Prefer figures/tables that are cited in the abstract, introduction, method overview, experiments, or conclusions.
2. Record the caption, page number, and why the asset matters.
3. Use `scripts/extract_pdf_assets.py` when you can infer a reasonable crop box:

```text
python3 scripts/extract_pdf_assets.py \
  --pdf 'sessions/{session_id}/raw/sources/paper.pdf' \
  --session 'sessions/{session_id}' \
  --source-id 'source_id' \
  --source-slug 'venue_slug__title_slug' \
  --label 'fig1_method_overview' \
  --asset-type figure \
  --page 3 \
  --box '72,120,540,430' \
  --caption 'Figure 1 caption...' \
  --rationale 'Shows the core architecture.'
```

The crop box is `x0,y0,x1,y1` in PDF points, or normalized `0..1` fractions. Do not invent precision: if you cannot infer a crop box, record the asset with `extraction_status: "uncertain"` or `failed` rather than saving a misleading crop.

Write successful figure images under `sessions/{session_id}/assets/figures/` and table images under `sessions/{session_id}/assets/tables/`. If a table can be represented as text, also save a Markdown or CSV copy under `sessions/{session_id}/artifacts/extracted/`.

## Filename Policy

- `venue_slug` should come from venue, conference, journal, publisher, collection, or source scope, e.g. `cvpr_2026`, `iclr_2026`, `arxiv`, `openreview`, `pmlr`.
- `title_slug` should come from the selected title, lowercased, ASCII-normalized when possible, with non-alphanumeric runs collapsed to `_`.
- Truncate long title slugs to about 90 characters and append a short stable suffix from `source_id`, DOI, arXiv id, poster id, or URL hash.
- Append a stable suffix if filenames collide.
- If title metadata is missing, use `{venue_slug}__{source_id}.{ext}`.

## Output Shape

```json
{
  "run_id": "run-id",
  "source_scope_obeyed": true,
  "queries": [],
  "browser_render_attempts": [],
  "venue_coverage": [
    {
      "venue": "CVPR",
      "year": "2026",
      "official_locations": [],
      "render_status": "success|failed|not_needed",
      "candidate_count": 0,
      "selected_count": 0,
      "gap_reason": ""
    }
  ],
  "candidates": [],
  "selected_sources": [],
  "downloaded_sources": [
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
      "code_status": {
        "availability": "official|unofficial|not_found|uncertain",
        "artifact_types": ["code|model|demo|dataset|supplementary"],
        "official_urls": [],
        "unofficial_urls": [],
        "lookup_queries": [],
        "candidate_urls": [],
        "match_rationale": "...",
        "last_checked": "YYYY-MM-DD",
        "notes": ""
      },
      "download_attempted": true,
      "download_status": "success|failed|not_available|not_applicable",
      "download_error": "...",
      "fallback_reason": "...",
      "raw_format": "pdf|html|txt|json|metadata_only",
      "access_date": "YYYY-MM-DD",
      "raw_path": "sessions/{session_id}/raw/sources/... or null",
      "extracted_path": "sessions/{session_id}/artifacts/extracted/... or null",
      "derived_artifact_paths": ["sessions/{session_id}/artifacts/discovery/..."],
      "extraction_method": "...",
      "known_failures": []
    }
  ],
  "visual_assets": [
    {
      "source_id": "stable-id",
      "asset_type": "figure|table",
      "label": "Figure 1",
      "caption": "...",
      "asset_path": "sessions/{session_id}/assets/figures/...",
      "page": 3,
      "bbox": [72, 120, 540, 430],
      "extraction_status": "success|uncertain|failed|not_applicable",
      "crop_method": "explicit_box|not_extracted",
      "rationale": "Why this visual matters for the source note.",
      "error": ""
    }
  ],
  "rejected_out_of_scope": [],
  "coverage_gaps": []
}
```
