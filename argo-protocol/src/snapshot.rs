//! A subscriber's whole view of one revision, and the live deltas that change it.
//!
//! The GUI initializes from the applied subscription cache, then applies deltas (§4). Structure
//! never changes through an [`Update`]: a structural change is a new revision.

use serde::{Deserialize, Serialize};

use crate::{
    Annotation, Attempt, EdgeStatus, Graph, Implementation, Job, Language, NodeId, NodeStatus,
    SourceFile, ValidationReport, Worker,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub graph: Graph,
    pub sources: Vec<SourceFile>,
    pub status: Vec<NodeStatus>,
    pub edge_status: Vec<EdgeStatus>,
    pub workers: Vec<Worker>,
    pub jobs: Vec<Job>,
    pub attempts: Vec<Attempt>,
    pub implementations: Vec<Implementation>,
    pub reports: Vec<ValidationReport>,
    pub annotations: Vec<Annotation>,
}

/// One live delta. Applying is an upsert keyed by the record's identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "update", content = "record")]
pub enum Update {
    Status(NodeStatus),
    EdgeStatus(EdgeStatus),
    Worker(Worker),
    Job(Job),
    Attempt(Attempt),
    Implementation(Implementation),
    Report(ValidationReport),
    /// Annotations accumulate; they are never replaced.
    Annotation(Annotation),
}

fn upsert<T>(rows: &mut Vec<T>, row: T, same: impl Fn(&T, &T) -> bool) {
    match rows.iter_mut().find(|r| same(r, &row)) {
        Some(slot) => *slot = row,
        None => rows.push(row),
    }
}

impl Snapshot {
    /// A fresh snapshot: every node starts as the original C (or no language for structure).
    pub fn new(graph: Graph, sources: Vec<SourceFile>) -> Self {
        let status = graph
            .nodes
            .iter()
            .map(|n| {
                let structural = n.kind.is_synthesized()
                    || matches!(
                        n.kind,
                        crate::NodeKind::Program
                            | crate::NodeKind::Module
                            | crate::NodeKind::Type(_)
                    );
                NodeStatus::new(
                    n.id,
                    if structural {
                        Language::None
                    } else {
                        Language::C
                    },
                )
            })
            .collect();
        Self {
            graph,
            sources,
            status,
            edge_status: Vec::new(),
            workers: Vec::new(),
            jobs: Vec::new(),
            attempts: Vec::new(),
            implementations: Vec::new(),
            reports: Vec::new(),
            annotations: Vec::new(),
        }
    }

    pub fn apply(&mut self, update: Update) {
        match update {
            Update::Status(s) => upsert(&mut self.status, s, |a, b| a.node == b.node),
            Update::EdgeStatus(s) => upsert(&mut self.edge_status, s, |a, b| a.edge == b.edge),
            Update::Worker(w) => upsert(&mut self.workers, w, |a, b| a.id == b.id),
            Update::Job(j) => upsert(&mut self.jobs, j, |a, b| a.id == b.id),
            Update::Attempt(a) => upsert(&mut self.attempts, a, |x, y| x.id == y.id),
            Update::Implementation(i) => upsert(&mut self.implementations, i, |a, b| a.id == b.id),
            Update::Report(r) => upsert(&mut self.reports, r, |a, b| a.id == b.id),
            Update::Annotation(a) => self.annotations.push(a),
        }
    }

    pub fn status(&self, node: NodeId) -> Option<&NodeStatus> {
        self.status.iter().find(|s| s.node == node)
    }

    pub fn report(&self, id: crate::ReportId) -> Option<&ValidationReport> {
        self.reports.iter().find(|r| r.id == id)
    }

    pub fn annotations(&self, node: NodeId) -> impl Iterator<Item = &Annotation> {
        self.annotations.iter().filter(move |a| a.node == node)
    }

    pub fn source(&self, file: crate::FileId) -> Option<&SourceFile> {
        self.sources.iter().find(|s| s.id == file)
    }
}
