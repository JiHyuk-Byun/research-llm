---
name: research-os-coding
description: Implement research-related code, parsers, analysis scripts, demos, simulations, or PoC tools after reading relevant research-os wiki context.
---

# Research-OS Coding

Read `sessions/{session_id}/wiki/index.md` first, then relevant session-local source, concept, entity, method, dataset, comparison, synthesis, and output pages. If the wiki is not sufficient to justify the requested implementation, request knowledge-building or a hybrid pipeline instead of guessing.

Implement code only in paths assigned by the plan or explicitly requested by the user.

## Storage Contract

- Repo/product feature or bug fix: edit the assigned repository files directly. Also write `sessions/{session_id}/artifacts/implementation/{turn_id}/notes.md` with changed files, validation commands, and the wiki evidence or user instruction that motivated the implementation.
- Research prototype, experiment scaffold, analysis script, parser, demo, simulation, or generated code artifact: create it under `sessions/{session_id}/artifacts/implementation/{turn_id}/`.
- Research implementation artifacts must include `README.md` or `notes.md` with purpose, entrypoints, inputs, outputs, and run instructions.
- Keep generated outputs from research implementation work inside the same `sessions/{session_id}/artifacts/implementation/{turn_id}/` subtree unless the plan assigns a more specific session-local artifact path.
- Do not create `runs/` or copy the session into a new workspace.

## Rules

- Connect implementation choices to research goals.
- Keep code artifacts reproducible.
- Cite or reference the wiki evidence motivating the implementation in output notes.
- Do not treat coding as evidence unless an experiment or validation artifact is produced.
- Request knowledge-building if the wiki does not support the requested implementation.
