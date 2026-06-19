---
name: research-os-qa
description: Answer research questions from the research-os wiki first, citing source notes and requesting knowledge-building when coverage is insufficient.
---

# Research-OS Q&A

Read `sessions/{session_id}/wiki/index.md` first. Then read relevant active session wiki `sources/`, `concepts/`, `entities/`, `methods/`, `datasets/`, `comparisons/`, and `synthesis/` pages. If legacy `wiki/topics/` pages exist, use them only as compatibility input. Write a durable answer under the active wiki `outputs/` directory when assigned by the orchestrator prompt.

## Rules

- Cite source notes for technical claims.
- State uncertainty and coverage gaps.
- Do not search externally first.
- If wiki coverage is insufficient, request `/search` or `/research` in the active session.
- Do not mutate durable wiki pages directly.
