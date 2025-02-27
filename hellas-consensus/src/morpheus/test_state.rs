use std::time::Duration;
use muchin::automaton::ModelState;
use muchin_model_state_derive::ModelState;

use crate::morpheus::state::MorpheusState;

/// Test model state used for testing
#[derive(ModelState, Debug)]
pub struct TestMorpheusState {
    pub morpheus: MorpheusState,
}

impl TestMorpheusState {
    pub fn new(
        process_id: crate::morpheus::types::ProcessId,
        num_processes: usize,
        f: usize,
        delta: Duration,
    ) -> Self {
        Self {
            morpheus: MorpheusState::new(process_id, num_processes, f, delta),
        }
    }
}