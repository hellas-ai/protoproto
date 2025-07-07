use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use crate::*;

/// Manages view-related state and transitions
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewManager {
    /// Current view number
    pub current_view: ViewNum,
    
    /// Time when this process entered the current view
    pub view_entry_time: u128,
    
    /// Tracks the phase within each view
    pub phase_by_view: BTreeMap<ViewNum, Phase>,
    
    /// Tracks which QCs we've already complained about to the leader
    pub complained_qcs: BTreeSet<FinishedQC>,
    
    /// Tracks start view messages received for each view
    pub start_views: BTreeMap<ViewNum, Vec<Arc<Signed<StartView>>>>,
    
    /// Network delay parameter (Δ in pseudocode)
    pub delta: u128,
    
    /// Total number of processes
    pub n: u32,
}

impl ViewManager {
    pub fn new(n: u32, delta: u128) -> Self {
        let mut phase_by_view = BTreeMap::new();
        phase_by_view.insert(ViewNum(0), Phase::High);
        
        Self {
            current_view: ViewNum(0),
            view_entry_time: 0,
            phase_by_view,
            complained_qcs: BTreeSet::new(),
            start_views: BTreeMap::new(),
            delta,
            n,
        }
    }
    
    /// Get the current view
    pub fn current_view(&self) -> ViewNum {
        self.current_view
    }
    
    /// Get the current phase for a view
    pub fn phase(&self, view: ViewNum) -> Phase {
        self.phase_by_view.get(&view).copied().unwrap_or(Phase::High)
    }
    
    /// Set the phase for the current view
    pub fn set_phase(&mut self, phase: Phase) {
        self.phase_by_view.insert(self.current_view, phase);
    }
    
    /// Calculate the leader for a given view
    pub fn leader(&self, view: ViewNum) -> Identity {
        Identity((view.0 as u32 % self.n) + 1)
    }
    
    /// Check if the given identity is the leader for the view
    pub fn is_leader(&self, id: Identity, view: ViewNum) -> bool {
        self.leader(view) == id
    }
    
    /// Enter a new view
    pub fn enter_view(&mut self, new_view: ViewNum, current_time: u128) {
        self.current_view = new_view;
        self.view_entry_time = current_time;
        self.phase_by_view.insert(new_view, Phase::High);
    }
    
    /// Add a start view message
    pub fn add_start_view(&mut self, msg: Arc<Signed<StartView>>) {
        self.start_views
            .entry(msg.data.view)
            .or_insert_with(Vec::new)
            .push(msg);
    }
    
    /// Get start view messages for a view
    pub fn get_start_views(&self, view: ViewNum) -> Option<&Vec<Arc<Signed<StartView>>>> {
        self.start_views.get(&view)
    }
    
    /// Check if we have enough start view messages
    pub fn has_enough_start_views(&self, view: ViewNum, f: u32) -> bool {
        self.start_views
            .get(&view)
            .map(|msgs| msgs.len() >= (self.n - f) as usize)
            .unwrap_or(false)
    }
    
    /// Mark that we've complained about a QC
    pub fn mark_complained(&mut self, qc: FinishedQC) -> bool {
        self.complained_qcs.insert(qc)
    }
    
    /// Calculate time in current view
    pub fn time_in_view(&self, current_time: u128) -> u128 {
        current_time.saturating_sub(self.view_entry_time)
    }
}

 