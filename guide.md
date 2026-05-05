# research-os Initial Guideline

## Purpose

research-os is a research-purpose orchestrator.

Its goal is not to be a simple search-and-summarize agent, but to build, maintain, and use a sustainable research state.

research-os turns raw sources into structured, citation-aware research artifacts, maintains them as an LLM-friendly research wiki, and uses that wiki to support research Q&A, synthesis, discussion, ideation, writing, visualization, coding, and PoC experiment planning.

## Example Input

> Please research recent semiconductor HBM research.

## Core Idea

research-os operates in two major modes:

1. Knowledge Building
2. Knowledge Using

Knowledge Building creates and updates the persistent research state.

Knowledge Using consumes the research state to answer questions, generate ideas, write documents, create visualizations, implement code, or plan experiments.

```text
raw sources
→ source notes
→ concept / method / dataset / benchmark pages
→ synthesis pages
→ followups / index / log
→ downstream research tasks
```

## Research-OS Orchestrator

The Research-OS Orchestrator receives a user instruction and decides:

- what the user is asking for
- whether the task requires new knowledge building
- which predefined agents should be executed
- in what order agents should run
- what artifacts each agent should produce
- whether the wiki should be updated after the task

The orchestrator should not directly perform all research work itself.
It should route work to specialized agents with clear responsibilities and artifact contracts.

---

# Agent Categories

research-os separates agents into two categories:

1. Knowledge-building agents
2. Knowledge-using agents

## 1. Knowledge-building agents

Knowledge-building agents create, structure, validate, and update the persistent research state.

They are responsible for:

- finding sources
- triaging sources
- ingesting raw materials
- reading sources
- extracting structured source notes
- updating the wiki
- synthesizing multi-source findings
- validating citations, conflicts, and coverage gaps

Default knowledge-building pipeline:

```text
Planner
→ Search
→ Source Triage
→ Ingest
→ Reader
→ Wiki Update
→ Synthesis
→ Wiki Update
→ Lint / Critic
```

## 2. Knowledge-using agents

Knowledge-using agents consume the research state to perform downstream research tasks.

They are responsible for:

- research Q&A
- technical discussion
- hypothesis generation
- ideation
- writing literature reviews, proposals, or reports
- visualization
- code implementation
- PoC / experiment planning

Knowledge-using agents must search and read the wiki first before using raw sources or external search.

If the wiki is insufficient, stale, or missing important coverage, the knowledge-using agent should request an additional knowledge-building pipeline run.

---

# Knowledge-Building Agents

## 1. Planner Agent

### Purpose

Plan the required pipeline for the given research instruction using predefined sub-agents.

### Input

- User instruction
- Existing wiki state if available
- Available source scope, such as web sources or fixed library
- User constraints, such as recency, source type, depth, or output format

### Output

- `runs/{run_id}/plan.json`

### Responsibilities

- Identify research intent.
- Break the instruction into subtopics.
- Decide source scope.
- Decide which agents to execute.
- Define expected outputs.
- Define inclusion and exclusion criteria.
- Decide whether this is a knowledge-building task, a knowledge-using task, or both.

### Must not

- Directly write final synthesis.
- Invent sources.
- Mutate the wiki.

---

## 2. Search Agent

### Purpose

Search for potentially relevant sources.

### Input

- `runs/{run_id}/plan.json`

### Output

- `runs/{run_id}/search_results.json`

### Responsibilities

- Search web sources, academic sources, and/or fixed local library.
- Collect source metadata:
  - title
  - authors or organization
  - date
  - URL or local path
  - source type
  - abstract or snippet
  - venue or publisher if available
- Deduplicate obvious duplicates.

### Must not

- Treat search results as verified knowledge.
- Update the wiki directly.
- Produce final research conclusions.

---

## 3. Source Triage Agent

### Purpose

Select the sources that should be ingested and read.

### Input

- `runs/{run_id}/search_results.json`
- `runs/{run_id}/plan.json`

### Output

- `runs/{run_id}/selected_sources.json`

### Responsibilities

- Rank sources by:
  - relevance
  - credibility
  - recency
  - diversity
  - expected usefulness
  - source type
- Select a balanced set of sources.
- Identify missing coverage areas.
- Explain why each selected source matters.

### Must not

- Prefer recency blindly over foundational importance.
- Summarize the field without reading selected sources.
- Mutate the wiki.

---

## 4. Ingest Agent

### Purpose

Bring selected sources into the local research workspace.

### Input

- `runs/{run_id}/selected_sources.json`

### Output

- raw sources under `raw/`
- parsed text under `parsed/`
- `runs/{run_id}/ingest_manifest.json`

### Responsibilities

- Download or import PDFs, web pages, reports, code, slides, or other source types.
- Preserve raw sources in immutable form.
- Extract text and metadata where possible.
- Record provenance:
  - original URL
  - access date
  - local raw path
  - parsed path
  - extraction method
  - known extraction failures

### Must not

- Modify raw sources.
- Create high-level synthesis.
- Mutate concept, method, or synthesis pages.

---

## 5. Reader Agent

### Purpose

Read each ingested source and create a structured source note.

### Input

- Parsed source text
- Raw source metadata
- User research question or plan context

### Output

- `wiki/sources/{source_id}.md`

### Responsibilities

Extract:

- main claims
- problem statement
- contributions
- methods
- datasets
- benchmarks
- metrics
- findings
- evidence
- limitations
- future work
- interesting details
- relevance to the user’s research question
- possible links to existing concepts

### Must

- Separate what the source claims from what the agent infers.
- Include location references when available:
  - page
  - section
  - figure
  - table
  - paragraph
- Preserve uncertainty.

### Must not

- Generalize across multiple sources.
- Update concept or synthesis pages directly.
- Add unsupported claims.

---

## 6. Wiki Update Agent

### Purpose

Update the persistent research state.

Wiki Update is a first-class stage because it mutates the durable research state.

### Input

- Newly created or updated source notes
- Existing wiki pages
- Optional synthesis output
- Optional downstream task artifacts

### Output

- Updated wiki pages
- `runs/{run_id}/wiki_update_report.md`

### Responsibilities

- Update concept pages.
- Update method pages.
- Update dataset pages.
- Update benchmark pages.
- Update synthesis indexes.
- Update `index.md`.
- Update `followups.md`.
- Update `log.md`.
- Link source notes to relevant wiki pages.
- Create new pages when recurring concepts do not yet exist.
- Record relations between claims:
  - supports
  - contradicts
  - refines
  - extends
  - duplicates

### Must

- Run after every knowledge-producing stage, especially after Reader and Synthesis.
- Keep raw source claims and agent interpretation separate.
- Preserve traceability from wiki claims to source notes.
- Prefer conservative updates.
- Record all meaningful wiki mutations in `log.md`.

### Must not

- Silently delete existing claims.
- Overwrite conflicting claims without marking the conflict.
- Add uncited technical claims to concept or synthesis pages.
- Let other agents directly mutate persistent wiki pages unless explicitly delegated.

---

## 7. Synthesis Agent

### Purpose

Organize findings across multiple sources and wiki pages.

### Input

- Source notes
- Concept pages
- Method pages
- Dataset or benchmark pages
- Previous synthesis pages if available

### Output

- `wiki/synthesis/{topic}.md`
- Optional user-facing answer draft

### Responsibilities

- Compare recent works.
- Identify agreements and disagreements.
- Identify research trends.
- Identify technical bottlenecks.
- Identify research gaps and unresolved questions.
- Produce tables, taxonomies, timelines, diagrams, or research maps when useful.
- Suggest follow-up research directions.

### Must

- Cite source notes for non-trivial claims.
- Clearly mark uncertainty and weak evidence.
- Distinguish source claims from synthesis-level interpretation.

### Must not

- Produce claims unsupported by source notes or wiki pages.
- Use raw search results as evidence.
- Skip wiki update after producing important synthesis.

---

## 8. Lint / Critic Agent

### Purpose

Validate the research state and final outputs.

### Input

- Updated wiki
- Source notes
- Synthesis output
- Optional final answer draft

### Output

- `runs/{run_id}/lint_report.md`

### Responsibilities

Check for:

- missing citations
- unsupported claims
- overclaims
- contradictions
- stale claims
- source coverage gaps
- broken links
- duplicate concepts
- weak evidence
- unclear separation between source claims and agent inference

### Must

- Suggest concrete fixes.
- Flag uncertainty clearly.
- Recommend follow-up research where coverage is weak.

### Must not

- Silently rewrite the wiki without reporting changes.
- Delete claims without preserving history or rationale.

---

# Knowledge-Using Agents

Knowledge-using agents operate on the research wiki and existing artifacts.

They should not treat external search as the first step unless the wiki is missing, stale, or insufficient.

Default knowledge-using flow:

```text
User task
→ retrieve relevant wiki pages
→ inspect source notes if needed
→ determine whether existing research state is sufficient
→ perform task
→ optionally request Wiki Update
```

## Research Q&A Agent

Answers user questions using the research wiki.

Must:

- search wiki first
- cite relevant source notes
- state uncertainty
- request additional knowledge-building if coverage is insufficient

## Discussion Agent

Helps reason through technical tradeoffs, interpretations, and research implications.

Must:

- distinguish evidence-backed claims from speculation
- point to unresolved questions
- suggest what would change the conclusion

## Ideation Agent

Generates research ideas, hypotheses, project directions, or paper angles.

Must:

- ground ideas in known gaps or tensions from the wiki
- mark speculative ideas clearly
- optionally send promising ideas to Wiki Update

## Writing Agent

Produces research memos, literature reviews, related work sections, proposals, or reports.

Must:

- use source notes and synthesis pages
- preserve citation traceability
- avoid claims not backed by research state

## Visualization Agent

Creates charts, diagrams, tables, or visual summaries.

Must:

- identify the data source for each visual element
- avoid visualizing unsupported numeric claims
- save visualization specs or outputs as artifacts when useful

## Coding Agent

Implements code related to the research task, such as parsers, analysis scripts, demos, simulations, or PoC tools.

Must:

- read relevant wiki context before coding
- connect implementation choices to research goals
- save code artifacts in a reproducible structure

## PoC / Experiment Planning Agent

Designs proof-of-concept experiments, evaluation plans, benchmarks, or ablation studies.

Must:

- ground experiment plans in research gaps or claims from the wiki
- define hypothesis, variables, metrics, dataset, baseline, and expected failure modes
- mark speculative assumptions clearly

---

# Key Design Principles

## 1. Research state over one-shot answers

research-os should build reusable research state, not just produce one-time summaries.

## 2. Raw sources are immutable

Raw sources are the source of truth and should be preserved.

## 3. Wiki is the structured research state

The wiki contains source notes, concept pages, method pages, dataset pages, benchmark pages, synthesis pages, follow-ups, indexes, and logs.

## 4. Wiki Update is a first-class stage

Wiki Update is separated from Reader and Synthesis because it mutates persistent research state.

It has two purposes:

1. Compression
   Reduce context and memory load by turning source content into structured notes.

2. Organization
   Update the persistent research state so future agents can reuse it.

## 5. Every important claim must be traceable

Every non-trivial technical claim in concept or synthesis pages should point back to a source note or raw source reference.

## 6. Knowledge-building and knowledge-using are separate

Knowledge-building agents create and update the research state.

Knowledge-using agents consume the research state to perform downstream research tasks.

## 7. Wiki-first downstream behavior

Before answering, coding, visualizing, writing, discussing, or planning experiments, agents should search and read the wiki first.

If the wiki is insufficient, the orchestrator should plan additional search and ingest steps.

## 8. Conservative mutation

Only Wiki Update Agent should directly mutate persistent wiki pages by default.

Other agents produce artifacts or proposed changes.
Wiki Update Agent decides how to incorporate them.

## 9. Auditability

Every run should leave artifacts under `runs/{run_id}/`.

Every meaningful wiki change should be traceable through:

- source note
- wiki update report
- log entry
- final output if applicable

---

# Suggested Directory Structure

```text
research-os/
  AGENTS.md
  config.yaml
  README.md

  raw/
    papers/
    web/
    reports/
    code/
    slides/

  parsed/
    papers/
    web/
    reports/

  wiki/
    index.md
    log.md
    followups.md

    sources/
    concepts/
    methods/
    datasets/
    benchmarks/
    synthesis/
    ideas/
    experiments/
    artifacts/

  runs/
    {run_id}/
      plan.json
      search_results.json
      selected_sources.json
      ingest_manifest.json
      wiki_update_report.md
      lint_report.md
      final_answer.md

  scripts/
    search/
    ingest/
    parse/
    lint/
```

---

# Default Pipelines

## Knowledge-Building Pipeline

```text
User instruction
→ Planner
→ Search
→ Source Triage
→ Ingest
→ Reader
→ Wiki Update
→ Synthesis
→ Wiki Update
→ Lint / Critic
→ Final response
```

## Knowledge-Using Pipeline

```text
User instruction
→ Planner
→ Wiki Retrieval
→ Task-specific Knowledge-Using Agent
→ Optional Wiki Update
→ Final response
```

## Hybrid Pipeline

Used when the user asks a downstream task but the wiki lacks enough information.

```text
User instruction
→ Planner
→ Wiki Retrieval
→ coverage gap detected
→ Search
→ Source Triage
→ Ingest
→ Reader
→ Wiki Update
→ Task-specific Knowledge-Using Agent
→ Optional Synthesis
→ Lint / Critic
→ Final response
```

---

# Minimal Invariants

1. No final research answer should be produced directly from raw search results.

2. A final research answer should be based on:
   - source notes
   - updated wiki pages
   - synthesis output
   - critic/lint feedback when available

3. Raw sources must remain immutable.

4. Every non-trivial claim in concept or synthesis pages must be traceable to a source note.

5. Wiki Update Agent is the default owner of persistent research state mutation.

6. Knowledge-using agents must read the wiki first.

7. If the wiki is insufficient, run or request knowledge building before giving a confident answer.

