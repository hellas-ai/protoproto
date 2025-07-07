use serde::{Deserialize, Serialize};

use crate::*;

/// Constants for timeout durations (in units of delta)
pub const COMPLAIN_TIMEOUT_FACTOR: u128 = 6;
pub const END_VIEW_TIMEOUT_FACTOR: u128 = 12;

/// Manages timeout tracking and complaint logic
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeoutManager {
    /// Current logical time
    pub current_time: u128,
    
    /// Network delay parameter (Δ in pseudocode)
    pub delta: u128,
}

impl TimeoutManager {
    pub fn new(delta: u128) -> Self {
        Self {
            current_time: 0,
            delta,
        }
    }
    
    /// Update the current time
    pub fn set_time(&mut self, time: u128) {
        self.current_time = time;
    }
    
    /// Check if complaint timeout has been reached for a given view entry time
    pub fn should_complain(&self, view_entry_time: u128) -> bool {
        let time_in_view = self.current_time.saturating_sub(view_entry_time);
        time_in_view >= self.delta * COMPLAIN_TIMEOUT_FACTOR
    }
    
    /// Check if end-view timeout has been reached for a given view entry time
    pub fn should_end_view(&self, view_entry_time: u128) -> bool {
        let time_in_view = self.current_time.saturating_sub(view_entry_time);
        time_in_view >= self.delta * END_VIEW_TIMEOUT_FACTOR
    }
    
    /// Calculate the complaint timeout threshold
    pub fn complaint_timeout(&self) -> u128 {
        self.delta * COMPLAIN_TIMEOUT_FACTOR
    }
    
    /// Calculate the end-view timeout threshold
    pub fn end_view_timeout(&self) -> u128 {
        self.delta * END_VIEW_TIMEOUT_FACTOR
    }
    
    /// Get the time elapsed since a given timestamp
    pub fn time_since(&self, timestamp: u128) -> u128 {
        self.current_time.saturating_sub(timestamp)
    }
} 