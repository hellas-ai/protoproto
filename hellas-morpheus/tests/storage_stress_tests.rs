//! Stress tests for the storage subsystem
//!
//! This module contains stress tests that push the storage subsystem
//! to its limits and verify it behaves correctly under load.

mod common;

use hellas_morpheus::*;
use hellas_morpheus::test_harness::TestTransaction;
use hellas_morpheus::*;
use redb::Database;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Instant;

/// Create a test database
fn create_test_db() -> Arc<Database> {
    Arc::new(
        redb::Builder::new()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap(),
    )
}

/// Generate a block with given parameters
fn generate_block(
    view: i64,
    height: usize,
    author: u32,
    tx_count: usize,
) -> Arc<Signed<Block<TestTransaction>>> {
    let block_key = BlockKey {
        type_: BlockType::Tr,
        view: ViewNum(view),
        height: height as u64,
        author: Some(Identity(author)),
        slot: SlotNum(height as u64),
        hash: Some(BlockHash(view as u64 * 1000000 + height as u64)),
    };

    let genesis_qc = Arc::new(ThreshSigned {
        data: VoteData {
            z: 1,
            for_which: GEN_BLOCK_KEY,
        },
        signature: hints::Signature::default(),
    });

    let transactions: Vec<TestTransaction> = (0..tx_count)
        .map(|i| TestTransaction {
            id: (view as u64 * 1000 + i as u64),
            data: vec![i as u8; 100], // 100 bytes per transaction
        })
        .collect();

    let block = Block {
        key: block_key.clone(),
        prev: vec![genesis_qc.clone()],
        one: genesis_qc,
        data: BlockData::Tr { transactions },
    };

    Arc::new(Signed {
        data: block,
        author: Identity(author),
        signature: hints::PartialSignature::default(),
    })
}

#[test]
fn test_bulk_store_high_volume() {
    let db = create_test_db();
    let mut store = RedbBulkStore::new(db.clone()).unwrap();

    let start = Instant::now();

    // Insert 1000 blocks across 100 views
    for view in 0..100 {
        for i in 0..10 {
            let block = generate_block(view, (view * 10 + i) as usize, ((i % 4) + 1) as u32, 10);
            store.append_block(block).unwrap();
        }
    }

    let insert_time = start.elapsed();
    println!("Inserted 1000 blocks in {:?}", insert_time);

    // Verify all blocks can be retrieved
    let start = Instant::now();
    let mut retrieved = 0;

    for view in 0..100 {
        let blocks_in_view = store.get_blocks_in_view(ViewNum(view)).unwrap();
        assert_eq!(blocks_in_view.len(), 10);

        for block_ref in blocks_in_view {
            let block = store.get_block(&block_ref).unwrap();
            assert!(block.is_some());
            retrieved += 1;
        }
    }

    let retrieve_time = start.elapsed();
    println!("Retrieved {} blocks in {:?}", retrieved, retrieve_time);
    assert_eq!(retrieved, 1000);
}

#[test]
fn test_view_cache_eviction_under_load() {
    let mut cache = ViewCache::new(ViewNum(0));

    // Fill cache with blocks from many views
    for view in 0..100 {
        // Simulate view transition
        cache.transition_to_view(ViewNum(view));

        for i in 0..10 {
            let block = generate_block(view, (view * 10 + i) as usize, ((i % 4) + 1) as u32, 5);
            let block_ref = BlockRef {
                key: block.data.key.clone(),
                hash: block.data.key.hash.clone(),
            };
            cache.insert_block(block.clone(), block_ref);

            // Create and cache corresponding QC
            let qc = Arc::new(ThreshSigned {
                data: VoteData {
                    z: 1,
                    for_which: block.data.key.clone(),
                },
                signature: hints::Signature::default(),
            });
            let qc_ref = QCRef {
                vote_data: qc.data.clone(),
                hash: None,
            };
            cache.insert_qc(qc, qc_ref);
        }
    }

    // After transitioning through views, only the last view's data should be cached
    // Check that recent view data is still there
    for i in 0..10 {
        let key = BlockKey {
            type_: BlockType::Tr,
            view: ViewNum(99),
            height: (99 * 10 + i) as u64,
            author: Some(Identity(((i % 4) + 1) as u32)),
            slot: SlotNum((99 * 10 + i) as u64),
            hash: Some(BlockHash(99_u64 * 1000000 + (99 * 10 + i) as u64)),
        };
        assert!(cache.get_block(&key).is_some());
    }

    // Check that old view data is gone
    for view in 0..99 {
        for i in 0..10 {
            let key = BlockKey {
                type_: BlockType::Tr,
                view: ViewNum(view),
                height: (view * 10 + i) as u64,
                author: Some(Identity(((i % 4) + 1) as u32)),
                slot: SlotNum((view * 10 + i) as u64),
                hash: Some(BlockHash(view as u64 * 1000000 + (view * 10 + i) as u64)),
            };
            assert!(cache.get_block(&key).is_none());
        }
    }
}

#[test]
fn test_snapshot_store_many_snapshots() {
    let db = create_test_db();
    let mut store = RedbSnapshotStore::new(db.clone()).unwrap();

    let start = Instant::now();

    // Create 100 snapshots
    for i in 0..100 {
        let state = ConsensusState {
            current_view: ViewNum(i),
            current_phase: if i % 2 == 0 { Phase::High } else { Phase::Low },
            view_entry_time: i as u128 * 1000,
            tips: vec![QCRef {
                vote_data: VoteData {
                    z: 2,
                    for_which: BlockKey {
                        type_: BlockType::Tr,
                        view: ViewNum(i),
                        height: (i as u64) * 10,
                        author: Some(Identity((i % 4 + 1) as u32)),
                        slot: SlotNum(i as u64 * 10),
                        hash: Some(BlockHash(i as u64 * 10000)),
                    },
                },
                hash: None,
            }],
            max_1qc: QCRef {
                vote_data: VoteData {
                    z: 1,
                    for_which: GEN_BLOCK_KEY,
                },
                hash: None,
            },
            finalized_blocks: (0..5)
                .map(|j| BlockRef {
                    key: BlockKey {
                        type_: BlockType::Tr,
                        view: ViewNum(i),
                        height: (i * 10 + j) as u64,
                        author: Some(Identity((j % 4 + 1) as u32)),
                        slot: SlotNum((i * 10 + j) as u64),
                        hash: Some(BlockHash((i * 10000 + j * 100) as u64)),
                    },
                    hash: Some(BlockHash((i * 10000 + j * 100) as u64)),
                })
                .collect(),
            unfinalized_qcs: im::HashMap::new(),
            leader_blocks_by_view: im::HashMap::new(),
            unfinalized_leader_by_view: im::HashMap::new(),
        };

        store.save_snapshot(&state).unwrap();
    }

    let save_time = start.elapsed();
    println!("Saved 100 snapshots in {:?}", save_time);

    // Verify latest snapshot can be loaded
    // First save a final state
    let final_state = ConsensusState {
        current_view: ViewNum(99),
        current_phase: Phase::Low,
        view_entry_time: 99 * 1000,
        tips: vec![QCRef {
            vote_data: VoteData {
                z: 2,
                for_which: GEN_BLOCK_KEY,
            },
            hash: None,
        }],
        max_1qc: QCRef {
            vote_data: VoteData {
                z: 1,
                for_which: GEN_BLOCK_KEY,
            },
            hash: None,
        },
        finalized_blocks: im::HashSet::new(),
        unfinalized_qcs: im::HashMap::new(),
        leader_blocks_by_view: im::HashMap::new(),
        unfinalized_leader_by_view: im::HashMap::new(),
    };

    let start = Instant::now();
    let root = store.save_snapshot(&final_state).unwrap();
    let loaded = store.load_snapshot(&root).unwrap();
    assert!(loaded.is_some());
    assert_eq!(loaded.unwrap().current_view, ViewNum(99));

    let load_time = start.elapsed();
    println!("Loaded latest snapshot in {:?}", load_time);

    // Test pruning performance
    let start = Instant::now();
    store.prune_snapshots(20).unwrap();
    let prune_time = start.elapsed();
    println!("Pruned snapshots in {:?}", prune_time);
}

#[test]
fn test_concurrent_access_simulation() {
    // This test simulates concurrent access patterns
    let db = create_test_db();
    let mut bulk_store = RedbBulkStore::new(db.clone()).unwrap();
    let mut snapshot_store = RedbSnapshotStore::new(db.clone()).unwrap();
    let mut cache = ViewCache::new(ViewNum(0));

    // Simulate a running protocol with mixed operations
    for round in 0..20 {
        let view = round * 5;

        // Update cache view
        cache.transition_to_view(ViewNum(view + 4));

        // Phase 1: Add new blocks
        for i in 0..5 {
            let block =
                generate_block(view + i, (view + i) as usize * 10, ((i % 4) + 1) as u32, 20);
            let block_ref = bulk_store.append_block(block.clone()).unwrap();
            cache.insert_block(block.clone(), block_ref);

            // Add QCs
            let qc = Arc::new(ThreshSigned {
                data: VoteData {
                    z: 1,
                    for_which: block.data.key.clone(),
                },
                signature: hints::Signature::default(),
            });
            let qc_ref = bulk_store.append_qc(qc.clone()).unwrap();
            cache.insert_qc(qc, qc_ref);
        }

        // Phase 2: Add votes
        for i in 0..5 {
            for voter in 1..=4 {
                let vote = Arc::new(ThreshPartial {
                    data: VoteData {
                        z: 1,
                        for_which: BlockKey {
                            type_: BlockType::Tr,
                            view: ViewNum(view + i),
                            height: (view + i) as u64 * 10,
                            author: Some(Identity(((i % 4) + 1) as u32)),
                            slot: SlotNum((view + i) as u64 * 10),
                            hash: Some(BlockHash(
                                (view + i) as u64 * 1000000 + (view + i) as u64 * 10,
                            )),
                        },
                    },
                    author: Identity(voter),
                    signature: hints::PartialSignature::default(),
                });
                let vote_ref = bulk_store.append_vote(vote.clone()).unwrap();
                cache.insert_vote(vote, vote_ref);
            }
        }

        // Phase 3: Save snapshot every 5 views
        if round % 5 == 0 {
            let state = ConsensusState {
                current_view: ViewNum(view + 4),
                current_phase: Phase::Low,
                view_entry_time: (view + 4) as u128 * 1000,
                tips: vec![QCRef {
                    vote_data: VoteData {
                        z: 2,
                        for_which: BlockKey {
                            type_: BlockType::Tr,
                            view: ViewNum(view + 4),
                            height: (view + 4) as u64 * 10,
                            author: Some(Identity(1)),
                            slot: SlotNum((view + 4) as u64 * 10),
                            hash: Some(BlockHash((view + 4) as u64 * 1000000)),
                        },
                    },
                    hash: None,
                }],
                max_1qc: QCRef {
                    vote_data: VoteData {
                        z: 1,
                        for_which: GEN_BLOCK_KEY,
                    },
                    hash: None,
                },
                finalized_blocks: im::HashSet::new(),
                unfinalized_qcs: im::HashMap::new(),
                leader_blocks_by_view: im::HashMap::new(),
                unfinalized_leader_by_view: im::HashMap::new(),
            };

            snapshot_store.save_snapshot(&state).unwrap();
        }
    }

    // Verify final state
    let final_view = 19 * 5 + 4;

    // Check recent views are in cache
    for view in final_view - 10..=final_view {
        let blocks_in_view = bulk_store.get_blocks_in_view(ViewNum(view)).unwrap();
        for block_ref in blocks_in_view {
            let cached = cache.get_block(&block_ref.key);
            // Only the current view should be cached
            if view == final_view {
                assert!(cached.is_some(), "Current view block should be cached");
            } else {
                assert!(cached.is_none(), "Old view blocks should not be cached");
            }
        }
    }

    // Verify all data is in bulk storage
    for view in 0..100 {
        let blocks = bulk_store.get_blocks_in_view(ViewNum(view)).unwrap();
        let qcs = bulk_store.get_qcs_in_view(ViewNum(view)).unwrap();

        // Each view should have blocks and QCs
        if view < final_view {
            assert!(!blocks.is_empty() || !qcs.is_empty());
        }
    }
}

#[test]
fn test_large_block_handling() {
    let db = create_test_db();
    let mut store = RedbBulkStore::new(db.clone()).unwrap();

    // Create a block with many large transactions
    let mut transactions = Vec::new();
    for i in 0..1000 {
        transactions.push(TestTransaction {
            id: i,
            data: vec![i as u8; 1000], // 1KB per transaction
        });
    }

    let block_key = BlockKey {
        type_: BlockType::Tr,
        view: ViewNum(1),
        height: 1,
        author: Some(Identity(1)),
        slot: SlotNum(1),
        hash: Some(BlockHash(1)),
    };

    let genesis_qc = Arc::new(ThreshSigned {
        data: VoteData {
            z: 1,
            for_which: GEN_BLOCK_KEY,
        },
        signature: hints::Signature::default(),
    });

    let large_block = Arc::new(Signed {
        data: Block {
            key: block_key.clone(),
            prev: vec![genesis_qc.clone()],
            one: genesis_qc,
            data: BlockData::Tr { transactions },
        },
        author: Identity(1),
        signature: hints::PartialSignature::default(),
    });

    // Store and retrieve the large block
    let start = Instant::now();
    let block_ref = store.append_block(large_block.clone()).unwrap();
    let store_time = start.elapsed();
    println!("Stored 1MB block in {:?}", store_time);

    let start = Instant::now();
    let retrieved = store.get_block(&block_ref).unwrap();
    let retrieve_time = start.elapsed();
    println!("Retrieved 1MB block in {:?}", retrieve_time);

    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().data.key, block_key);
}

#[test]
fn test_memory_efficiency() {
    // Test that the storage system efficiently manages memory
    let db = create_test_db();
    let mut bulk_store = RedbBulkStore::new(db.clone()).unwrap();
    let mut cache = ViewCache::new(ViewNum(0));

    // Generate and store many blocks
    for view in 0..50 {
        cache.transition_to_view(ViewNum(view));

        for i in 0..20 {
            let block = generate_block(view, (view * 20 + i) as usize, ((i % 4) + 1) as u32, 50);
            let block_ref = bulk_store.append_block(block.clone()).unwrap();

            // Only cache if in current view
            if view == 49 {
                cache.insert_block(block, block_ref);
            }
        }
    }

    // Verify that cache only contains current view
    let cached_blocks: &im::HashMap<BlockKey, Arc<Signed<Block<TestTransaction>>>> = cache.blocks();
    for (key, _) in cached_blocks {
        assert_eq!(
            key.view,
            ViewNum(49),
            "Cache should only contain current view"
        );
    }

    // Verify all blocks are in bulk storage
    for view in 0..50 {
        let blocks: Vec<BlockRef> = bulk_store.get_blocks_in_view(ViewNum(view)).unwrap();
        assert_eq!(blocks.len(), 20);
    }
}
