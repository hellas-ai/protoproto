use std::collections::VecDeque;
use crate::*;

impl MorpheusProcess {
    /// Check if one QC observes another via the observes relation ⪰
    pub fn observes(&self, root: VoteData, needle: &VoteData) -> bool {
        let mut to_visit: VecDeque<VoteData> = vec![root].into();
        while let Some(node) = to_visit.pop_front() {
            if self.directly_observes(&node, needle) {
                return true;
            }
            if let Some(block) = self.index.blocks.get(&node.for_which) {
                for prev in &block.data.prev {
                    to_visit.push_back(prev.data.clone());
                }
            } else {
                tracing::warn!("Block not found for {:?}", node.for_which);
            }
        }
        false
    }

    /// Direct observation: same type/author and slot/view relationships or explicit pointer
    pub fn directly_observes(&self, looks: &VoteData, seen: &VoteData) -> bool {
        if looks.for_which.type_ == seen.for_which.type_
            && looks.for_which.author == seen.for_which.author
            && looks.for_which.slot > seen.for_which.slot
        {
            return true;
        }
        if looks.for_which.type_ == seen.for_which.type_
            && looks.for_which.author == seen.for_which.author
            && looks.for_which.slot == seen.for_which.slot
            && looks.z >= seen.z
        {
            return true;
        }
        if let Some(block) = self.index.blocks.get(&looks.for_which) {
            block.data.prev.iter().any(|prev| prev.data.for_which == seen.for_which)
        } else {
            false
        }
    }
}