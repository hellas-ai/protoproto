use std::sync::Arc;
use crate::*;

impl MorpheusProcess {
    /// Record a new block into the state index and prepare votes
    pub fn record_block(&mut self, block: &Arc<Signed<Block>>) {
        if self.index.blocks.contains_key(&block.data.key) {
            tracing::warn!(target: "duplicate_block", key = ?block.data.key);
            return;
        }
        // update max height
        if block.data.key.height > self.index.max_height.0 {
            tracing::debug!(target: "new_max_height", height = block.data.key.height, key = ?block.data.key);
            self.index.max_height = (block.data.key.height, block.data.key.clone());
        }
        // index by author for lead blocks
        if let Some(author) = &block.data.key.author {
            self.index.block_index
                .entry((block.data.key.type_, block.data.key.view, author.clone()))
                .or_default().push(block.clone());
            if block.data.key.type_ == BlockType::Lead && author == &self.id {
                self.produced_lead_in_view.insert(block.data.key.view, true);
            }
        }
        // add to blocks and mark unfinalized
        let key = block.data.key.clone();
        self.index.finalized.insert(key.clone(), false);
        self.index.blocks.insert(key.clone(), block.clone());
        // update pending votes
        let pending = self.pending_votes.entry(block.data.key.view).or_default();
        match block.data.key.type_ {
            BlockType::Lead => {
                self.index.contains_lead_by_view.insert(block.data.key.view, true);
                self.index.unfinalized_lead_by_view.entry(block.data.key.view).or_default().insert(key.clone());
                pending.lead_1.insert(key.clone(), true);
            }
            BlockType::Tr => { pending.tr_1.insert(key.clone(), true); }
            BlockType::Genesis => panic!("Why are we recording the genesis block?"),
        }
        pending.dirty = true;
        // record pointers and QCs in the block
        for qc in &block.data.prev {
            self.index.block_pointed_by.entry(qc.data.for_which.clone()).or_default().insert(key.clone());
        }
        for qc in block.data.prev.iter().chain(Some(&block.data.one)) {
            self.record_qc(Arc::new(qc.clone()));
        }
    }
}