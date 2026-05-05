---
name: research-os-planner
description: Plan research-os runs by classifying user intent, selecting the knowledge-building, knowledge-using, or hybrid pipeline, preserving source constraints, and writing runs/{run_id}/plan.json.
---

# Research-OS Planner

Write `runs/{run_id}/plan.json`. Do not run downstream agents, invent sources, or mutate wiki artifacts.

## Required Behavior

1. Read the user instruction and `runs/{run_id}/wiki/index.md` if it exists.
2. Classify `task_type` as `knowledge_building`, `knowledge_using`, or `hybrid`.
3. Extract binding `source_scope` constraints, including venue, collection, local folder, source type, recency, and expansion policy.
4. Choose ordered stages from the predefined research-os skills.
5. Assign required inputs and outputs for every stage.

If the user confines search, set `expansion_policy` to `ask_user_before_expanding` unless the user explicitly allows broadening.

## Standard Stage Orders

Knowledge building:

```text
research-os-search
research-os-source-triage
research-os-ingest
research-os-reader
research-os-wiki-update
research-os-synthesis
research-os-wiki-update
research-os-lint-critic
```

Knowledge using:

```text
research-os-qa | research-os-discussion | research-os-ideation | research-os-writing | research-os-visualization | research-os-coding | research-os-experiment-planning
optional research-os-wiki-update
```

Hybrid:

```text
research-os-search
research-os-source-triage
research-os-ingest
research-os-reader
research-os-wiki-update
task-specific knowledge-using skill
optional research-os-synthesis
research-os-lint-critic
```

## `plan.json` Shape

```json
{
  "run_id": "run-id",
  "user_instruction": "...",
  "task_type": "knowledge_building",
  "source_scope": {
    "mode": "web|academic|local_library|fixed_collection|venue_scope|wiki_only",
    "allowed_sources": [],
    "disallowed_sources": [],
    "allowed_source_types": [],
    "recency": "as specified or default",
    "expansion_policy": "ask_user_before_expanding|allowed|forbidden"
  },
  "stages": [
    {
      "name": "search",
      "agent_skill": "research-os-search",
      "inputs": ["runs/{run_id}/plan.json"],
      "outputs": ["runs/{run_id}/search_results.json"],
      "may_mutate_wiki": false
    }
  ],
  "final_expected_artifact": "runs/{run_id}/wiki/outputs/final_answer.md"
}
```

Use run-local artifact roots for all planned run artifacts:

- `runs/{run_id}/raw/`
- `runs/{run_id}/parsed/`
- `runs/{run_id}/schemas/`
- `runs/{run_id}/wiki/`
