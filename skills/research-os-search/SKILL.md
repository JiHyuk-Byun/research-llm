---
name: research-os-search
description: Search only within the source_scope declared by research-os Planner and write candidate source metadata to search_results.json.
---

# Research-OS Search

Read `runs/{run_id}/plan.json` first. Write `runs/{run_id}/search_results.json`.

## Rules

- Search only within `source_scope`.
- User-confined scope is binding. Do not broaden it.
- Record exact queries, searched locations, and rejected out-of-scope candidates.
- Do not spend effort finding download-target PDF/file links. The Search agent discovers research source candidates, not downloadable files.
- If a direct PDF URL is already visible in the same source candidate record, you may preserve it as incidental metadata, but do not derive, validate, or search for it.
- When an allowed source page is JavaScript-rendered and static text fetch does not expose the source list, use a browser-rendering fallback before declaring a coverage gap.
- Do not treat search results as verified knowledge.
- Do not mutate `wiki/`.

If scoped search is insufficient, write a coverage gap instead of expanding.

## Browser-rendered Source Enumeration

Some official venue/library pages expose paper lists only after JavaScript runs. If the source scope names one of these pages or a venue whose official paper list is on such a page, enumerate sources from the rendered DOM instead of relying only on text search snippets.

Required fallback behavior:

1. Try the allowed static page first and inspect whether it contains source records.
2. If the page says JavaScript is required, shows an empty list, or only exposes navigation/chrome, open the allowed page with a headless browser and read the rendered DOM.
3. Prefer the repo-local helper when available:
   `npm run render-source -- '<allowed-url>' --out 'runs/{run_id}/raw/sources/rendered-source.json'`.
   The helper launches Chromium headless through Playwright, opens the page, waits for rendered DOM, then extracts source-candidate titles, authors, detail links, and snippets from anchors and nearby DOM text.
4. Keep the rendered collection inside the declared `source_scope`. Do not follow unrelated venue years, sponsor pages, personal project pages, or general web results unless the plan explicitly allows them.
5. If the rendered DOM exposes poster/oral/detail links, record those official links as `url_or_path`.
6. If detail pages must be opened to confirm source metadata or abstracts, open only detail pages that are still inside the allowed source scope.
7. If browser rendering is unavailable, blocked, times out, or the rendered page still has no source records, record the exact failure in `browser_render_attempts` and `coverage_gaps`.

Keep official detail pages as `url_or_path`. Ingest owns later PDF/full-text discovery and download.

## Output Shape

```json
{
  "run_id": "run-id",
  "source_scope_obeyed": true,
  "queries": [],
  "browser_render_attempts": [
    {
      "url": "...",
      "status": "success|failed|not_needed",
      "method": "playwright|browser_tool|other",
      "records_found": 0,
      "notes": "..."
    }
  ],
  "results": [
    {
      "source_id": "stable-id",
      "title": "...",
      "authors_or_org": [],
      "date": "...",
      "url_or_path": "...",
      "source_type": "paper|report|web|code|slide|dataset|benchmark",
      "venue_or_publisher": "...",
      "abstract_or_snippet": "...",
      "discovery_method": "web_search|static_fetch|browser_rendered_dom|local_library",
      "official_confirmation_url": "...",
      "scope_match_rationale": "..."
    }
  ],
  "rejected_out_of_scope": [],
  "coverage_gaps": []
}
```
