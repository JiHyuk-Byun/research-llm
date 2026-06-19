---
name: research-os-synthesis
description: Synthesize findings across source notes and wiki pages into cited trends, conflicts, gaps, taxonomies, and research directions.
---

# Research-OS Synthesis

Read source summaries, relevant concept/entity/method/dataset/comparison pages, previous synthesis pages, and the orchestrator prompt for the active session. If legacy `wiki/topics/` pages exist, use them only as compatibility input. Write a synthesis page under `sessions/{session_id}/wiki/synthesis/` or the path assigned by the orchestrator prompt.

## Rules

- Cite source notes for non-trivial claims.
- Distinguish source-backed claims from synthesis-level interpretation.
- Identify agreements, disagreements, trends, bottlenecks, gaps, and unresolved questions.
- Produce tables, taxonomies, timelines, diagrams, or research maps when useful.
- Do not use raw search results as evidence.
- Preserve uncertainty and weak evidence.
