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

use crate::ledger::{self, Hypothesis, HypothesisStatus, Ledger, Proposal, ProposalStatus};

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
