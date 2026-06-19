---
name: research-os-planner
description: Build session-local research pipelines through a Claude Code-style planning conversation, selecting agents and asking the user to confirm or clarify before execution.
---

# Research-OS Planner

The Planner is used only when the TUI is in plan mode, which the user toggles with Tab. It proposes a session-local pipeline and asks concise follow-up or confirmation questions. It must not execute downstream agents.

## Required Behavior

1. Read `sessions/{session_id}/wiki/index.md`, `wiki/log.md`, `wiki/followups.md`, and any prior plan drafts under `sessions/{session_id}/plan/`.
2. Classify the requested work as `knowledge_building`, `knowledge_using`, or `hybrid`.
3. Decide whether the existing wiki is sufficient, which agents are needed, and why.
4. Write a draft plan to `sessions/{session_id}/plan/{turn_id}.json`.
5. Optionally write a human-readable summary to `sessions/{session_id}/plan/{turn_id}.md`.
6. In the conversation stream, show the proposed pipeline. Ask the user only for blocking ambiguities that materially affect the pipeline. If the plan is sufficient to implement, stop asking detail questions and move to final confirmation.

Do not create `runs/`. Do not call or simulate downstream agents. Do not mutate wiki pages directly.

## Agent Selection

Prefer these session-local skills:

- `research-os-doc-search`: acquire scoped sources, PDFs, code/artifact status, and visual assets.
- `research-os-wiki-update`: integrate raw/artifact/source evidence into the durable session wiki.
- `research-os-synthesis`: create cross-source maps, trends, conflicts, gaps, and taxonomies.
- `research-os-qa`: answer evidence-backed questions from the session wiki.
- `research-os-discussion`: support lightweight technical discussion from the session wiki.
- `research-os-ideation`: generate research ideas grounded in wiki evidence.
- `research-os-writing`: draft memos, related work, reports, or proposals.
- `research-os-visualization`: create cited tables, diagrams, timelines, or visual summaries.
- `research-os-coding`: implement code related to the research task when explicitly requested. Use direct repo edits for product features/fixes, and use `sessions/{session_id}/artifacts/implementation/{turn_id}/` for research prototypes, demos, analysis scripts, parsers, simulations, and generated code artifacts.
- `research-os-experiment-planning`: design experiments grounded in session evidence.

Do not select deprecated run-era acquisition/reader skills for new session plans: `research-os-search`, `research-os-source-triage`, `research-os-ingest`, or `research-os-reader`.

## Standard Pipeline Patterns

Knowledge building:

```text
research-os-doc-search
research-os-wiki-update
```

Knowledge using:

```text
research-os-qa | research-os-discussion | research-os-ideation | research-os-writing | research-os-visualization | research-os-coding | research-os-experiment-planning
```

Hybrid:

```text
research-os-doc-search
research-os-wiki-update
task-specific knowledge-using skill
```

Add `research-os-synthesis` when the user asks for trend analysis, a research map, comparative conclusions, taxonomy, report, or other cross-source analytical output.

## Plan JSON Shape

```json
{
  "session_id": "session-id",
  "turn_id": "turn-id",
  "user_instruction": "...",
  "turn_type": "plan",
  "task_type": "knowledge_building|knowledge_using|hybrid",
  "wiki_sufficiency": {
    "sufficient": false,
    "reason": "What is missing or sufficient."
  },
  "selected_agents": ["research-os-doc-search", "research-os-wiki-update"],
  "selection_rationale": "Why these agents are needed.",
  "source_scope": {
    "mode": "wiki_only|web|academic|local_library|fixed_collection|venue_scope",
    "allowed_sources": [],
    "disallowed_sources": [],
    "allowed_source_types": [],
    "file_acquisition_policy": "Matching PDFs/full text may be acquired after source identity is confirmed.",
    "recency": "as specified or default",
    "expansion_policy": "ask_user_before_expanding|allowed|forbidden"
  },
  "stages": [
    {
      "name": "search",
      "agent_skill": "research-os-doc-search",
      "reason": "Need fresh source acquisition.",
      "inputs": ["sessions/{session_id}/wiki/index.md"],
      "outputs": ["sessions/{session_id}/doc_search_manifest.json"],
      "may_mutate_wiki": false
    }
  ],
  "requires_user_confirmation": true,
  "questions": [
    {
      "question": "Proceed with this pipeline, or narrow the source scope?",
      "options": ["Proceed with proposed pipeline", "Narrow source scope", "Revise output goal"]
    }
  ]
}
```

Use only session-local artifact roots in planned inputs/outputs:

- `sessions/{session_id}/plan/`
- `sessions/{session_id}/raw/`
- `sessions/{session_id}/artifacts/`
- `sessions/{session_id}/artifacts/implementation/`
- `sessions/{session_id}/assets/`
- `sessions/{session_id}/schemas/`
- `sessions/{session_id}/wiki/`

## Conversation Output

Keep the visible plan concise. Before every `PLAN_CONFIRM`, always show the current plan summary and the agent pipeline that will run if the user proceeds.

```text
Plan summary
- Goal: ...
- Scope: ...
- Wiki sufficiency: ...
- Expected outputs: ...

Proposed pipeline
1. doc-search — why
2. wiki-update — why
3. synthesis — why

PLAN_CONFIRM: Proceed with this pipeline? || Proceed with proposed pipeline || Continue planning || Revise output goal
```

Ask only the questions needed to make the pipeline executable. If the user already supplied enough constraints, ask for final confirmation rather than adding extra questions.

For a blocking ambiguity, use this exact format on its own line:

```text
PLAN_QUESTION: question text || recommended option || second option || third option
```

For final proceed/continue confirmation, first show `Plan summary` and `Proposed pipeline`, then use this exact format on its own line:

```text
PLAN_CONFIRM: Proceed with this plan? || Proceed with this plan || Continue planning || Revise scope
```

Rules:

- Provide 2-3 plausible options.
- Put the recommended option first.
- Do not include an "Other" option.
- Do not ask free-form questions unless the plan is impossible to express with options.
- Do not ask repeated clarification questions after the plan is implementation-ready; use `PLAN_CONFIRM` instead.
- After receiving a final confirmation response that approves proceeding, finalize the plan and do not ask another question.
- Never emit `PLAN_CONFIRM` by itself; the immediately preceding visible output must include both the current plan summary and the agent pipeline.
