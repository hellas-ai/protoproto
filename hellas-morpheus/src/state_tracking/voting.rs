use std::sync::Arc;
use crate::*;

impl MorpheusProcess {
    /// Attempt to send a z-vote for a block if not already voted
    pub fn try_vote(
        &mut self,
        z: u8,
        block: &BlockKey,
        target: Option<Identity>,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) -> bool {
        tracing::debug!(target: "try_vote", z = z, block = ?block, target = ?target);
        let author = block.author.clone().expect("not voting for genesis block");
        if !self.voted_i.contains(&(z, block.type_, block.slot, author.clone())) {
            self.voted_i.insert((z, block.type_, block.slot, author.clone()));
            let voted = Arc::new(ThreshPartial::from_data(
                VoteData { z, for_which: block.clone() },
                &self.kb,
            ));
            self.send_msg(to_send, (Message::NewVote(voted.clone()), target));
            true
        } else {
            false
        }
    }

    /// Record a received vote and form a QC if threshold reached
    pub fn record_vote(
        &mut self,
        vote_data: &Arc<ThreshPartial<VoteData>>,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) -> bool {
        tracing::debug!(target: "record_vote", vote_data = ?vote_data.data);
        match self.vote_tracker.record_vote(vote_data.clone()) {
            Ok(num_votes) => {
                if num_votes == self.n - self.f {
                    // aggregate and form QC
                    let votes_now = self.vote_tracker.votes[&vote_data.data]
                        .values()
                        .map(|v| (v.author.0 as usize - 1, v.signature.clone()))
                        .collect::<Vec<_>>();
                    let agg = self.kb.hints_setup.aggregator();
                    let mut data = Vec::new();
                    vote_data.data.serialize_compressed(&mut data).unwrap();
                    let signed = hints::sign_aggregate(&agg, hints::F::from((self.f + 1) as u64), &votes_now, &data).unwrap();
                    let quorum_formed = Arc::new(ThreshSigned { data: vote_data.data.clone(), signature: signed });
                    // broadcast 0-QC for own blocks
                    if vote_data.data.z == 0
                        && vote_data.data.for_which.author.as_ref() == Some(&self.id)
                        && !self.zero_qcs_sent.contains(&vote_data.data.for_which)
                    {
                        self.zero_qcs_sent.insert(vote_data.data.for_which.clone());
                        crate::tracing_setup::qc_formed(&self.id, vote_data.data.z, &vote_data.data);
                        self.send_msg(to_send, (Message::QC(quorum_formed.clone()), None));
                    }
                    self.record_qc(quorum_formed);
                }
                true
            }
            Err(Duplicate) => false,
        }
    }
}