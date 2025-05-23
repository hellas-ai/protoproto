use crate::format::*;
use crate::*;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

/// Represents a violation of an internal state invariant
///
/// This has a nice-ish `Display` impl.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvariantViolation {
    // View and phase consistency
    ViewHasNoPhase(ViewNum),

    // Time consistency
    ViewEntryTimeAfterCurrentTime {
        view_entry_time: u128,
        current_time: u128,
    },

    // Block related violations
    BlockKeyMismatch {
        index_key: BlockKey,
        block_key: BlockKey,
    },
    BlockPointsToMissingFromPointedBy {
        block: BlockKey,
        pointed_to: BlockKey,
    },
    BlockPointsToMissingPointedByEntry {
        block: BlockKey,
        pointed_to: BlockKey,
    },
    BlockPointedByContainsNonExistentBlock {
        key: BlockKey,
    },
    PointingBlockDoesNotActuallyPoint {
        pointing_block: BlockKey,
        pointed_block: BlockKey,
    },
    BlockPointedByContainsNonExistentPointingBlock {
        pointed_block: BlockKey,
        pointing_block: BlockKey,
    },

    // QC related violations
    QcDataMismatch {
        index_data: VoteData,
        qc_data: VoteData,
    },
    QcIndexMismatch {
        qc_index_data: VoteData,
        qc_data: VoteData,
    },
    QcNotInQcIndex {
        vote_data: VoteData,
        qc_index: String,
    },
    // Tips related violations
    TipsMissingQCs {
        missing_tips: Vec<FinishedQC>,
    },
    TipsContainsExtraQCs {
        extra_tips: Vec<FinishedQC>,
    },

    // Finalization related violations
    BlockWithObserved2QcNotFinalized {
        block: BlockKey,
    },
    FinalizedBlockNot2QcObserved {
        block: BlockKey,
    },

    // Height related violations
    MaxHeightMismatch {
        recorded: usize,
        actual: usize,
    },
    MaxHeightKeyDoesNotExist {
        key: BlockKey,
    },

    // Max 1QC related violations
    Max1QcHasWrongZ {
        z: u8,
    },
    Found1QcGreaterThanMax1Qc {
        found: VoteData,
        max_1qc: VoteData,
    },

    // Finalization consistency
    BlockFinalizedButAlsoUnfinalized {
        block: BlockKey,
    },

    // Unfinalized 2QC consistency
    UnfinalizedQcHasWrongZ {
        vote_data: FinishedQC,
    },
    UnfinalizedQcNotInQcs {
        vote_data: FinishedQC,
    },
    BlockForUnfinalizedQcNotInUnfinalized {
        block: BlockKey,
    },

    // Leader consistency
    LeaderVerificationFailed {
        leader: Identity,
        view: ViewNum,
    },

    // Vote tracking consistency
    UntrackedVote {
        vote_data: ThreshPartial<VoteData>,
    },
    VoteCountMismatch {
        vote_data: VoteData,
        received_count: usize,
        tracked_count: usize,
    },
    MissingQCDespiteQuorum {
        vote_data: VoteData,
    },

    // Pending votes consistency
    PendingVotesBlockNotFound {
        view: ViewNum,
        block_key: BlockKey,
        vote_type: String,
    },
    PendingVotesForFinalizedBlock {
        view: ViewNum,
        block_key: BlockKey,
        vote_type: String,
    },
    PendingVotesMissingEligibleBlock {
        view: ViewNum,
        block_key: BlockKey,
        vote_type: String,
    },
    PendingVotesAlreadyVoted {
        view: ViewNum,
        block_key: BlockKey,
        vote_type: String,
    },
}

impl fmt::Display for InvariantViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ViewHasNoPhase(view) => write!(f, "Current view {} has no phase entry", view.0),

            Self::ViewEntryTimeAfterCurrentTime {
                view_entry_time,
                current_time,
            } => write!(
                f,
                "View entry time {} is after current time {}",
                view_entry_time, current_time
            ),

            Self::BlockKeyMismatch {
                index_key,
                block_key,
            } => write!(
                f,
                "Block key mismatch: index key {:?} doesn't match block key {:?}",
                format_block_key(index_key),
                format_block_key(block_key)
            ),

            Self::BlockPointsToMissingFromPointedBy { block, pointed_to } => write!(
                f,
                "Block {:?} points to {:?} but not in block_pointed_by",
                format_block_key(block),
                format_block_key(pointed_to)
            ),

            Self::BlockPointsToMissingPointedByEntry { block, pointed_to } => write!(
                f,
                "Block {:?} points to {:?} which is missing from block_pointed_by",
                format_block_key(block),
                format_block_key(pointed_to)
            ),

            Self::BlockPointedByContainsNonExistentBlock { key } => write!(
                f,
                "block_pointed_by contains key {:?} but no such block exists",
                format_block_key(key)
            ),

            Self::PointingBlockDoesNotActuallyPoint {
                pointing_block,
                pointed_block,
            } => write!(
                f,
                "Block {:?} in block_pointed_by for {:?} but block data disagrees",
                pointing_block, pointed_block
            ),

            Self::BlockPointedByContainsNonExistentPointingBlock {
                pointed_block,
                pointing_block,
            } => write!(
                f,
                "block_pointed_by for {:?} contains non-existent block {:?}",
                format_block_key(pointed_block),
                format_block_key(pointing_block)
            ),

            Self::QcDataMismatch {
                index_data,
                qc_data,
            } => write!(
                f,
                "QC data mismatch: index data {:?} doesn't match QC data {:?}",
                format_vote_data(index_data, false),
                format_vote_data(qc_data, false)
            ),

            Self::QcIndexMismatch {
                qc_index_data,
                qc_data,
            } => write!(
                f,
                "QC index mismatch: qc_index data {:?} doesn't match QC data {:?}",
                format_vote_data(qc_index_data, false),
                format_vote_data(qc_data, false)
            ),

            Self::QcNotInQcIndex {
                vote_data,
                qc_index,
            } => write!(
                f,
                "QC {:?} not found in qc_index:\n{:?}",
                vote_data, qc_index
            ),

            Self::TipsMissingQCs { missing_tips } => write!(
                f,
                "Tips is missing QCs that should be tips: {:?}",
                missing_tips
            ),

            Self::TipsContainsExtraQCs { extra_tips } => write!(
                f,
                "Tips contains QCs that should not be tips: {:?}",
                extra_tips
            ),

            Self::BlockWithObserved2QcNotFinalized { block } => write!(
                f,
                "Block {:?} with 2-QC is observed by another QC but not marked as finalized",
                format_block_key(block)
            ),

            Self::FinalizedBlockNot2QcObserved { block } => write!(
                f,
                "Block {:?} is marked as finalized but its 2-QC is not observed by any other QC",
                format_block_key(block)
            ),

            Self::MaxHeightMismatch { recorded, actual } => write!(
                f,
                "max_height ({}) does not match actual max height ({})",
                recorded, actual
            ),

            Self::MaxHeightKeyDoesNotExist { key } => write!(
                f,
                "max_height_key {:?} does not exist in blocks",
                format_block_key(key)
            ),

            Self::Max1QcHasWrongZ { z } => write!(f, "max_1qc has z = {} instead of 1", z),

            Self::Found1QcGreaterThanMax1Qc { found, max_1qc } => write!(
                f,
                "Found 1-QC {:?} that is greater than max_1qc {:?} according to compare_qc",
                format_vote_data(found, false),
                format_vote_data(max_1qc, false)
            ),

            Self::BlockFinalizedButAlsoUnfinalized { block } => write!(
                f,
                "Block {:?} is marked as finalized but is also in unfinalized",
                block
            ),

            Self::UnfinalizedQcHasWrongZ { vote_data } => write!(
                f,
                "VoteData {:?} in unfinalized_2qc has z = {} instead of 2",
                format_vote_data(&vote_data.data, false),
                vote_data.data.z
            ),

            Self::UnfinalizedQcNotInQcs { vote_data } => write!(
                f,
                "VoteData {:?} in unfinalized_2qc not found in qcs",
                format_vote_data(&vote_data.data, false)
            ),

            Self::BlockForUnfinalizedQcNotInUnfinalized { block } => write!(
                f,
                "Block {:?} for VoteData in unfinalized_2qc not found in unfinalized",
                format_block_key(block)
            ),

            Self::LeaderVerificationFailed { leader, view } => write!(
                f,
                "Current leader {} for view {} fails verification",
                leader.0, view.0
            ),

            Self::UntrackedVote { vote_data } => write!(
                f,
                "VoteData {:?} received in NewVote message from {:?} but not found in vote_tracker",
                format_vote_data(&vote_data.data, false),
                vote_data.author
            ),

            Self::VoteCountMismatch {
                vote_data,
                received_count,
                tracked_count,
            } => write!(
                f,
                "VoteData {:?} received {} times but tracked {} times",
                format_vote_data(vote_data, false),
                received_count,
                tracked_count
            ),

            Self::MissingQCDespiteQuorum { vote_data } => write!(
                f,
                "Quorum present for {:?} but no corresponding QC found",
                format_vote_data(vote_data, false)
            ),

            Self::PendingVotesBlockNotFound {
                view,
                block_key,
                vote_type,
            } => write!(
                f,
                "Block {:?} in pending_votes.{} for view {} doesn't exist in blocks",
                format_block_key(block_key),
                vote_type,
                view.0
            ),

            Self::PendingVotesForFinalizedBlock {
                view,
                block_key,
                vote_type,
            } => write!(
                f,
                "Block {:?} in pending_votes.{} for view {} is already finalized",
                format_block_key(block_key),
                vote_type,
                view.0
            ),

            Self::PendingVotesMissingEligibleBlock {
                view,
                block_key,
                vote_type,
            } => write!(
                f,
                "Eligible block {:?} for {} vote in view {} not in pending_votes",
                format_block_key(block_key),
                vote_type,
                view.0
            ),

            Self::PendingVotesAlreadyVoted {
                view,
                block_key,
                vote_type,
            } => write!(
                f,
                "Block {:?} in pending_votes.{} for view {:?} has already voted",
                format_block_key(block_key),
                vote_type,
                view,
            ),
        }
    }
}

impl<Tr: Transaction> MorpheusProcess<Tr> {
    /// Checks key protocol invariants and returns a list of invariant violations
    ///
    /// This method is intended for testing purposes to ensure protocol invariants
    /// are maintained throughout execution.
    pub fn check_invariants(&self) -> Vec<InvariantViolation> {
        let mut violations = Vec::new();

        // Check view and phase consistency
        if !self.phase_i.contains_key(&self.view_i) {
            violations.push(InvariantViolation::ViewHasNoPhase(self.view_i));
        }

        // Check time consistency
        if self.view_entry_time > self.current_time {
            violations.push(InvariantViolation::ViewEntryTimeAfterCurrentTime {
                view_entry_time: self.view_entry_time,
                current_time: self.current_time,
            });
        }

        let qcs = self
            .qcs
            .iter()
            .map(|qc| (qc.data.clone(), qc.clone()))
            .collect::<Vec<_>>();

        // Reconstruct Q_i - the set of QCs
        // According to pseudocode: "Q_i stores at most one z-QC for each block"
        let q_i_qcs: BTreeSet<&VoteData> = qcs.iter().map(|qc| &qc.0).collect();

        // Check block DAG consistency
        for (key, block) in &self.index.blocks {
            // Check that block key matches the block's actual key
            if &block.data.key != key {
                violations.push(InvariantViolation::BlockKeyMismatch {
                    index_key: key.clone(),
                    block_key: block.data.key.clone(),
                });
            }

            // Check that each block is correctly indexed in block_pointed_by
            for qc in &block.data.prev {
                let pointed_block_key = &qc.data.for_which;
                if let Some(pointed_blocks) = self.index.block_pointed_by.get(pointed_block_key) {
                    if !pointed_blocks.contains(key) {
                        violations.push(InvariantViolation::BlockPointsToMissingFromPointedBy {
                            block: key.clone(),
                            pointed_to: pointed_block_key.clone(),
                        });
                    }
                } else {
                    violations.push(InvariantViolation::BlockPointsToMissingPointedByEntry {
                        block: key.clone(),
                        pointed_to: pointed_block_key.clone(),
                    });
                }
            }
        }

        // Check block_pointed_by consistency
        for (key, pointing_blocks) in &self.index.block_pointed_by {
            // Verify the key exists in blocks
            if !self.index.blocks.contains_key(key) && *key != GEN_BLOCK_KEY {
                violations.push(InvariantViolation::BlockPointedByContainsNonExistentBlock {
                    key: key.clone(),
                });
            }

            // Verify that each pointing block actually points to this block
            for pointing_key in pointing_blocks {
                if let Some(pointing_block) = self.index.blocks.get(pointing_key) {
                    let points_to_key = pointing_block
                        .data
                        .prev
                        .iter()
                        .any(|qc| &qc.data.for_which == key);

                    if !points_to_key {
                        violations.push(InvariantViolation::PointingBlockDoesNotActuallyPoint {
                            pointing_block: pointing_key.clone(),
                            pointed_block: key.clone(),
                        });
                    }
                } else {
                    violations.push(
                        InvariantViolation::BlockPointedByContainsNonExistentPointingBlock {
                            pointed_block: key.clone(),
                            pointing_block: pointing_key.clone(),
                        },
                    );
                }
            }
        }

        // Check QC consistency
        for (vote_data, qc) in &qcs {
            // Check that QC data matches index
            if &qc.data != vote_data {
                violations.push(InvariantViolation::QcDataMismatch {
                    index_data: vote_data.clone(),
                    qc_data: qc.data.clone(),
                });
            }
        }

        // Check tips consistency using self.observes() relation
        // "The tips of Q_i are those q ∈ Q_i such that there does not exist q' ∈ Q_i with q' ≻ q"
        let mut computed_tips = Vec::new();
        for (qc_data, qc) in &qcs {
            let is_tip = !q_i_qcs.iter().any(|qc_data2| {
                // Is there any QC that observes this one and is not the same?
                qc_data != *qc_data2
                    && self.observes((*qc_data2).clone(), qc_data)
                    && !self.observes((*qc_data).clone(), qc_data2)
            });

            if is_tip {
                computed_tips.push(Arc::clone(qc));
            }
        }

        // Check if our computed tips match the actual tips
        let actual_tips_set: BTreeSet<FinishedQC> = self.index.tips.iter().cloned().collect();
        let computed_tips_set: BTreeSet<FinishedQC> = computed_tips.into_iter().collect();

        if actual_tips_set != computed_tips_set {
            // Find elements in computed_tips but not in actual_tips
            let missing_tips: Vec<_> = computed_tips_set
                .difference(&actual_tips_set)
                .cloned()
                .collect();

            // Find elements in actual_tips but not in computed_tips
            let extra_tips: Vec<_> = actual_tips_set
                .difference(&computed_tips_set)
                .cloned()
                .collect();

            if !missing_tips.is_empty() {
                violations.push(InvariantViolation::TipsMissingQCs { missing_tips });
            }

            if !extra_tips.is_empty() {
                violations.push(InvariantViolation::TipsContainsExtraQCs { extra_tips });
            }
        }

        // Check finalization according to pseudocode definition:
        // "Process p_i regards q ∈ Q_i (and q.b) as final if there exists q' ∈ Q_i such
        // that q' ⪰ q and q is a 2-QC (for any block)."
        for (vote_data, _) in &qcs {
            // Only check 2-QCs for finalization
            if vote_data.z == 2 {
                let block_key = &vote_data.for_which;

                // Check if any QC observes this 2-QC
                let observed_by_any = qcs.iter().any(|(q_data, _)| {
                    q_data != vote_data && self.observes(q_data.clone(), vote_data)
                });

                // According to pseudocode, this 2-QC should be final if observed by any other QC
                let should_be_final = observed_by_any;

                // Check if it's actually marked as final
                let is_marked_final = self
                    .index
                    .finalized
                    .contains(block_key);

                if should_be_final && !is_marked_final {
                    violations.push(InvariantViolation::BlockWithObserved2QcNotFinalized {
                        block: block_key.clone(),
                    });
                }

                // Also check the opposite - blocks marked as final should satisfy the definition
                if is_marked_final && !should_be_final && !observed_by_any {
                    violations.push(InvariantViolation::FinalizedBlockNot2QcObserved {
                        block: block_key.clone(),
                    });
                }
            }
        }

        // Check max_height consistency
        let max_height = self.index.max_height.0;
        let max_height_key = &self.index.max_height.1;
        let actual_max_height = self
            .index
            .blocks
            .keys()
            .map(|k| k.height)
            .max()
            .unwrap_or(0);

        if max_height != actual_max_height {
            violations.push(InvariantViolation::MaxHeightMismatch {
                recorded: max_height,
                actual: actual_max_height,
            });
        }

        if max_height > 0 && !self.index.blocks.contains_key(max_height_key) {
            violations.push(InvariantViolation::MaxHeightKeyDoesNotExist {
                key: max_height_key.clone(),
            });
        }

        // Check max_1qc maximality according to compare_qc
        // "max_1qc is a maximal amongst 1-QCs seen by p_i"
        if self.index.max_1qc.data.z != 1 {
            violations.push(InvariantViolation::Max1QcHasWrongZ {
                z: self.index.max_1qc.data.z,
            });
        }

        // Check if max_1qc is actually maximal among all 1-QCs
        for (vote_data, _) in &qcs {
            if vote_data.z == 1 {
                let comparison = vote_data.compare_qc(&self.index.max_1qc.data);
                if comparison == std::cmp::Ordering::Greater {
                    violations.push(InvariantViolation::Found1QcGreaterThanMax1Qc {
                        found: vote_data.clone(),
                        max_1qc: self.index.max_1qc.data.clone(),
                    });
                }
            }
        }

        // Check finalization consistency
        for key in &self.index.finalized {
                // If finalized, it shouldn't be in unfinalized
                if self.index.unfinalized.contains_key(key) {
                    violations.push(InvariantViolation::BlockFinalizedButAlsoUnfinalized {
                        block: key.clone(),
                    });
                }
        }

        // Check unfinalized_2qc consistency
        for vote_data in &self.index.unfinalized_2qc {
            if vote_data.data.z != 2 {
                violations.push(InvariantViolation::UnfinalizedQcHasWrongZ {
                    vote_data: vote_data.clone(),
                });
            }

            // Should be in qcs
            if !qcs.iter().any(|(qc_data, _)| qc_data == &vote_data.data) {
                violations.push(InvariantViolation::UnfinalizedQcNotInQcs {
                    vote_data: vote_data.clone(),
                });
            }

            // The block should be unfinalized
            let block_key = &vote_data.data.for_which;
            if !self.index.unfinalized.contains_key(block_key) {
                violations.push(InvariantViolation::BlockForUnfinalizedQcNotInUnfinalized {
                    block: block_key.clone(),
                });
            }
        }

        // Check view leader consistency
        let leader = self.lead(self.view_i);
        if !self.verify_leader(leader.clone(), self.view_i) {
            violations.push(InvariantViolation::LeaderVerificationFailed {
                leader,
                view: self.view_i,
            });
        }

        // Count all the voting messages manually and check that a QC is present for each with quorum
        let mut vote_counts = BTreeMap::new();
        for msg in &self.received_messages {
            match msg {
                Message::NewVote(vote) => {
                    *vote_counts.entry(vote.data.clone()).or_insert(0usize) += 1;
                    if !self
                        .vote_tracker
                        .votes
                        .get(&vote.data)
                        .unwrap()
                        .contains_key(&vote.author)
                    {
                        violations.push(InvariantViolation::UntrackedVote {
                            vote_data: ThreshPartial::clone(&vote),
                        });
                    }
                }
                _ => {}
            }
        }
        for (vote_data, &received_count) in &vote_counts {
            if received_count >= (self.n - self.f) as usize {
                if !qcs.iter().any(|(qc_data, _)| qc_data == vote_data) {
                    violations.push(InvariantViolation::MissingQCDespiteQuorum {
                        vote_data: vote_data.clone(),
                    });
                }
            }
            let tracked_count = self
                .vote_tracker
                .votes
                .get(&vote_data)
                .unwrap_or(&BTreeMap::new())
                .len();
            if received_count != tracked_count {
                violations.push(InvariantViolation::VoteCountMismatch {
                    vote_data: vote_data.clone(),
                    received_count,
                    tracked_count,
                });
            }
        }

        for (view, pending) in &self.pending_votes {
            for block_key in pending.tr_1.keys() {
                if !self.index.blocks.contains_key(block_key) {
                    violations.push(InvariantViolation::PendingVotesBlockNotFound {
                        view: *view,
                        block_key: block_key.clone(),
                        vote_type: "tr_1".to_string(),
                    });
                    continue;
                }

                if self
                    .index
                    .finalized
                    .contains(block_key)
                {
                    violations.push(InvariantViolation::PendingVotesForFinalizedBlock {
                        view: *view,
                        block_key: block_key.clone(),
                        vote_type: "tr_1".to_string(),
                    });
                }

                if self.voted_i.contains(&(
                    1,
                    block_key.type_,
                    block_key.slot,
                    block_key.author.clone().unwrap(),
                )) {
                    violations.push(InvariantViolation::PendingVotesAlreadyVoted {
                        view: *view,
                        block_key: block_key.clone(),
                        vote_type: "tr_1".to_string(),
                    });
                }
            }

            for block_key in pending.tr_2.keys() {
                if !self.index.blocks.contains_key(block_key) {
                    violations.push(InvariantViolation::PendingVotesBlockNotFound {
                        view: *view,
                        block_key: block_key.clone(),
                        vote_type: "tr_2".to_string(),
                    });
                    continue;
                }

                if self
                    .index
                    .finalized
                    .contains(block_key)
                {
                    violations.push(InvariantViolation::PendingVotesForFinalizedBlock {
                        view: *view,
                        block_key: block_key.clone(),
                        vote_type: "tr_2".to_string(),
                    });
                }
                if self.voted_i.contains(&(
                    2,
                    block_key.type_,
                    block_key.slot,
                    block_key.author.clone().unwrap(),
                )) {
                    violations.push(InvariantViolation::PendingVotesAlreadyVoted {
                        view: *view,
                        block_key: block_key.clone(),
                        vote_type: "tr_2".to_string(),
                    });
                }
            }

            for block_key in pending.lead_1.keys() {
                if !self.index.blocks.contains_key(block_key) {
                    violations.push(InvariantViolation::PendingVotesBlockNotFound {
                        view: *view,
                        block_key: block_key.clone(),
                        vote_type: "lead_1".to_string(),
                    });
                    continue;
                }

                if self
                    .index
                    .finalized
                    .contains(block_key)
                {
                    violations.push(InvariantViolation::PendingVotesForFinalizedBlock {
                        view: *view,
                        block_key: block_key.clone(),
                        vote_type: "lead_1".to_string(),
                    });
                }

                if self.voted_i.contains(&(
                    1,
                    block_key.type_,
                    block_key.slot,
                    block_key.author.clone().unwrap(),
                )) {
                    violations.push(InvariantViolation::PendingVotesAlreadyVoted {
                        view: *view,
                        block_key: block_key.clone(),
                        vote_type: "lead_1".to_string(),
                    });
                }
            }

            for block_key in pending.lead_2.keys() {
                if !self.index.blocks.contains_key(block_key) {
                    violations.push(InvariantViolation::PendingVotesBlockNotFound {
                        view: *view,
                        block_key: block_key.clone(),
                        vote_type: "lead_2".to_string(),
                    });
                    continue;
                }

                if self
                    .index
                    .finalized
                    .contains(block_key)
                {
                    violations.push(InvariantViolation::PendingVotesForFinalizedBlock {
                        view: *view,
                        block_key: block_key.clone(),
                        vote_type: "lead_2".to_string(),
                    });
                }

                if self.voted_i.contains(&(
                    2,
                    block_key.type_,
                    block_key.slot,
                    block_key.author.clone().unwrap(),
                )) {
                    violations.push(InvariantViolation::PendingVotesAlreadyVoted {
                        view: *view,
                        block_key: block_key.clone(),
                        vote_type: "lead_2".to_string(),
                    });
                }
            }

            // For the current view, check if all eligible blocks are in pending_votes
            if *view == self.view_i {
                for (block_key, _) in &self.index.blocks {
                    if block_key.type_ == BlockType::Tr
                        && block_key.view == self.view_i
                        && !self
                            .index
                            .finalized
                            .contains(block_key)
                        && self.is_eligible_for_tr_1_vote(block_key)
                        && !pending.tr_1.contains_key(block_key)
                    {
                        violations.push(InvariantViolation::PendingVotesMissingEligibleBlock {
                            view: *view,
                            block_key: block_key.clone(),
                            vote_type: "tr_1".to_string(),
                        });
                    }
                }

                for (vote_data, _) in &qcs {
                    if vote_data.z == 1
                        && vote_data.for_which.type_ == BlockType::Tr
                        && vote_data.for_which.view == self.view_i
                        && !self
                            .index
                            .finalized
                            .contains(&vote_data.for_which)
                        && self.is_eligible_for_tr_2_vote(&vote_data.for_which)
                        && !pending.tr_2.contains_key(&vote_data.for_which)
                    {
                        violations.push(InvariantViolation::PendingVotesMissingEligibleBlock {
                            view: *view,
                            block_key: vote_data.for_which.clone(),
                            vote_type: "tr_2".to_string(),
                        });
                    }
                }
            }
        }

        violations
    }
}
