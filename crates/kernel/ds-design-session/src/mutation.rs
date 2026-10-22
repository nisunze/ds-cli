//! Session mutation log.
//!
//! Diffs are feature-state transitions, not UI commands. That keeps undo,
//! redo, split, vertex edits, and table-backed property edits on one path:
//! apply each change's `after` state for forward, or `before` for undo.

use crate::session::{Feature, FeatureId};
use serde::{Deserialize, Serialize};

pub const DEFAULT_MUTATION_CAPACITY: usize = 200;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeatureChange {
    pub feature_id: FeatureId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Feature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Feature>,
}

impl FeatureChange {
    pub fn reversed(&self) -> Self {
        Self {
            feature_id: self.feature_id.clone(),
            before: self.after.clone(),
            after: self.before.clone(),
        }
    }

    pub fn is_noop(&self) -> bool {
        self.before == self.after
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionDiff {
    pub kind: String,
    pub changes: Vec<FeatureChange>,
}

impl SessionDiff {
    pub fn single(kind: impl Into<String>, change: FeatureChange) -> Self {
        Self {
            kind: kind.into(),
            changes: vec![change],
        }
    }

    pub fn is_noop(&self) -> bool {
        self.changes.is_empty() || self.changes.iter().all(FeatureChange::is_noop)
    }

    pub fn reversed(&self, kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            changes: self
                .changes
                .iter()
                .rev()
                .map(FeatureChange::reversed)
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct MutationLog {
    undo: Vec<SessionDiff>,
    redo: Vec<SessionDiff>,
    capacity: usize,
}

impl Default for MutationLog {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_MUTATION_CAPACITY)
    }
}

impl MutationLog {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            capacity: capacity.max(1),
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    pub fn record(&mut self, diff: SessionDiff) {
        if diff.is_noop() {
            return;
        }
        if self.undo.len() == self.capacity {
            self.undo.remove(0);
        }
        self.undo.push(diff);
        self.redo.clear();
    }

    pub fn pop_undo(&mut self) -> Option<SessionDiff> {
        self.undo.pop()
    }

    pub fn pop_redo(&mut self) -> Option<SessionDiff> {
        self.redo.pop()
    }

    pub fn push_undo(&mut self, diff: SessionDiff) {
        if self.undo.len() == self.capacity {
            self.undo.remove(0);
        }
        self.undo.push(diff);
    }

    pub fn push_redo(&mut self, diff: SessionDiff) {
        if self.redo.len() == self.capacity {
            self.redo.remove(0);
        }
        self.redo.push(diff);
    }
}
