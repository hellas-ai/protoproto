use std::collections::BTreeSet;
use fastbloom::BloomFilter;
use serde::{Deserialize, Serialize};
use crate::*;

/// Handles message processing and deduplication
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MessageHandler {
    /// Bloom filter for fast duplicate detection
    #[serde(skip)]
    pub seen_messages: BloomFilter,
    
    /// Exact hash set for definitive duplicate detection
    pub seen_message_hashes: BTreeSet<[u8; 32]>,
}

impl Default for MessageHandler {
    fn default() -> Self {
        Self {
            seen_messages: BloomFilter::with_num_bits(8 * 1024 * 16)
                .seed(&0x8F3A57D2C19E4B7F0123456789ABCDEF)
                .expected_items(100_000),
            seen_message_hashes: BTreeSet::new(),
        }
    }
}

impl MessageHandler {
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Check if we've seen this message before
    pub fn is_duplicate<Tr: Transaction>(&self, message: &Message<Tr>) -> bool {
        if self.seen_messages.contains(message) {
            let bytes = postcard::to_stdvec(message).unwrap();
            let hash = blake3::hash(&bytes);
            return self.seen_message_hashes.contains(hash.as_bytes());
        }
        false
    }
    
    /// Record that we've seen this message
    pub fn record_message<Tr: Transaction>(&mut self, message: &Message<Tr>) {
        self.seen_messages.insert(message);
        let bytes = postcard::to_stdvec(message).unwrap();
        let hash = blake3::hash(&bytes);
        self.seen_message_hashes.insert(*hash.as_bytes());
    }
} 