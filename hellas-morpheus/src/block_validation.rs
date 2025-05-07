use crate::*;
use std::{fmt, sync::Arc};

/// Represents the different ways a block validation can fail
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockValidationError {
    // Signature validation
    InvalidSignature,

    // Genesis block validation
    InvalidGenesisBlock {
        key: BlockKey,
    },

    // Block author validation
    MissingAuthor {
        key: BlockKey,
    },

    // Block structure validation
    EmptyPrevPointers,

    // QC validation
    PrevQcViewGreaterThanBlockView {
        prev_view: ViewNum,
        block_view: ViewNum,
    },
    PrevQcHeightGreaterOrEqualBlockHeight {
        prev_height: usize,
        block_height: usize,
    },

    // One-QC validation
    OneQcNotZ1 {
        z: u8,
    },
    OneQcHeightGreaterOrEqualBlockHeight {
        qc_height: usize,
        block_height: usize,
    },

    // Height consistency
    InvalidHeight {
        block_height: usize,
        max_prev_height: usize,
    },

    // Block type-specific validation
    BlockDataTypeMismatch {
        key_type: BlockType,
        data_type: BlockType,
    },

    // Transaction block validation
    MissingPredecessorTrBlock {
        slot: SlotNum,
    },
    EmptyTransactions,

    // Leader block validation
    NotLeader {
        leader: Identity,
        view: ViewNum,
    },
    MissingPredecessorLeadBlock {
        slot: SlotNum,
    },
    IncorrectOneQcForLeadBlock {
        one_qc_for: BlockKey,
        expected_for: BlockKey,
    },
    InvalidJustificationSize {
        size: usize,
        expected: usize,
    },
    InvalidJustificationSignature,
    JustificationQcLessThanOneQc,
    InvalidPrevQcSignature,
    InvalidOneQcSignature,
    InvalidGenesisOneQc,
}

impl fmt::Display for BlockValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSignature => write!(f, "Block has invalid signature"),

            Self::InvalidGenesisBlock { key } => {
                write!(f, "Invalid genesis block with key {:?}", key)
            }

            Self::MissingAuthor { key } => write!(f, "Block key {:?} is missing author", key),

            Self::EmptyPrevPointers => write!(f, "Block has empty prev pointers"),

            Self::PrevQcViewGreaterThanBlockView {
                prev_view,
                block_view,
            } => write!(
                f,
                "Prev QC view {} > block view {}",
                prev_view.0, block_view.0
            ),

            Self::PrevQcHeightGreaterOrEqualBlockHeight {
                prev_height,
                block_height,
            } => write!(
                f,
                "Prev QC height {} >= block height {}",
                prev_height, block_height
            ),

            Self::OneQcNotZ1 { z } => write!(f, "Block's one-QC has z = {} instead of 1", z),

            Self::OneQcHeightGreaterOrEqualBlockHeight {
                qc_height,
                block_height,
            } => write!(
                f,
                "Block's one-QC height {} >= block height {}",
                qc_height, block_height
            ),

            Self::InvalidHeight {
                block_height,
                max_prev_height,
            } => write!(
                f,
                "Block height {} is not exactly 1 more than max prev height {}",
                block_height, max_prev_height
            ),

            Self::BlockDataTypeMismatch {
                key_type,
                data_type,
            } => write!(
                f,
                "Block key type {:?} does not match block data type {:?}",
                key_type, data_type
            ),

            Self::MissingPredecessorTrBlock { slot } => write!(
                f,
                "Transaction block at slot {} is missing predecessor",
                slot.0
            ),

            Self::EmptyTransactions => write!(f, "Transaction block has no transactions"),

            Self::NotLeader { leader, view } => write!(
                f,
                "Block author {} is not the leader for view {}",
                leader.0, view.0
            ),

            Self::MissingPredecessorLeadBlock { slot } => {
                write!(f, "Leader block at slot {} is missing predecessor", slot.0)
            }

            Self::IncorrectOneQcForLeadBlock {
                one_qc_for,
                expected_for,
            } => write!(
                f,
                "Leader block's one-QC points to {:?} instead of {:?}",
                one_qc_for, expected_for
            ),

            Self::InvalidJustificationSize { size, expected } => write!(
                f,
                "Leader block justification has size {} instead of expected {}",
                size, expected
            ),

            Self::InvalidJustificationSignature => {
                write!(f, "Leader block justification contains invalid signatures")
            }

            Self::JustificationQcLessThanOneQc => {
                write!(f, "Leader block justification contains QC less than one-QC")
            }
            Self::InvalidPrevQcSignature => write!(f, "Prev QC has invalid signature"),
            Self::InvalidOneQcSignature => write!(f, "One-QC has invalid signature"),
            Self::InvalidGenesisOneQc => write!(f, "One-QC referring to genesis block is invalid"),
        }
    }
}

impl<Tr: Transaction> MorpheusProcess<Tr> {
    /// Validates a block according to the Morpheus protocol rules
    ///
    /// Returns Ok(()) if the block is valid, or the specific error that caused validation to fail
    pub fn block_valid(
        &self,
        signed_block: &Signed<Block<Tr>>,
    ) -> Result<(), BlockValidationError> {
        let block = &signed_block.data;

        // validate the genesis block, otherwise extract the author
        let author = if let BlockType::Genesis = block.key.type_ {
            if block.key == GEN_BLOCK_KEY
                && block.prev.is_empty()
                && block.one == *self.genesis_qc
                && block.data == BlockData::Genesis
            {
                return Ok(());
            } else {
                return Err(BlockValidationError::InvalidGenesisBlock {
                    key: block.key.clone(),
                });
            }
        } else {
            if let Some(auth) = block.key.author.clone() {
                auth
            } else {
                return Err(BlockValidationError::MissingAuthor {
                    key: block.key.clone(),
                });
            }
        };

        if !signed_block.valid_signature(&self.kb) {
            return Err(BlockValidationError::InvalidSignature);
        }

        if block.prev.is_empty() {
            return Err(BlockValidationError::EmptyPrevPointers);
        }

        for prev in &block.prev {
            if prev.data.for_which.view > block.key.view {
                return Err(BlockValidationError::PrevQcViewGreaterThanBlockView {
                    prev_view: prev.data.for_which.view,
                    block_view: block.key.view,
                });
            }
            if prev.data.for_which.height >= block.key.height {
                return Err(
                    BlockValidationError::PrevQcHeightGreaterOrEqualBlockHeight {
                        prev_height: prev.data.for_which.height,
                        block_height: block.key.height,
                    },
                );
            }
            if prev != &*self.genesis_qc && !prev.valid_signature(&self.kb, self.n - self.f) {
                return Err(BlockValidationError::InvalidPrevQcSignature);
            }
        }

        if block.one.data.z != 1 {
            return Err(BlockValidationError::OneQcNotZ1 {
                z: block.one.data.z,
            });
        }

        if block.one.data.for_which.height >= block.key.height {
            return Err(BlockValidationError::OneQcHeightGreaterOrEqualBlockHeight {
                qc_height: block.one.data.for_which.height,
                block_height: block.key.height,
            });
        }

        if block.one.data.for_which.type_ != BlockType::Genesis {
            if !block.one.valid_signature(&self.kb, self.n - self.f) {
                return Err(BlockValidationError::InvalidOneQcSignature);
            }
        } else {
            if *self.genesis_qc != block.one {
                return Err(BlockValidationError::InvalidGenesisOneQc);
            }
        }

        match block.prev.iter().max_by_key(|qc| qc.data.for_which.height) {
            None => (),
            Some(qc_max_height) => {
                if block.key.height != qc_max_height.data.for_which.height + 1 {
                    return Err(BlockValidationError::InvalidHeight {
                        block_height: block.key.height,
                        max_prev_height: qc_max_height.data.for_which.height,
                    });
                }
            }
        }

        match &block.data {
            BlockData::Genesis => unreachable!("genesis blocks are validated above"),
            BlockData::Tr { transactions } => {
                if block.key.type_ != BlockType::Tr {
                    return Err(BlockValidationError::BlockDataTypeMismatch {
                        key_type: block.key.type_,
                        data_type: BlockType::Tr,
                    });
                }
                if block.key.slot > SlotNum(0) {
                    if !block.prev.iter().any(|qc| {
                        qc.data.for_which.type_ == BlockType::Tr
                            && qc.data.for_which.author == Some(author.clone())
                            && qc.data.for_which.slot.is_pred(block.key.slot)
                    }) {
                        return Err(BlockValidationError::MissingPredecessorTrBlock {
                            slot: block.key.slot,
                        });
                    }
                }
                if transactions.is_empty() {
                    return Err(BlockValidationError::EmptyTransactions);
                }
            }
            BlockData::Lead { justification } => {
                if block.key.type_ != BlockType::Lead {
                    return Err(BlockValidationError::BlockDataTypeMismatch {
                        key_type: block.key.type_,
                        data_type: BlockType::Lead,
                    });
                }

                let leader = block.key.author.clone().unwrap();
                if !self.verify_leader(leader.clone(), block.key.view) {
                    return Err(BlockValidationError::NotLeader {
                        leader,
                        view: block.key.view,
                    });
                }

                let prev_leader_for: Vec<&ThreshSigned<VoteData>> = block
                    .prev
                    .iter()
                    .filter(|qc| {
                        qc.data.for_which.type_ == BlockType::Lead
                            && qc.data.for_which.author == Some(author.clone())
                            && qc.data.for_which.slot.is_pred(block.key.slot)
                    })
                    .collect();

                if block.key.slot > SlotNum(0) {
                    if prev_leader_for.len() != 1 {
                        return Err(BlockValidationError::MissingPredecessorLeadBlock {
                            slot: block.key.slot,
                        });
                    }

                    if prev_leader_for[0].data.for_which.view == block.key.view {
                        if block.one.data.for_which != prev_leader_for[0].data.for_which {
                            return Err(BlockValidationError::IncorrectOneQcForLeadBlock {
                                one_qc_for: block.one.data.for_which.clone(),
                                expected_for: prev_leader_for[0].data.for_which.clone(),
                            });
                        }
                    }
                }

                if block.key.slot == SlotNum(0)
                    || prev_leader_for[0].data.for_which.view < block.key.view
                {
                    let mut just: Vec<Arc<Signed<StartView>>> = justification.clone();
                    just.sort_by(|m1, m2| m1.author.cmp(&m2.author));

                    if just.len() < self.n as usize - self.f as usize {
                        return Err(BlockValidationError::InvalidJustificationSize {
                            size: just.len(),
                            expected: (self.n - self.f) as usize,
                        });
                    }

                    if !just.iter().all(|j| j.valid_signature(&self.kb)) {
                        return Err(BlockValidationError::InvalidJustificationSignature);
                    }

                    if !just.iter().all(|j| {
                        block.one.data.compare_qc(&j.data.qc.data) != std::cmp::Ordering::Less
                    }) {
                        return Err(BlockValidationError::JustificationQcLessThanOneQc);
                    }
                }
            }
        }

        Ok(())
    }
}
