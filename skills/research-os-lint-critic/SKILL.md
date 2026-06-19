---
name: research-os-lint-critic
description: Validate research-os wiki and final artifacts for citations, unsupported claims, contradictions, stale claims, gaps, and broken links.
---

# Research-OS Lint / Critic

Read the updated run-local wiki, source/concept/entity/method/dataset/comparison pages, synthesis output, final answer draft if present, and `runs/{run_id}/plan/plan.json`. Write the lint artifact assigned in the plan, usually `runs/{run_id}/lint_report.md` or `runs/{run_id}/lint_report.json`.

## Check For

- missing citations
- unsupported claims
- overclaims
- contradictions
- stale claims
- source coverage gaps
- broken links
- duplicate or overlapping wiki pages
- weak evidence
- unclear separation between source claims and inference
- source-scope violations

Suggest concrete fixes. Do not silently rewrite the wiki.
