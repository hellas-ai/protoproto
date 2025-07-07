use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Handles block production state and logic
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockProducer<Tr: Transaction> {
    /// Current slot for leader blocks
    pub slot_lead: SlotNum,

    /// Current slot for transaction blocks  
    pub slot_tr: SlotNum,

    /// Ready transactions to include in next block
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    #[serde(with = "ark_serialize::vec_compressed_checked")]
    pub ready_transactions: Vec<Tr>,

    /// Tracks whether we've produced a leader block in each view
    pub produced_lead_in_view: BTreeMap<ViewNum, bool>,
}

impl<Tr: Transaction> Default for BlockProducer<Tr> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Tr: Transaction> BlockProducer<Tr> {
    pub fn new() -> Self {
        let mut produced_lead_in_view = BTreeMap::new();
        produced_lead_in_view.insert(ViewNum(0), false);

        Self {
            slot_lead: SlotNum(0),
            slot_tr: SlotNum(0),
            ready_transactions: Vec::new(),
            produced_lead_in_view,
        }
    }

    /// Set ready transactions
    pub fn set_ready_transactions(&mut self, transactions: Vec<Tr>) {
        self.ready_transactions = transactions;
    }

    /// Check if we have transactions ready
    pub fn has_ready_transactions(&self) -> bool {
        !self.ready_transactions.is_empty()
    }

    /// Take ready transactions (consuming them)
    pub fn take_ready_transactions(&mut self) -> Vec<Tr> {
        std::mem::take(&mut self.ready_transactions)
    }

    /// Get current transaction slot
    pub fn current_tr_slot(&self) -> SlotNum {
        self.slot_tr
    }

    /// Get current leader slot
    pub fn current_lead_slot(&self) -> SlotNum {
        self.slot_lead
    }

    /// Advance transaction slot
    pub fn advance_tr_slot(&mut self) {
        self.slot_tr = SlotNum(self.slot_tr.0 + 1);
    }

    /// Advance leader slot
    pub fn advance_lead_slot(&mut self) {
        self.slot_lead = SlotNum(self.slot_lead.0 + 1);
    }

    /// Check if we've produced a leader block in the given view
    pub fn has_produced_lead_in_view(&self, view: ViewNum) -> bool {
        self.produced_lead_in_view
            .get(&view)
            .copied()
            .unwrap_or(false)
    }

    /// Mark that we've produced a leader block in the given view
    pub fn mark_lead_produced(&mut self, view: ViewNum) {
        self.produced_lead_in_view.insert(view, true);
    }
}
