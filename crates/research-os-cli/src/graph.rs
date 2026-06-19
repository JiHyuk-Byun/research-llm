//! Derived relationship graph for the wiki (`wiki/graph.json`).
//!
//! Markdown + ledger are the source of truth; this is a regenerable cache built
//! deterministically (no NLP) from structured traces only: wiki source-note
//! frontmatter (`source_id`, `source_type`, `title`, `relations:`) plus the
//! ledger (hypotheses/experiments and their `evidence_refs`/`hypothesis_id`).
//! The fuzzy judgment that produced a `relations` entry happened upstream (an
//! agent wrote it); the builder only reads it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ledger::Ledger;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String, // source | hypothesis | experiment
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>, // paper | experiment (for source nodes)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default)]
    pub degree: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub kind: String, // cites|supports|contradicts|refines|extends|duplicates|derives_from|tests
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub provenance: String, // frontmatter | ledger
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Graph {
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

pub fn graph_path(session_root: &Path) -> PathBuf {
    session_root.join("wiki").join("graph.json")
}

impl Graph {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, self.to_json())?;
        fs::rename(&tmp, path)
    }

    pub fn neighbors(&self, id: &str) -> Vec<&Edge> {
        self.edges.iter().filter(|e| e.from == id || e.to == id).collect()
    }

    /// Unresolved contradiction edges — a key gap signal.
    pub fn contradictions(&self) -> Vec<&Edge> {
        self.edges.iter().filter(|e| e.kind == "contradicts").collect()
    }

    /// Nodes with no incident edge (cited by nothing / citing nothing).
    pub fn orphans(&self) -> Vec<&Node> {
        self.nodes
            .iter()
            .filter(|n| !self.edges.iter().any(|e| e.from == n.id || e.to == n.id))
            .collect()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
    }
}

/// A compact markdown digest of the graph slice most relevant to the current
/// phase — the working-context pack injected into a stage prompt. Seeds from
/// the active experiment/hypothesis (EXPERIMENT/POST), unresolved
/// contradictions, and the highest-degree hubs; expands one hop; caps at
/// `max_nodes`. Returns "" when the graph is empty (e.g. early INIT).
pub fn context_pack(
    session_root: &Path,
    ledger: &Ledger,
    phase: crate::ledger::Phase,
    max_nodes: usize,
) -> String {
    use crate::ledger::Phase;
    let g = build_graph(session_root, ledger);
    if g.nodes.is_empty() {
        return String::new();
    }

    // Seeds.
    let mut seeds: Vec<String> = Vec::new();
    if matches!(phase, Phase::Experiment | Phase::Post) {
        if let Some(e) = ledger.experiments.last() {
            seeds.push(e.id.clone());
            if !e.hypothesis_id.is_empty() {
                seeds.push(e.hypothesis_id.clone());
            }
        }
    }
    for e in g.contradictions() {
        seeds.push(e.from.clone());
        seeds.push(e.to.clone());
    }
    let mut by_degree: Vec<&Node> = g.nodes.iter().collect();
    by_degree.sort_by(|a, b| b.degree.cmp(&a.degree).then(a.id.cmp(&b.id)));
    for n in by_degree.iter().take(5) {
        seeds.push(n.id.clone());
    }

    // Expand one hop, dedup, cap.
    let mut selected: Vec<String> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for s in &seeds {
        if seen.insert(s.clone()) {
            selected.push(s.clone());
        }
        for e in g.neighbors(s) {
            for nb in [&e.from, &e.to] {
                if nb != s && seen.insert(nb.clone()) {
                    selected.push(nb.clone());
                }
            }
        }
        if selected.len() >= max_nodes {
            break;
        }
    }
    selected.truncate(max_nodes);
    if selected.is_empty() {
        return String::new();
    }

    let node_by_id: BTreeMap<&str, &Node> =
        g.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut out = String::from("## Relevant knowledge (curated from the wiki graph)\n");
    for id in &selected {
        match node_by_id.get(id.as_str()) {
            Some(n) => {
                let title = n.title.as_deref().unwrap_or(id);
                let path = n
                    .path
                    .as_deref()
                    .map(|p| format!(" ({p})"))
                    .unwrap_or_default();
                out.push_str(&format!("- [{}] {}{}\n", n.kind, truncate(title, 80), path));
                for e in g.edges.iter().filter(|e| &e.from == id) {
                    out.push_str(&format!("    ↳ {} → {}\n", e.kind, e.to));
                }
            }
            None => out.push_str(&format!("- [ref] {id}\n")),
        }
    }
    out.push_str("(Read a node's path for full detail, or call graph_query for more.)\n");
    out
}

/// Build the graph from a session's wiki source notes + ledger.
pub fn build_graph(session_root: &Path, ledger: &Ledger) -> Graph {
    let mut nodes: BTreeMap<String, Node> = BTreeMap::new();
    let mut edges: Vec<Edge> = Vec::new();

    // 1. wiki/sources/*.md — one node per source note + its frontmatter relations.
    let sources_dir = session_root.join("wiki").join("sources");
    if let Ok(rd) = fs::read_dir(&sources_dir) {
        let mut entries: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
            .collect();
        entries.sort(); // deterministic order
        for path in entries {
            let text = fs::read_to_string(&path).unwrap_or_default();
            let fm = frontmatter_block(&text).unwrap_or("");
            let file_stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let id = scalar(fm, "source_id").unwrap_or(file_stem.clone());
            let subtype = scalar(fm, "source_type");
            let title = scalar(fm, "title");
            let rel_path = format!(
                "wiki/sources/{}",
                path.file_name().and_then(|s| s.to_str()).unwrap_or("")
            );
            let kind = if subtype.as_deref() == Some("experiment") {
                "experiment"
            } else {
                "source"
            };
            nodes.entry(id.clone()).or_insert(Node {
                id: id.clone(),
                kind: kind.to_string(),
                subtype,
                title,
                path: Some(rel_path),
                degree: 0,
            });
            for (rtype, target, note) in parse_relations(fm) {
                edges.push(Edge {
                    from: id.clone(),
                    to: normalize_ref(&target),
                    kind: rtype,
                    note,
                    provenance: "frontmatter".to_string(),
                });
            }
        }
    }

    // 2. ledger — hypothesis/experiment nodes + derives_from/tests edges.
    for h in &ledger.hypotheses {
        nodes.entry(h.id.clone()).or_insert(Node {
            id: h.id.clone(),
            kind: "hypothesis".to_string(),
            subtype: None,
            title: Some(h.statement.clone()),
            path: None,
            degree: 0,
        });
        for ev in &h.evidence_refs {
            edges.push(Edge {
                from: h.id.clone(),
                to: normalize_ref(ev),
                kind: "derives_from".to_string(),
                note: None,
                provenance: "ledger".to_string(),
            });
        }
    }
    for e in &ledger.experiments {
        nodes.entry(e.id.clone()).or_insert(Node {
            id: e.id.clone(),
            kind: "experiment".to_string(),
            subtype: None,
            title: None,
            path: e.source_note_path.clone(),
            degree: 0,
        });
        if !e.hypothesis_id.is_empty() {
            edges.push(Edge {
                from: e.id.clone(),
                to: e.hypothesis_id.clone(),
                kind: "tests".to_string(),
                note: None,
                provenance: "ledger".to_string(),
            });
        }
    }

    let mut graph = Graph {
        nodes: nodes.into_values().collect(),
        edges,
    };
    compute_degrees(&mut graph);
    graph
}

fn compute_degrees(graph: &mut Graph) {
    let mut deg: BTreeMap<String, usize> = BTreeMap::new();
    for e in &graph.edges {
        *deg.entry(e.from.clone()).or_default() += 1;
        *deg.entry(e.to.clone()).or_default() += 1;
    }
    for n in &mut graph.nodes {
        n.degree = deg.get(&n.id).copied().unwrap_or(0);
    }
}

/// Strip a leading `src:`/`hyp:`/`exp:` namespace from a reference so edges
/// resolve to bare node ids.
fn normalize_ref(s: &str) -> String {
    let s = s.trim();
    for p in ["src:", "hyp:", "exp:"] {
        if let Some(rest) = s.strip_prefix(p) {
            return rest.to_string();
        }
    }
    s.to_string()
}

/// The YAML frontmatter block between the leading `---` and the next `---`.
fn frontmatter_block(text: &str) -> Option<&str> {
    let t = text.trim_start();
    let rest = t.strip_prefix("---")?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

/// A top-level (column-0) `key: value` scalar from the frontmatter.
fn scalar(fm: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    for line in fm.lines() {
        if line.starts_with(&prefix) {
            return Some(line[prefix.len()..].trim().trim_matches('"').to_string());
        }
    }
    None
}

/// Parse a `relations:` block of inline entries:
/// `  - { type: contradicts, target: src:foo, note: ... }`.
fn parse_relations(fm: &str) -> Vec<(String, String, Option<String>)> {
    let mut out = Vec::new();
    let mut in_rel = false;
    for line in fm.lines() {
        if line.starts_with("relations:") {
            in_rel = true;
            continue;
        }
        if !in_rel {
            continue;
        }
        let trimmed = line.trim_start();
        if !trimmed.starts_with('-') {
            // A new column-0 key ends the relations block.
            if !line.starts_with(char::is_whitespace) {
                break;
            }
            continue;
        }
        let inner = trimmed
            .trim_start_matches('-')
            .trim()
            .trim_start_matches('{')
            .trim_end_matches('}');
        let (mut rtype, mut target, mut note) = (None, None, None);
        for kv in inner.split(',') {
            let mut it = kv.splitn(2, ':');
            let k = it.next().unwrap_or("").trim();
            let v = it.next().unwrap_or("").trim().trim_matches('"');
            match k {
                "type" => rtype = Some(v.to_string()),
                "target" => target = Some(v.to_string()),
                "note" if !v.is_empty() => note = Some(v.to_string()),
                _ => {}
            }
        }
        if let (Some(rt), Some(tg)) = (rtype, target) {
            out.push((rt, tg, note));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{Experiment, ExperimentStatus, Hypothesis, HypothesisStatus};

    fn write(p: &Path, body: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    #[test]
    fn builds_nodes_and_edges_from_notes_and_ledger() {
        let dir =
            std::env::temp_dir().join(format!("ros-graph-{}", crate::ledger::now_ms()));
        let sroot = dir.clone();
        // A paper note and an experiment note that contradicts it.
        write(
            &sroot.join("wiki/sources/paper-a.md"),
            "---\nsource_id: paper-a\nsource_type: paper\ntitle: \"Paper A\"\n---\n# A\n",
        );
        write(
            &sroot.join("wiki/sources/exp-1.md"),
            "---\nsource_id: exp-1\nsource_type: experiment\ntitle: \"Exp 1\"\nrelations:\n  - { type: contradicts, target: src:paper-a }\n---\n# Exp\n",
        );
        let mut led = Ledger::new("s");
        led.hypotheses.push(Hypothesis {
            id: "hyp-1".into(),
            statement: "h".into(),
            evidence_refs: vec!["src:paper-a".into()],
            status: HypothesisStatus::UnderTest,
            created_at: None,
            updated_at: None,
        });
        led.experiments.push(Experiment {
            id: "exp-1".into(),
            proposal_id: None,
            hypothesis_id: "hyp-1".into(),
            turn_id: None,
            record_dir: None,
            status: ExperimentStatus::Done,
            outcome: None,
            post_path: None,
            source_note_path: Some("wiki/sources/exp-1.md".into()),
            created_at: None,
            updated_at: None,
        });

        let g = build_graph(&sroot, &led);
        // nodes: paper-a, exp-1 (merged from note+ledger), hyp-1
        assert!(g.nodes.iter().any(|n| n.id == "paper-a" && n.kind == "source"));
        assert!(g.nodes.iter().any(|n| n.id == "exp-1" && n.kind == "experiment"));
        assert!(g.nodes.iter().any(|n| n.id == "hyp-1" && n.kind == "hypothesis"));
        // edges: exp-1 contradicts paper-a; hyp-1 derives_from paper-a; exp-1 tests hyp-1
        assert!(g.edges.iter().any(|e| e.from == "exp-1" && e.to == "paper-a" && e.kind == "contradicts"));
        assert!(g.edges.iter().any(|e| e.from == "hyp-1" && e.to == "paper-a" && e.kind == "derives_from"));
        assert!(g.edges.iter().any(|e| e.from == "exp-1" && e.to == "hyp-1" && e.kind == "tests"));
        // queries
        assert_eq!(g.contradictions().len(), 1);
        assert!(g.nodes.iter().find(|n| n.id == "paper-a").unwrap().degree >= 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn frontmatter_and_relations_parse() {
        let fm = "source_id: x\nsource_type: paper\nrelations:\n  - { type: refines, target: src:y, note: a b }\n  - { type: supports, target: z }";
        assert_eq!(scalar(fm, "source_id").as_deref(), Some("x"));
        let rel = parse_relations(fm);
        assert_eq!(rel.len(), 2);
        assert_eq!(rel[0].0, "refines");
        assert_eq!(rel[0].1, "src:y");
        assert_eq!(rel[1], ("supports".to_string(), "z".to_string(), None));
    }

    #[test]
    fn context_pack_surfaces_relevant_slice() {
        let dir = std::env::temp_dir().join(format!("ros-ctx-{}", crate::ledger::now_ms()));
        write(
            &dir.join("wiki/sources/paper-a.md"),
            "---\nsource_id: paper-a\nsource_type: paper\ntitle: \"Paper A\"\n---\n",
        );
        write(
            &dir.join("wiki/sources/exp-1.md"),
            "---\nsource_id: exp-1\nsource_type: experiment\ntitle: \"Exp 1\"\nrelations:\n  - { type: contradicts, target: src:paper-a }\n---\n",
        );
        let pack = context_pack(&dir, &Ledger::new("s"), crate::ledger::Phase::Discuss, 12);
        assert!(pack.contains("Relevant knowledge"));
        assert!(pack.contains("contradicts"));
        let _ = fs::remove_dir_all(&dir);

        // Empty graph -> empty pack (e.g. early INIT).
        let empty = std::env::temp_dir().join(format!("ros-ctx-e-{}", crate::ledger::now_ms()));
        assert!(context_pack(&empty, &Ledger::new("s"), crate::ledger::Phase::Init, 12).is_empty());
    }
}
