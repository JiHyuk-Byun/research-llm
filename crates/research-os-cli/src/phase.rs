//! Phase state machine + exit-signal computation for the research-loop driver.
//!
//! This is the *decision* logic, kept pure and testable: it reads only
//! mechanical facts from the ledger and filesystem (counts, statuses, file
//! existence) — never a quality judgment. The driver (execution) consumes
//! [`compute_exit_signal`] / [`should_checkpoint`] / [`next_phase`] to decide
//! when to surface a checkpoint and where to route.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use crate::ledger::{
    DecisionAction, ExperimentStatus, HypothesisStatus, Ledger, Phase, ProposalStatus,
};

/// Minimum wiki source notes before INIT is considered plausibly sufficient.
pub const INIT_MIN_SOURCES: usize = 3;

#[derive(Clone, Debug)]
pub struct SignalDetails {
    pub source_count: usize,
    pub pending_proposals: usize,
    pub experiments_has_results: usize,
    pub experiments_done: usize,
    pub open_hypotheses: usize,
}

#[derive(Clone, Debug)]
pub struct ExitSignal {
    /// Whether the phase's structured gate is met (worth offering a checkpoint).
    pub boundary_plausible: bool,
    /// Hash of the gate-relevant state, for hysteresis.
    pub signal_hash: String,
    pub details: SignalDetails,
}

/// Stable map key for a phase in `ledger.phases`.
pub fn phase_key(p: Phase) -> &'static str {
    match p {
        Phase::Init => "INIT",
        Phase::Discuss => "DISCUSS",
        Phase::Experiment => "EXPERIMENT",
        Phase::Post => "POST",
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

fn hash_str(s: &str) -> String {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Compute the structured exit signal for `phase`. Gate inputs are mechanical
/// (counts, statuses, file existence); the fuzzy judgment that produced them
/// (e.g. an agent calling propose_experiment) happened upstream and left a
/// structured trace this reads.
pub fn compute_exit_signal(phase: Phase, ledger: &Ledger, session_root: &Path) -> ExitSignal {
    let source_count = count_markdown(&session_root.join("wiki").join("sources"));
    let pending: Vec<&str> = ledger
        .proposals
        .iter()
        .filter(|p| p.status == ProposalStatus::Proposed && p.rough_design.is_some())
        .map(|p| p.id.as_str())
        .collect();
    let has_results: Vec<&str> = ledger
        .experiments
        .iter()
        .filter(|e| e.status == ExperimentStatus::HasResults)
        .map(|e| e.id.as_str())
        .collect();
    let done: Vec<&str> = ledger
        .experiments
        .iter()
        .filter(|e| {
            e.status == ExperimentStatus::Done && e.source_note_path.is_some()
        })
        .map(|e| e.id.as_str())
        .collect();
    let open_hypotheses = ledger
        .hypotheses
        .iter()
        .filter(|h| {
            matches!(h.status, HypothesisStatus::Open | HypothesisStatus::UnderTest)
        })
        .count();

    let details = SignalDetails {
        source_count,
        pending_proposals: pending.len(),
        experiments_has_results: has_results.len(),
        experiments_done: done.len(),
        open_hypotheses,
    };

    let boundary_plausible = match phase {
        Phase::Init => source_count >= INIT_MIN_SOURCES,
        Phase::Discuss => !pending.is_empty(),
        Phase::Experiment => !has_results.is_empty(),
        Phase::Post => !done.is_empty(),
    };

    // The hash captures exactly the gate-relevant identifiers/counts for this
    // phase, so it changes only when a material new trace appears.
    let basis = match phase {
        Phase::Init => format!("INIT:src={source_count}"),
        Phase::Discuss => format!("DISCUSS:pending={}", pending.join(",")),
        Phase::Experiment => format!("EXPERIMENT:has_results={}", has_results.join(",")),
        Phase::Post => format!("POST:done={}", done.join(",")),
    };

    ExitSignal {
        boundary_plausible,
        signal_hash: hash_str(&basis),
        details,
    }
}

/// Whether the driver should surface a checkpoint now: the gate must be met,
/// and we suppress re-asking while the signal is unchanged since the user last
/// chose STAY (hysteresis).
pub fn should_checkpoint(phase: Phase, signal: &ExitSignal, ledger: &Ledger) -> bool {
    if !signal.boundary_plausible {
        return false;
    }
    match ledger.phases.get(phase_key(phase)) {
        Some(state)
            if state.last_signal_hash.as_deref() == Some(signal.signal_hash.as_str())
                && state.last_decision == Some(DecisionAction::Stay) =>
        {
            false
        }
        _ => true,
    }
}

/// The next phase given a decision. `None` means STOP (terminal). `BRANCH`
/// requires `target`; without one it is treated as STAY.
pub fn next_phase(current: Phase, action: DecisionAction, target: Option<Phase>) -> Option<Phase> {
    match action {
        DecisionAction::Stop => None,
        DecisionAction::Stay => Some(current),
        DecisionAction::Branch => Some(target.unwrap_or(current)),
        DecisionAction::Advance => Some(match current {
            Phase::Init => Phase::Discuss,
            Phase::Discuss => Phase::Experiment,
            Phase::Experiment => Phase::Post,
            Phase::Post => Phase::Discuss, // loop closes back to the hub
        }),
    }
}

/// Low-stakes transitions the LLM may self-route via `phase_route`; everything
/// else must escalate to the human via `checkpoint_ask`. High-stakes =
/// starting an experiment, committing to the wiki (POST), stopping, or dropping
/// an experiment. This is enforced by withholding the phase_route tool.
pub fn is_low_stakes(phase: Phase, action: DecisionAction) -> bool {
    match action {
        DecisionAction::Stay => true,
        DecisionAction::Advance => phase == Phase::Init, // INIT -> DISCUSS only
        DecisionAction::Branch | DecisionAction::Stop => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{PhaseState, Proposal};

    fn ledger_with_proposal(rough: Option<&str>) -> Ledger {
        let mut l = Ledger::new("s");
        l.current_phase = Phase::Discuss;
        l.proposals.push(Proposal {
            id: "prop-1".into(),
            hypothesis_id: "hyp-1".into(),
            rationale: None,
            rough_design: rough.map(|s| s.to_string()),
            evidence_refs: vec![],
            status: ProposalStatus::Proposed,
            created_at: None,
            decided_at: None,
        });
        l
    }

    #[test]
    fn discuss_gate_needs_a_proposed_with_design() {
        let root = std::env::temp_dir();
        let with = ledger_with_proposal(Some("a design"));
        assert!(compute_exit_signal(Phase::Discuss, &with, &root).boundary_plausible);
        let without = ledger_with_proposal(None);
        assert!(!compute_exit_signal(Phase::Discuss, &without, &root).boundary_plausible);
    }

    #[test]
    fn hysteresis_suppresses_until_signal_changes() {
        let root = std::env::temp_dir();
        let mut l = ledger_with_proposal(Some("d"));
        let sig = compute_exit_signal(Phase::Discuss, &l, &root);
        // First time: should ask.
        assert!(should_checkpoint(Phase::Discuss, &sig, &l));
        // User chose STAY at this signal -> suppress while unchanged.
        l.phases.insert(
            phase_key(Phase::Discuss).to_string(),
            PhaseState {
                last_signal_hash: Some(sig.signal_hash.clone()),
                last_decision: Some(DecisionAction::Stay),
                last_checkpoint_at: None,
                visits: 1,
            },
        );
        assert!(!should_checkpoint(Phase::Discuss, &sig, &l));
        // A new proposal changes the signal -> ask again.
        l.proposals.push(Proposal {
            id: "prop-2".into(),
            hypothesis_id: "hyp-2".into(),
            rationale: None,
            rough_design: Some("d2".into()),
            evidence_refs: vec![],
            status: ProposalStatus::Proposed,
            created_at: None,
            decided_at: None,
        });
        let sig2 = compute_exit_signal(Phase::Discuss, &l, &root);
        assert_ne!(sig.signal_hash, sig2.signal_hash);
        assert!(should_checkpoint(Phase::Discuss, &sig2, &l));
    }

    #[test]
    fn routing_and_stakes() {
        assert_eq!(
            next_phase(Phase::Init, DecisionAction::Advance, None),
            Some(Phase::Discuss)
        );
        assert_eq!(
            next_phase(Phase::Discuss, DecisionAction::Branch, Some(Phase::Experiment)),
            Some(Phase::Experiment)
        );
        assert_eq!(next_phase(Phase::Post, DecisionAction::Advance, None), Some(Phase::Discuss));
        assert_eq!(next_phase(Phase::Discuss, DecisionAction::Stop, None), None);

        assert!(is_low_stakes(Phase::Init, DecisionAction::Advance));
        assert!(is_low_stakes(Phase::Discuss, DecisionAction::Stay));
        assert!(!is_low_stakes(Phase::Discuss, DecisionAction::Branch)); // start experiment
        assert!(!is_low_stakes(Phase::Post, DecisionAction::Advance)); // wiki commit boundary
    }
}
