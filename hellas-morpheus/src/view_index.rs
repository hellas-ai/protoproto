use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::*;

/// Manages view-specific tracking and lookups
///
/// This component is responsible for:
/// - Tracking which views contain leader blocks
/// - Managing unfinalized leader blocks by view
/// - Supporting efficient view-based queries
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ViewIndex {
    /// Tracks whether we've seen a leader block for each view
    /// Used to implement logic that depends on leader blocks within a view
    pub contains_lead_by_view: BTreeMap<ViewNum, bool>,

    /// Maps views to sets of unfinalized leader blocks
    /// Tracks which leader blocks are not yet finalized by view
    pub unfinalized_lead_by_view: BTreeMap<ViewNum, BTreeSet<BlockKey>>,
}

impl ViewIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a new leader block for a view
    pub fn insert_leader_block(&mut self, block_key: &BlockKey) {
        if block_key.type_ != BlockType::Lead {
            return;
        }

        self.contains_lead_by_view.insert(block_key.view, true);
        self.unfinalized_lead_by_view
            .entry(block_key.view)
            .or_default()
            .insert(block_key.clone());
    }

    /// Marks a leader block as finalized
    pub fn finalize_leader_block(&mut self, block_key: &BlockKey) {
        if block_key.type_ != BlockType::Lead {
            return;
        }

        self.unfinalized_lead_by_view
            .entry(block_key.view)
            .or_default()
            .remove(block_key);
    }

    /// Checks if a view contains any leader blocks
    pub fn has_leader_in_view(&self, view: ViewNum) -> bool {
        self.contains_lead_by_view
            .get(&view)
            .copied()
            .unwrap_or(false)
    }

    /// Checks if a view has any unfinalized leader blocks
    pub fn has_unfinalized_leaders(&self, view: ViewNum) -> bool {
        self.unfinalized_lead_by_view
            .get(&view)
            .map_or(false, |set| !set.is_empty())
    }

    /// Gets all unfinalized leader blocks for a view
    pub fn get_unfinalized_leaders(&self, view: ViewNum) -> Vec<BlockKey> {
        self.unfinalized_lead_by_view
            .get(&view)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }
} 