use crate::debug_impls::*;
use crate::*;

use std::collections::BTreeSet;
use std::fmt;

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
    QcNotInQcByView {
        vote_data: VoteData,
        view_key: (BlockType, Identity, ViewNum),
    },

    // Tips related violations
    TipsMissingQCs {
        missing_tips: Vec<VoteData>,
    },
    TipsContainsExtraQCs {
        extra_tips: Vec<VoteData>,
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
        vote_data: VoteData,
    },
    UnfinalizedQcNotInQcs {
        vote_data: VoteData,
    },
    BlockForUnfinalizedQcNotInUnfinalized {
        block: BlockKey,
    },

    // Leader consistency
    LeaderVerificationFailed {
        leader: Identity,
        view: ViewNum,
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

            Self::QcNotInQcByView {
                vote_data,
                view_key,
            } => write!(
                f,
                "QC {:?} not found in qc_by_view for key {:?}",
                format_vote_data(vote_data, false),
                view_key
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
                format_vote_data(vote_data, false),
                vote_data.z
            ),

            Self::UnfinalizedQcNotInQcs { vote_data } => write!(
                f,
                "VoteData {:?} in unfinalized_2qc not found in qcs",
                format_vote_data(vote_data, false)
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
        }
    }
}

impl MorpheusProcess {
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

        // Reconstruct Q_i - the set of QCs
        // According to pseudocode: "Q_i stores at most one z-QC for each block"
        let q_i_qcs: BTreeSet<&VoteData> = self.qcs.keys().collect();

        // Check block DAG consistency
        for (key, block) in &self.blocks {
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
                if let Some(pointed_blocks) = self.block_pointed_by.get(pointed_block_key) {
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
        for (key, pointing_blocks) in &self.block_pointed_by {
            // Verify the key exists in blocks
            if !self.blocks.contains_key(key) && *key != GEN_BLOCK_KEY {
                violations.push(InvariantViolation::BlockPointedByContainsNonExistentBlock {
                    key: key.clone(),
                });
            }

            // Verify that each pointing block actually points to this block
            for pointing_key in pointing_blocks {
                if let Some(pointing_block) = self.blocks.get(pointing_key) {
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
        for (vote_data, qc) in &self.qcs {
            // Check that QC data matches index
            if &qc.data != vote_data {
                violations.push(InvariantViolation::QcDataMismatch {
                    index_data: vote_data.clone(),
                    qc_data: qc.data.clone(),
                });
            }

            // Check that QC is correctly indexed in qc_index
            let index_key = (
                vote_data.for_which.type_,
                vote_data
                    .for_which
                    .author
                    .clone()
                    .unwrap_or(Identity(u64::MAX)),
                vote_data.for_which.slot,
            );
            if let Some(indexed_qc) = self.qc_by_slot.get(&index_key) {
                if &indexed_qc.data != vote_data {
                    violations.push(InvariantViolation::QcIndexMismatch {
                        qc_index_data: indexed_qc.data.clone(),
                        qc_data: vote_data.clone(),
                    });
                }
            } else {
                violations.push(InvariantViolation::QcNotInQcIndex {
                    vote_data: vote_data.clone(),
                    qc_index: format!("{:?}", self.qc_by_slot),
                });
            }

            // Check that QC is correctly indexed in qc_by_view
            let view_key = (
                vote_data.for_which.type_,
                vote_data
                    .for_which
                    .author
                    .clone()
                    .unwrap_or(Identity(u64::MAX)),
                vote_data.for_which.view,
            );
            if let Some(view_qcs) = self.qc_by_view.get(&view_key) {
                if !view_qcs.iter().any(|q| &q.data == vote_data) {
                    violations.push(InvariantViolation::QcNotInQcByView {
                        vote_data: vote_data.clone(),
                        view_key,
                    });
                }
            } else {
                violations.push(InvariantViolation::QcNotInQcByView {
                    vote_data: vote_data.clone(),
                    view_key,
                });
            }
        }

        // Check tips consistency using self.observes() relation
        // "The tips of Q_i are those q ∈ Q_i such that there does not exist q' ∈ Q_i with q' ≻ q"
        let mut computed_tips = Vec::new();
        for qc_data in q_i_qcs.iter() {
            let is_tip = !q_i_qcs.iter().any(|qc_data2| {
                // Is there any QC that observes this one and is not the same?
                qc_data != qc_data2
                    && self.observes((*qc_data2).clone(), qc_data)
                    && !self.observes((*qc_data).clone(), qc_data2)
            });

            if is_tip {
                computed_tips.push((*qc_data).clone());
            }
        }

        // Check if our computed tips match the actual tips
        let actual_tips_set: BTreeSet<VoteData> = self.tips.iter().cloned().collect();
        let computed_tips_set: BTreeSet<VoteData> = computed_tips.into_iter().collect();

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
        for (vote_data, _) in &self.qcs {
            // Only check 2-QCs for finalization
            if vote_data.z == 2 {
                let block_key = &vote_data.for_which;

                // Check if any QC observes this 2-QC
                let observed_by_any = self
                    .qcs
                    .keys()
                    .any(|q_data| q_data != vote_data && self.observes(q_data.clone(), vote_data));

                // According to pseudocode, this 2-QC should be final if observed by any other QC
                let should_be_final = observed_by_any;

                // Check if it's actually marked as final
                let is_marked_final = self.finalized.get(block_key).cloned().unwrap_or(false);

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
        let max_height = self.max_height.0;
        let max_height_key = &self.max_height.1;
        let actual_max_height = self.blocks.keys().map(|k| k.height).max().unwrap_or(0);

        if max_height != actual_max_height {
            violations.push(InvariantViolation::MaxHeightMismatch {
                recorded: max_height,
                actual: actual_max_height,
            });
        }

        if max_height > 0 && !self.blocks.contains_key(max_height_key) {
            violations.push(InvariantViolation::MaxHeightKeyDoesNotExist {
                key: max_height_key.clone(),
            });
        }

        // Check max_1qc maximality according to compare_qc
        // "max_1qc is a maximal amongst 1-QCs seen by p_i"
        if self.max_1qc.data.z != 1 {
            violations.push(InvariantViolation::Max1QcHasWrongZ {
                z: self.max_1qc.data.z,
            });
        }

        // Check if max_1qc is actually maximal among all 1-QCs
        for (vote_data, _) in &self.qcs {
            if vote_data.z == 1 {
                let comparison = vote_data.compare_qc(&self.max_1qc.data);
                if comparison == std::cmp::Ordering::Greater {
                    violations.push(InvariantViolation::Found1QcGreaterThanMax1Qc {
                        found: vote_data.clone(),
                        max_1qc: self.max_1qc.data.clone(),
                    });
                }
            }
        }

        // Check finalization consistency
        for (key, is_finalized) in &self.finalized {
            if *is_finalized {
                // If finalized, it shouldn't be in unfinalized
                if self.unfinalized.contains_key(key) {
                    violations.push(InvariantViolation::BlockFinalizedButAlsoUnfinalized {
                        block: key.clone(),
                    });
                }
            }
        }

        // Check unfinalized_2qc consistency
        for vote_data in &self.unfinalized_2qc {
            if vote_data.z != 2 {
                violations.push(InvariantViolation::UnfinalizedQcHasWrongZ {
                    vote_data: vote_data.clone(),
                });
            }

            // Should be in qcs
            if !self.qcs.contains_key(vote_data) {
                violations.push(InvariantViolation::UnfinalizedQcNotInQcs {
                    vote_data: vote_data.clone(),
                });
            }

            // The block should be unfinalized
            let block_key = &vote_data.for_which;
            if !self.unfinalized.contains_key(block_key) {
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

        violations
    }
}
