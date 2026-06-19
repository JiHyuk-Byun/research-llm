//! Experiment ledger: the managed working-memory / loop-control state for a
//! research session, persisted as `sessions/{session_id}/ledger.json`.
//!
//! Integrity rule (enforced by callers, mirrored in the JSON schema): the
//! orchestration sections (`current_phase`, `phases`, `decisions`) are written
//! only by the Rust orchestrator; the content sections (`hypotheses`,
//! `proposals`, `experiments`) are written by the agent via MCP tools. This
//! module is the data model + load/save; gate logic lives in the phase driver.
//!
//! Schema: `schemas/experiment_ledger.schema.json`.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "1";

/// Unix epoch milliseconds, matching the turn-id convention used elsewhere.
pub fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// `sessions/{session_id}/ledger.json`.
pub fn ledger_path(root: &Path, session_id: &str) -> PathBuf {
    root.join("sessions").join(session_id).join("ledger.json")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Phase {
    Init,
    Discuss,
    Experiment,
    Post,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum DecisionAction {
    Stay,
    Advance,
    Branch,
    Stop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisStatus {
    Open,
    UnderTest,
    Supported,
    Refuted,
    Inconclusive,
    Parked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Proposed,
    Accepted,
    Declined,
    Superseded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentStatus {
    Planned,
    Running,
    HasResults,
    Analyzed,
    Done,
    Abandoned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Supports,
    Contradicts,
    Inconclusive,
}

/// Per-phase orchestration state (Rust-core only) used for checkpoint
/// hysteresis: don't re-ask while the signal is unchanged and last was STAY.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PhaseState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_signal_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_decision: Option<DecisionAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checkpoint_at: Option<String>,
    #[serde(default)]
    pub visits: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chosen {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub action: DecisionAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_phase: Option<Phase>,
}

/// Who made a checkpoint decision. `phase_route` self-decisions are `Llm`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecisionBy {
    Human,
    Llm,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    pub phase: Phase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    pub chosen: Chosen,
    pub by: DecisionBy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hypothesis {
    pub id: String,
    pub statement: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,
    pub status: HypothesisStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub hypothesis_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rough_design: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,
    pub status: ProposalStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Experiment {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
    pub hypothesis_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_dir: Option<String>,
    pub status: ExperimentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_note_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// A routing decision produced by a checkpoint (by `checkpoint_ask` after a
/// human choice, or by `phase_route` for a low-stakes LLM self-decision). The
/// phase driver reads `Ledger::pending_route`, routes on it, records it into
/// `decisions`, and clears it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouteDecision {
    pub action: DecisionAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_phase: Option<Phase>,
    pub by: DecisionBy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chosen_label: Option<String>,
    pub at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ledger {
    pub session_id: String,
    pub schema_version: String,
    pub current_phase: Phase,
    #[serde(default)]
    pub phases: BTreeMap<String, PhaseState>,
    /// Set by a checkpoint tool, consumed and cleared by the driver.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_route: Option<RouteDecision>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<Decision>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hypotheses: Vec<Hypothesis>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proposals: Vec<Proposal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub experiments: Vec<Experiment>,
}

impl Ledger {
    /// A fresh ledger that starts in INIT.
    pub fn new(session_id: &str) -> Self {
        Ledger {
            session_id: session_id.to_string(),
            schema_version: SCHEMA_VERSION.to_string(),
            current_phase: Phase::Init,
            phases: BTreeMap::new(),
            pending_route: None,
            decisions: Vec::new(),
            hypotheses: Vec::new(),
            proposals: Vec::new(),
            experiments: Vec::new(),
        }
    }

    pub fn from_json(text: &str) -> serde_json::Result<Self> {
        serde_json::from_str(text)
    }

    pub fn to_json(&self) -> String {
        // Pretty so the ledger stays human-inspectable next to the wiki.
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Load from disk, or return a fresh INIT ledger if the file is absent.
    pub fn load_or_new(path: &Path, session_id: &str) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => Self::from_json(&text)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::new(session_id)),
            Err(e) => Err(e),
        }
    }

    /// Atomically write the ledger (temp file + rename) so a crash mid-write
    /// can't truncate it.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, self.to_json())?;
        fs::rename(&tmp, path)
    }

    pub fn experiment(&self, exp_id: &str) -> Option<&Experiment> {
        self.experiments.iter().find(|e| e.id == exp_id)
    }

    pub fn experiment_mut(&mut self, exp_id: &str) -> Option<&mut Experiment> {
        self.experiments.iter_mut().find(|e| e.id == exp_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Ledger {
        let mut l = Ledger::new("sess-1");
        l.current_phase = Phase::Discuss;
        l.hypotheses.push(Hypothesis {
            id: "hyp-1".into(),
            statement: "scoping helps".into(),
            evidence_refs: vec!["src:a".into()],
            status: HypothesisStatus::UnderTest,
            created_at: Some("t0".into()),
            updated_at: None,
        });
        l.proposals.push(Proposal {
            id: "prop-1".into(),
            hypothesis_id: "hyp-1".into(),
            rationale: Some("why".into()),
            rough_design: Some("design".into()),
            evidence_refs: vec![],
            status: ProposalStatus::Proposed,
            created_at: Some("t0".into()),
            decided_at: None,
        });
        l.experiments.push(Experiment {
            id: "exp-1".into(),
            proposal_id: Some("prop-1".into()),
            hypothesis_id: "hyp-1".into(),
            turn_id: None,
            record_dir: Some("experiments/exp-1".into()),
            status: ExperimentStatus::HasResults,
            outcome: None,
            post_path: None,
            source_note_path: None,
            created_at: Some("t0".into()),
            updated_at: None,
        });
        l
    }

    #[test]
    fn round_trips_through_json() {
        let l = sample();
        let json = l.to_json();
        let back = Ledger::from_json(&json).expect("parse");
        assert_eq!(back.session_id, "sess-1");
        assert_eq!(back.current_phase, Phase::Discuss);
        assert_eq!(back.hypotheses.len(), 1);
        assert_eq!(back.experiments[0].status, ExperimentStatus::HasResults);
    }

    #[test]
    fn enum_values_match_schema_strings() {
        // The JSON must use the schema's lowercase/uppercase string forms so
        // the on-disk ledger validates against experiment_ledger.schema.json.
        let json = sample().to_json();
        assert!(json.contains("\"current_phase\": \"DISCUSS\""));
        assert!(json.contains("\"status\": \"has_results\""));
        assert!(json.contains("\"status\": \"proposed\""));
        assert!(json.contains("\"status\": \"under_test\""));
    }

    #[test]
    fn missing_file_yields_fresh_init_ledger() {
        let dir = std::env::temp_dir().join(format!("ros-ledger-{}", now_ms()));
        let path = ledger_path(&dir, "sess-x");
        let l = Ledger::load_or_new(&path, "sess-x").expect("load_or_new");
        assert_eq!(l.current_phase, Phase::Init);
        assert_eq!(l.session_id, "sess-x");
    }

    #[test]
    fn save_then_load_is_stable() {
        let dir = std::env::temp_dir().join(format!("ros-ledger-{}", now_ms()));
        let path = ledger_path(&dir, "sess-y");
        let mut l = sample();
        l.session_id = "sess-y".into();
        l.save(&path).expect("save");
        let back = Ledger::load_or_new(&path, "sess-y").expect("load");
        assert_eq!(back.experiments.len(), 1);
        assert_eq!(back.proposals[0].status, ProposalStatus::Proposed);
        let _ = fs::remove_dir_all(&dir);
    }
}
