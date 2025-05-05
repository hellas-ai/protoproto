//! Block production module
//!
//! Splits transaction and leader block production logic into submodules.
use crate::*;

mod transaction;
mod leader;

impl MorpheusProcess {
    /// Attempt to produce transaction and leader blocks
    pub fn try_produce_blocks(&mut self, to_send: &mut Vec<(Message, Option<Identity>)>) {
        if self.payload_ready() {
            self.make_tr_block(to_send);
        }
        if self.id == self.lead(self.view_i)
            && self.leader_ready()
            && self.phase_i.get(&self.view_i).unwrap_or(&Phase::High) == &Phase::High
            && self.index.tips.len() > 1
        {
            self.make_leader_block(to_send);
        }
    }
}