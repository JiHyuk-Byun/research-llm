//! Local MCP sidecar (stdio) exposing research-os's custom domain tools to a
//! `claude -p` stage via `--mcp-config`. Started as the hidden subcommand
//! `research-os __mcp <session_id>` — a child process of the claude stage, with
//! cwd at the workspace root.
//!
//! Phase 2b-1: file-based tools only (`ledger_read`, `propose_experiment`) that
//! act directly on `sessions/{id}/ledger.json` via the [`crate::ledger`] model.
//! Tools that need the human (`checkpoint_ask`) require IPC back to the running
//! TUI and arrive in a later sub-step.

use std::path::{Path, PathBuf};

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::transport::stdio;
use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt};
use serde::Deserialize;

use crate::ledger::{
    self, DecisionAction, DecisionBy, Experiment, ExperimentStatus, Hypothesis, HypothesisStatus,
    Ledger, Outcome, Phase, Proposal, ProposalStatus, RouteDecision,
};

#[derive(Clone)]
struct Sidecar {
    root: PathBuf,
    session_id: String,
    tool_router: ToolRouter<Sidecar>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct LedgerReadParams {
    /// Optional section to return: phases | decisions | hypotheses | proposals |
    /// experiments. Omit to return the whole ledger.
    #[serde(default)]
    section: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProposeExperimentParams {
    /// The testable hypothesis / question to record.
    hypothesis: String,
    /// Why this is worth testing.
    #[serde(default)]
    rationale: Option<String>,
    /// Wiki source ids or refs backing the hypothesis (e.g. src:...).
    #[serde(default)]
    evidence_refs: Option<Vec<String>>,
    /// A rough experiment design sketch; required before EXPERIMENT can start.
    #[serde(default)]
    rough_design: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RelatesToParam {
    /// A wiki source id or page reference this finding relates to (e.g. src:...).
    target: String,
    /// supports | contradicts | refines | extends | duplicates.
    #[serde(default)]
    relation: Option<String>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CaptureResultsParams {
    /// Existing ledger experiment id to mark done; omit to create a new entry.
    #[serde(default)]
    exp_id: Option<String>,
    /// Ledger hypothesis id this experiment tested, if known (its status is
    /// updated from the outcome).
    #[serde(default)]
    hypothesis_id: Option<String>,
    /// The question / hypothesis the experiment tested.
    question: String,
    /// How the experiment was set up to answer it.
    setup: String,
    /// What was observed (reference metrics and figures).
    result: String,
    /// Interpretation: what it means for the hypothesis, caveats.
    analysis: String,
    /// supports | contradicts | inconclusive.
    outcome: String,
    /// Headline metrics as a JSON object (name -> value).
    #[serde(default)]
    key_metrics: Option<serde_json::Value>,
    /// Figure image paths relative to the session root, embedded in the note.
    #[serde(default)]
    figure_paths: Option<Vec<String>>,
    /// Prior wiki sources this finding supports/contradicts/refines.
    #[serde(default)]
    relates_to: Option<Vec<RelatesToParam>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CheckpointOption {
    /// Display label for this choice (put the recommended option first).
    label: String,
    /// The transition this option routes to: STAY, ADVANCE, BRANCH, or STOP.
    action: String,
    /// For BRANCH: the target phase (INIT, DISCUSS, EXPERIMENT, POST).
    #[serde(default)]
    target_phase: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CheckpointAskParams {
    /// The phase-transition / checkpoint question to put to the user.
    question: String,
    /// Your self-assessment / why you're asking now (shown to the user).
    #[serde(default)]
    assessment: Option<String>,
    /// The options to choose from; put your recommended option first. Each
    /// option carries the transition it routes to.
    options: Vec<CheckpointOption>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PhaseRouteParams {
    /// The transition to take without asking: STAY, ADVANCE, or BRANCH.
    action: String,
    /// For BRANCH: the target phase (INIT, DISCUSS, EXPERIMENT, POST).
    #[serde(default)]
    target_phase: Option<String>,
    /// Why you're routing this way (recorded in the decision log).
    reason: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GraphQueryParams {
    /// One of: neighbors, contradictions, orphans, impact.
    query: String,
    /// Node id for neighbors / impact queries (a source_id, hypothesis, or
    /// experiment id).
    #[serde(default)]
    node: Option<String>,
}

#[tool_router]
impl Sidecar {
    #[tool(
        description = "Read this session's experiment ledger (working-memory / loop state) as JSON. Optionally pass a section name (phases|decisions|hypotheses|proposals|experiments) to return only that slice."
    )]
    fn ledger_read(
        &self,
        Parameters(p): Parameters<LedgerReadParams>,
    ) -> Result<CallToolResult, McpError> {
        let l = self.load();
        let text = match p.section.as_deref() {
            Some("hypotheses") => serde_json::to_string_pretty(&l.hypotheses),
            Some("proposals") => serde_json::to_string_pretty(&l.proposals),
            Some("experiments") => serde_json::to_string_pretty(&l.experiments),
            Some("decisions") => serde_json::to_string_pretty(&l.decisions),
            Some("phases") => serde_json::to_string_pretty(&l.phases),
            _ => Ok(l.to_json()),
        }
        .unwrap_or_else(|_| "{}".to_string());
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Record a testable hypothesis and an experiment proposal in the ledger (the DISCUSS->EXPERIMENT bridge). Does not run anything. Returns the new hypothesis and proposal ids."
    )]
    fn propose_experiment(
        &self,
        Parameters(p): Parameters<ProposeExperimentParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut l = self.load();
        let ts = ledger::now_ms().to_string();
        let hyp_id = format!("hyp-{}", l.hypotheses.len() + 1);
        let prop_id = format!("prop-{}", l.proposals.len() + 1);
        let evidence = p.evidence_refs.unwrap_or_default();
        l.hypotheses.push(Hypothesis {
            id: hyp_id.clone(),
            statement: p.hypothesis,
            evidence_refs: evidence.clone(),
            status: HypothesisStatus::Open,
            created_at: Some(ts.clone()),
            updated_at: None,
        });
        l.proposals.push(Proposal {
            id: prop_id.clone(),
            hypothesis_id: hyp_id.clone(),
            rationale: p.rationale,
            rough_design: p.rough_design,
            evidence_refs: evidence,
            status: ProposalStatus::Proposed,
            created_at: Some(ts),
            decided_at: None,
        });
        self.save(&l)
            .map_err(|e| McpError::internal_error(format!("save ledger: {e}"), None))?;
        let out = format!("{{\"hypothesis_id\":\"{hyp_id}\",\"proposal_id\":\"{prop_id}\"}}");
        Ok(CallToolResult::success(vec![Content::text(out)]))
    }

    #[tool(
        description = "Promote a toy experiment's findings into the wiki as a citable (Question, Setup, Result, Analysis) note (source_type: experiment), append the wiki log, and update the ledger (experiment -> done; hypothesis status from the outcome). This is how an experiment result becomes durable, citable knowledge. Returns the new wiki source note path and experiment id."
    )]
    fn capture_results(
        &self,
        Parameters(p): Parameters<CaptureResultsParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut l = self.load();
        let exp_id = p
            .exp_id
            .clone()
            .unwrap_or_else(|| format!("exp-{}", l.experiments.len() + 1));
        let ts = ledger::now_ms().to_string();

        // 1. Render and write the citable wiki source note (the promoted post).
        // exp_id already carries the `exp-` prefix.
        let note_rel = format!("wiki/sources/{exp_id}.md");
        let note_path = self.session_root().join(&note_rel);
        write_file(&note_path, &render_experiment_note(&exp_id, &ts, &p))
            .map_err(|e| McpError::internal_error(format!("write note: {e}"), None))?;

        // 2. Append a wiki log line.
        let _ = append_log(
            &self.session_root(),
            &format!(
                "## [{ts}] capture | experiment {exp_id} | outcome: {}",
                p.outcome
            ),
        );

        // 3. Update the ledger: experiment -> done; hypothesis status from outcome.
        let outcome = parse_outcome(&p.outcome);
        if let Some(e) = l.experiment_mut(&exp_id) {
            e.status = ExperimentStatus::Done;
            e.outcome = outcome;
            e.source_note_path = Some(note_rel.clone());
            e.updated_at = Some(ts.clone());
        } else {
            l.experiments.push(Experiment {
                id: exp_id.clone(),
                proposal_id: None,
                hypothesis_id: p.hypothesis_id.clone().unwrap_or_default(),
                turn_id: None,
                record_dir: Some(format!("experiments/{exp_id}")),
                status: ExperimentStatus::Done,
                outcome,
                post_path: None,
                source_note_path: Some(note_rel.clone()),
                created_at: Some(ts.clone()),
                updated_at: Some(ts.clone()),
            });
        }
        if let Some(hyp_id) = &p.hypothesis_id {
            if let Some(h) = l.hypotheses.iter_mut().find(|h| &h.id == hyp_id) {
                h.status = hypothesis_status_from_outcome(&p.outcome);
                h.updated_at = Some(ts.clone());
            }
        }
        self.save(&l)
            .map_err(|e| McpError::internal_error(format!("save ledger: {e}"), None))?;

        // Rebuild the derived graph so the new experiment note + its relations
        // are immediately queryable.
        let g = crate::graph::build_graph(&self.session_root(), &l);
        let _ = g.save(&crate::graph::graph_path(&self.session_root()));

        let out = format!("{{\"exp_id\":\"{exp_id}\",\"source_note_path\":\"{note_rel}\"}}");
        Ok(CallToolResult::success(vec![Content::text(out)]))
    }

    #[tool(
        description = "Report structured coverage / gap signals for this session: count of wiki source notes, and ledger hypotheses/proposals/experiments by status. Used to judge whether a phase boundary is plausibly reached."
    )]
    fn coverage_report(&self) -> Result<CallToolResult, McpError> {
        let l = self.load();
        let source_count = count_markdown(&self.session_root().join("wiki").join("sources"));
        let pending_proposals = l
            .proposals
            .iter()
            .filter(|p| matches!(p.status, ProposalStatus::Proposed))
            .count();
        let experiments_has_results = l
            .experiments
            .iter()
            .filter(|e| matches!(e.status, ExperimentStatus::HasResults))
            .count();
        let open_hypotheses = l
            .hypotheses
            .iter()
            .filter(|h| matches!(h.status, HypothesisStatus::Open | HypothesisStatus::UnderTest))
            .count();
        let json = serde_json::json!({
            "current_phase": l.current_phase,
            "source_count": source_count,
            "pending_proposals": pending_proposals,
            "experiments_has_results": experiments_has_results,
            "experiments_total": l.experiments.len(),
            "open_hypotheses": open_hypotheses,
        });
        Ok(CallToolResult::success(vec![Content::text(json.to_string())]))
    }

    #[tool(
        description = "Ask the human a phase-transition / checkpoint question and BLOCK until they choose. Put the recommended option first. Returns the chosen index and label as JSON. Requires the research-os TUI to be running (it renders the question)."
    )]
    fn checkpoint_ask(
        &self,
        Parameters(p): Parameters<CheckpointAskParams>,
    ) -> Result<CallToolResult, McpError> {
        let labels: Vec<String> = p.options.iter().map(|o| o.label.clone()).collect();
        let req = crate::checkpoint_ipc::CheckpointRequest {
            question: p.question.clone(),
            assessment: p.assessment,
            options: labels,
        };
        let r = crate::checkpoint_ipc::ask(&self.root, &req).map_err(|e| {
            McpError::internal_error(
                format!("checkpoint_ask failed: {e} (is the research-os TUI running?)"),
                None,
            )
        })?;
        // Record the route the human chose for the driver to act on.
        let chosen = p.options.get(r.chosen);
        let action = chosen
            .and_then(|o| parse_action(&o.action))
            .unwrap_or(DecisionAction::Stay);
        let target = chosen
            .and_then(|o| o.target_phase.as_deref())
            .and_then(parse_phase);
        let mut l = self.load();
        l.pending_route = Some(RouteDecision {
            action,
            target_phase: target,
            by: DecisionBy::Human,
            reason: None,
            question: Some(p.question),
            chosen_label: Some(r.label.clone()),
            at: ledger::now_ms().to_string(),
        });
        self.save(&l)
            .map_err(|e| McpError::internal_error(format!("save ledger: {e}"), None))?;
        let out = format!(
            "{{\"chosen\":{},\"label\":\"{}\"}}",
            r.chosen,
            r.label.replace('"', "'")
        );
        Ok(CallToolResult::success(vec![Content::text(out)]))
    }

    #[tool(
        description = "Low-stakes self-routing WITHOUT asking the human: record the chosen phase transition (STAY, ADVANCE, or BRANCH) in the ledger for the driver to act on. Only granted on low-stakes boundaries; high-stakes transitions (starting an experiment, committing to the wiki, stopping) must use checkpoint_ask instead."
    )]
    fn phase_route(
        &self,
        Parameters(p): Parameters<PhaseRouteParams>,
    ) -> Result<CallToolResult, McpError> {
        let action = parse_action(&p.action).ok_or_else(|| {
            McpError::internal_error(format!("invalid action: {}", p.action), None)
        })?;
        let target = p.target_phase.as_deref().and_then(parse_phase);
        let mut l = self.load();
        l.pending_route = Some(RouteDecision {
            action,
            target_phase: target,
            by: DecisionBy::Llm,
            reason: Some(p.reason),
            question: None,
            chosen_label: None,
            at: ledger::now_ms().to_string(),
        });
        self.save(&l)
            .map_err(|e| McpError::internal_error(format!("save ledger: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            "{\"routed\":true}".to_string(),
        )]))
    }

    #[tool(
        description = "Query the wiki relationship graph (read-only). query is one of: neighbors, contradictions, orphans, impact. neighbors/impact need a node id (a source_id, hypothesis, or experiment id; impact = edges pointing AT the node). Returns matching edges/nodes as JSON. Use it to expand discussion — e.g. find what a new finding contradicts, or what depends on a source."
    )]
    fn graph_query(
        &self,
        Parameters(p): Parameters<GraphQueryParams>,
    ) -> Result<CallToolResult, McpError> {
        let led = self.load();
        let g = crate::graph::build_graph(&self.session_root(), &led);
        let val = match p.query.trim().to_ascii_lowercase().as_str() {
            "contradictions" => serde_json::to_value(g.contradictions()).unwrap_or_default(),
            "orphans" => serde_json::to_value(g.orphans()).unwrap_or_default(),
            "neighbors" => {
                let id = p.node.as_deref().unwrap_or("");
                serde_json::to_value(g.neighbors(id)).unwrap_or_default()
            }
            "impact" => {
                let id = p.node.as_deref().unwrap_or("");
                let incoming: Vec<&crate::graph::Edge> =
                    g.edges.iter().filter(|e| e.to == id).collect();
                serde_json::to_value(incoming).unwrap_or_default()
            }
            other => serde_json::json!({
                "error": format!("unknown query '{other}'; use neighbors|contradictions|orphans|impact")
            }),
        };
        Ok(CallToolResult::success(vec![Content::text(val.to_string())]))
    }
}

fn parse_outcome(s: &str) -> Option<Outcome> {
    match s.trim().to_ascii_lowercase().as_str() {
        "supports" => Some(Outcome::Supports),
        "contradicts" => Some(Outcome::Contradicts),
        "inconclusive" => Some(Outcome::Inconclusive),
        _ => None,
    }
}

fn parse_action(s: &str) -> Option<DecisionAction> {
    match s.trim().to_ascii_uppercase().as_str() {
        "STAY" => Some(DecisionAction::Stay),
        "ADVANCE" => Some(DecisionAction::Advance),
        "BRANCH" => Some(DecisionAction::Branch),
        "STOP" => Some(DecisionAction::Stop),
        _ => None,
    }
}

fn parse_phase(s: &str) -> Option<Phase> {
    match s.trim().to_ascii_uppercase().as_str() {
        "INIT" => Some(Phase::Init),
        "DISCUSS" => Some(Phase::Discuss),
        "EXPERIMENT" => Some(Phase::Experiment),
        "POST" => Some(Phase::Post),
        _ => None,
    }
}

fn hypothesis_status_from_outcome(s: &str) -> HypothesisStatus {
    match s.trim().to_ascii_lowercase().as_str() {
        "supports" => HypothesisStatus::Supported,
        "contradicts" => HypothesisStatus::Refuted,
        _ => HypothesisStatus::Inconclusive,
    }
}

fn count_markdown(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map(|x| x == "md").unwrap_or(false))
                .count()
        })
        .unwrap_or(0)
}

fn write_file(path: &Path, body: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, body)
}

fn append_log(session_root: &Path, line: &str) -> std::io::Result<()> {
    let log = session_root.join("wiki").join("log.md");
    if let Some(parent) = log.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut existing = std::fs::read_to_string(&log).unwrap_or_default();
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str(line);
    existing.push('\n');
    std::fs::write(&log, existing)
}

fn escape_yaml(s: &str) -> String {
    s.replace('"', "'").replace('\n', " ")
}

/// Render the citable wiki source note (source_type: experiment) for a captured
/// result. Frontmatter carries structured `relations` for the future graph
/// builder; the body is the (Q,S,R,A) post with embedded figures.
fn render_experiment_note(exp_id: &str, ts: &str, p: &CaptureResultsParams) -> String {
    let mut s = String::new();
    s.push_str("---\n");
    s.push_str(&format!("source_id: {exp_id}\n"));
    s.push_str(&format!(
        "title: \"Experiment {exp_id}: {}\"\n",
        escape_yaml(&p.question)
    ));
    s.push_str("source_type: experiment\n");
    s.push_str(&format!("date: \"{ts}\"\n"));
    s.push_str(&format!("outcome: {}\n", p.outcome));
    if let Some(hid) = &p.hypothesis_id {
        s.push_str(&format!("hypothesis_id: {hid}\n"));
    }
    if let Some(rel) = &p.relates_to {
        if !rel.is_empty() {
            s.push_str("relations:\n");
            for r in rel {
                let relation = r.relation.clone().unwrap_or_else(|| "relates_to".to_string());
                s.push_str(&format!("  - {{ type: {relation}, target: {} }}\n", r.target));
            }
        }
    }
    s.push_str("---\n\n");
    s.push_str(&format!("# Experiment {exp_id}\n\n"));
    s.push_str("## Question\n\n");
    s.push_str(p.question.trim());
    s.push_str("\n\n## Setup\n\n");
    s.push_str(p.setup.trim());
    s.push_str("\n\n## Result\n\n");
    s.push_str(p.result.trim());
    s.push_str("\n\n");
    if let Some(m) = &p.key_metrics {
        s.push_str("### Metrics\n\n```json\n");
        s.push_str(&serde_json::to_string_pretty(m).unwrap_or_default());
        s.push_str("\n```\n\n");
    }
    if let Some(figs) = &p.figure_paths {
        for f in figs {
            // The note lives at wiki/sources/; figure paths are session-relative.
            s.push_str(&format!("![figure](../../{f})\n\n"));
        }
    }
    s.push_str("## Analysis\n\n");
    s.push_str(p.analysis.trim());
    s.push_str(&format!("\n\n**Outcome:** {}\n", p.outcome));
    if let Some(rel) = &p.relates_to {
        if !rel.is_empty() {
            s.push_str("\n## Relates to\n\n");
            for r in rel {
                let relation = r.relation.clone().unwrap_or_else(|| "relates_to".to_string());
                let note = r.note.clone().unwrap_or_default();
                s.push_str(&format!("- {relation} `{}` {note}\n", r.target));
            }
        }
    }
    s
}

#[tool_handler]
impl ServerHandler for Sidecar {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            instructions: Some(
                "research-os session sidecar: ledger and experiment-loop tools.".to_string(),
            ),
            ..Default::default()
        }
    }
}

impl Sidecar {
    fn new(root: PathBuf, session_id: String) -> Self {
        Self {
            root,
            session_id,
            tool_router: Self::tool_router(),
        }
    }

    fn ledger_path(&self) -> PathBuf {
        ledger::ledger_path(&self.root, &self.session_id)
    }

    fn session_root(&self) -> PathBuf {
        self.root.join("sessions").join(&self.session_id)
    }

    fn load(&self) -> Ledger {
        Ledger::load_or_new(&self.ledger_path(), &self.session_id)
            .unwrap_or_else(|_| Ledger::new(&self.session_id))
    }

    fn save(&self, l: &Ledger) -> std::io::Result<()> {
        l.save(&self.ledger_path())
    }
}

/// Entry point for the `research-os __mcp <session_id>` subcommand. Serves MCP
/// over stdio; must not write anything else to stdout.
pub fn run(root: &Path, session_id: Option<String>) -> Result<(), String> {
    let session_id =
        session_id.ok_or_else(|| "usage: research-os __mcp <session_id>".to_string())?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    rt.block_on(async move {
        let service = Sidecar::new(root.to_path_buf(), session_id)
            .serve(stdio())
            .await
            .map_err(|e| e.to_string())?;
        service.waiting().await.map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(outcome: &str) -> CaptureResultsParams {
        CaptureResultsParams {
            exp_id: None,
            hypothesis_id: Some("hyp-1".to_string()),
            question: "Q".to_string(),
            setup: "S".to_string(),
            result: "R".to_string(),
            analysis: "A".to_string(),
            outcome: outcome.to_string(),
            key_metrics: None,
            figure_paths: None,
            relates_to: Some(vec![RelatesToParam {
                target: "src:x".to_string(),
                relation: Some("refines".to_string()),
                note: None,
            }]),
        }
    }

    #[test]
    fn note_renders_qsra_with_experiment_type_and_relations() {
        let md = render_experiment_note("exp-1", "t0", &params("supports"));
        assert!(md.contains("source_type: experiment"));
        assert!(md.contains("source_id: exp-1"));
        assert!(md.contains("## Question") && md.contains("## Setup"));
        assert!(md.contains("## Result") && md.contains("## Analysis"));
        assert!(md.contains("type: refines, target: src:x"));
        assert!(md.contains("**Outcome:** supports"));
    }

    #[test]
    fn outcome_maps_to_enums_and_hypothesis_status() {
        assert!(matches!(parse_outcome("contradicts"), Some(Outcome::Contradicts)));
        assert!(matches!(parse_outcome("nope"), None));
        assert!(matches!(
            hypothesis_status_from_outcome("supports"),
            HypothesisStatus::Supported
        ));
        assert!(matches!(
            hypothesis_status_from_outcome("contradicts"),
            HypothesisStatus::Refuted
        ));
    }

    #[test]
    fn action_and_phase_parse_case_insensitively() {
        assert!(matches!(parse_action("advance"), Some(DecisionAction::Advance)));
        assert!(matches!(parse_action("BRANCH"), Some(DecisionAction::Branch)));
        assert!(matches!(parse_action(" stop "), Some(DecisionAction::Stop)));
        assert!(parse_action("nope").is_none());
        assert!(matches!(parse_phase("experiment"), Some(Phase::Experiment)));
        assert!(matches!(parse_phase("POST"), Some(Phase::Post)));
        assert!(parse_phase("bogus").is_none());
    }
}
