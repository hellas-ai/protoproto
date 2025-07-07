//! Comprehensive tests for the storage subsystem
//!
//! This module tests the storage subsystem for correctness, consistency,
//! and proper handling of various edge cases.

mod common;

use hellas_morpheus::test_harness::TestTransaction;
use hellas_morpheus::*;
use redb::Database;
use std::sync::Arc;

/// Create a test database
fn create_test_db() -> Arc<Database> {
    Arc::new(
        redb::Builder::new()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap(),
    )
}

/// Create a test QC
fn create_test_qc(block_key: BlockKey, z: u8) -> FinishedQC {
    Arc::new(ThreshSigned {
        data: VoteData {
            z,
            for_which: block_key,
        },
        signature: hints::Signature::default(),
    })
}

/// Create a test vote
fn create_test_vote(voter: Identity, vote_data: VoteData) -> Arc<ThreshPartial<VoteData>> {
    Arc::new(ThreshPartial {
        data: vote_data,
        author: voter,
        signature: hints::PartialSignature::default(),
    })
}

#[test]
fn test_bulk_store_basic_operations() {
    let db = create_test_db();
    let mut store = RedbBulkStore::new(db.clone()).unwrap();

    // Test block storage
    let block = common::create_test_block(1, 1, 1);
    let block_ref = store.append_block(block.clone()).unwrap();

    assert_eq!(block_ref.key, block.data.key);
    assert_eq!(block_ref.hash, block.data.key.hash);

    // Test block retrieval
    let retrieved_block: Arc<Signed<Block<TestTransaction>>> =
        store.get_block(&block_ref).unwrap().unwrap();
    assert_eq!(retrieved_block.data.key, block.data.key);

    // Test QC storage
    let qc = create_test_qc(block.data.key.clone(), 1);
    let qc_ref: QCRef = store.append_qc(qc.clone()).unwrap();

    assert_eq!(qc_ref.vote_data, qc.data);

    // Test QC retrieval
    let retrieved_qc = store.get_qc(&qc_ref).unwrap().unwrap();
    assert_eq!(retrieved_qc.data, qc.data);

    // Test vote storage
    let vote = create_test_vote(Identity(1), qc.data.clone());
    let vote_ref = store.append_vote(vote.clone()).unwrap();

    assert_eq!(vote_ref.voter, vote.author);
    assert_eq!(vote_ref.vote_data, vote.data);

    // Test vote retrieval
    let retrieved_vote = store.get_vote(&vote_ref).unwrap().unwrap();
    assert_eq!(retrieved_vote.data, vote.data);
}

#[test]
fn test_view_indexing() {
    let db = create_test_db();
    let mut store = RedbBulkStore::new(db.clone()).unwrap();

    // Add multiple blocks in the same view
    let view = ViewNum(5);
    let block1 = common::create_test_block(5, 1, 1);
    let block2 = common::create_test_block(5, 2, 2);
    let block3 = common::create_test_block(5, 3, 3);

    let ref1 = store.append_block(block1.clone()).unwrap();
    let ref2 = store.append_block(block2.clone()).unwrap();
    let ref3 = store.append_block(block3.clone()).unwrap();

    // Check view index
    let blocks_in_view: Vec<BlockRef> = store.get_blocks_in_view(view).unwrap();
    assert_eq!(blocks_in_view.len(), 3);
    assert!(blocks_in_view.contains(&ref1));
    assert!(blocks_in_view.contains(&ref2));
    assert!(blocks_in_view.contains(&ref3));

    // Add QCs in the same view
    let qc1 = create_test_qc(block1.data.key.clone(), 1);
    let qc2 = create_test_qc(block2.data.key.clone(), 1);

    let qc_ref1 = store.append_qc(qc1.clone()).unwrap();
    let qc_ref2 = store.append_qc(qc2.clone()).unwrap();

    // Check QC view index
    let qcs_in_view = store.get_qcs_in_view(view).unwrap();
    assert_eq!(qcs_in_view.len(), 2);
    assert!(qcs_in_view.contains(&qc_ref1));
    assert!(qcs_in_view.contains(&qc_ref2));
}

#[test]
fn test_snapshot_store_basic_operations() {
    let db = create_test_db();
    let mut store = RedbSnapshotStore::new(db.clone()).unwrap();

    // Create a mock consensus state
    let state = ConsensusState {
        current_view: ViewNum(10),
        current_phase: Phase::High,
        view_entry_time: 1000,
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

    // Save snapshot
    let root = store.save_snapshot(&state).unwrap();

    // Load snapshot using the root
    let loaded_state = store.load_snapshot(&root).unwrap().unwrap();
    assert_eq!(loaded_state.current_view, state.current_view);
    assert_eq!(loaded_state.current_phase, state.current_phase);
    assert_eq!(loaded_state.view_entry_time, state.view_entry_time);
}

#[test]
fn test_view_cache_operations() {
    let db = create_test_db();
    let bulk_store = Arc::new(RedbBulkStore::<TestTransaction>::new(db.clone()).unwrap());
    let mut cache = ViewCache::new(ViewNum(1));

    // Test block caching
    let block = common::create_test_block(1, 1, 1);
    let block_ref = BlockRef {
        key: block.data.key.clone(),
        hash: block.data.key.hash.clone(),
    };
    cache.insert_block(block.clone(), block_ref);

    let cached_block = cache.get_block(&block.data.key).unwrap();
    assert_eq!(cached_block.data.key, block.data.key);

    // Test QC caching
    let qc = create_test_qc(block.data.key.clone(), 1);
    let qc_ref = QCRef {
        vote_data: qc.data.clone(),
        hash: None,
    };
    cache.insert_qc(qc.clone(), qc_ref);

    let cached_qc = cache.get_qc(&qc.data).unwrap();
    assert_eq!(cached_qc.data, qc.data);

    // Test vote caching
    let vote = create_test_vote(Identity(1), qc.data.clone());
    let vote_ref = VoteRef {
        voter: vote.author.clone(),
        vote_data: vote.data.clone(),
        hash: None,
    };
    cache.insert_vote(vote.clone(), vote_ref);

    let cached_vote = cache.get_vote(&vote.author, &vote.data).unwrap();
    assert_eq!(cached_vote.data, vote.data);

    // Test view transition
    cache.transition_to_view(ViewNum(2));

    // After transition to view 2, our view 1 items should be gone
    assert!(cache.get_block(&block.data.key).is_none());
    assert!(cache.get_qc(&qc.data).is_none());
}

#[test]
fn test_lightweight_dag_index() {
    let db = create_test_db();
    let mut bulk_store = RedbBulkStore::new(db.clone()).unwrap();
    let mut dag = LightweightDAGIndex::new();

    // Add blocks and update DAG
    let block1 = common::create_test_block(1, 1, 1);
    let block2 = common::create_test_block(2, 2, 2);
    let block3 = common::create_test_block(3, 3, 3);

    let ref1 = bulk_store.append_block(block1.clone()).unwrap();
    let ref2 = bulk_store.append_block(block2.clone()).unwrap();
    let ref3 = bulk_store.append_block(block3.clone()).unwrap();

    dag.insert_block_ref(ref1.clone());
    dag.insert_block_ref(ref2.clone());
    dag.insert_block_ref(ref3.clone());

    // Update relationships (we need to create proper prev references)
    dag.block_points_to.insert(
        ref1.key.clone(),
        im::HashSet::from_iter(vec![ref2.key.clone()]),
    );
    dag.block_pointed_by.insert(
        ref2.key.clone(),
        im::HashSet::from_iter(vec![ref1.key.clone()]),
    );
    dag.block_points_to.insert(
        ref2.key.clone(),
        im::HashSet::from_iter(vec![ref3.key.clone()]),
    );
    dag.block_pointed_by.insert(
        ref3.key.clone(),
        im::HashSet::from_iter(vec![ref2.key.clone()]),
    );

    // Test height tracking
    assert_eq!(dag.max_height, 3);

    // Test parent-child relationships
    assert!(dag
        .block_points_to
        .get(&ref1.key)
        .unwrap()
        .contains(&ref2.key));
    assert!(dag
        .block_points_to
        .get(&ref2.key)
        .unwrap()
        .contains(&ref3.key));
}

#[test]
fn test_storage_consistency() {
    let db = create_test_db();
    let mut bulk_store = RedbBulkStore::new(db.clone()).unwrap();
    let mut snapshot_store = RedbSnapshotStore::new(db.clone()).unwrap();

    // Build up some state
    let mut blocks = vec![];
    let mut qcs = vec![];

    for i in 0..10 {
        let block = common::create_test_block(i, i as usize, (i % 4 + 1) as u32);
        let block_ref = bulk_store.append_block(block.clone()).unwrap();
        blocks.push((block_ref, block.clone()));

        let qc = create_test_qc(block.data.key.clone(), 1);
        let qc_ref = bulk_store.append_qc(qc.clone()).unwrap();
        qcs.push((qc_ref, qc));
    }

    // Create and save a snapshot
    let state = ConsensusState {
        current_view: ViewNum(9),
        current_phase: Phase::Low,
        view_entry_time: 5000,
        tips: vec![qcs.last().unwrap().0.clone()],
        max_1qc: qcs[5].0.clone(),
        finalized_blocks: blocks.iter().skip(7).map(|(r, _)| r.clone()).collect(),
        unfinalized_qcs: im::HashMap::new(),
        leader_blocks_by_view: im::HashMap::new(),
        unfinalized_leader_by_view: im::HashMap::new(),
    };

    let root = snapshot_store.save_snapshot(&state).unwrap();

    // Verify we can reconstruct the state
    let loaded_state = snapshot_store.load_snapshot(&root).unwrap().unwrap();

    // Check consistency
    assert_eq!(loaded_state.current_view, ViewNum(9));
    assert_eq!(loaded_state.tips.len(), 1);
    assert_eq!(loaded_state.finalized_blocks.len(), 3);

    // Verify we can retrieve all referenced blocks and QCs
    for block_ref in &loaded_state.finalized_blocks {
        let block = bulk_store.get_block(&block_ref).unwrap();
        assert!(block.is_some());
        assert_eq!(block.unwrap().data.key, block_ref.key);
    }

    // Verify QC retrieval
    let tip_qc = bulk_store.get_qc(&loaded_state.tips[0]).unwrap();
    assert!(tip_qc.is_some());
}

#[test]
fn test_storage_error_handling() {
    let db = create_test_db();
    let store = RedbBulkStore::<TestTransaction>::new(db.clone()).unwrap();

    // Test retrieval of non-existent items
    let fake_block_ref = BlockRef {
        key: BlockKey {
            type_: BlockType::Tr,
            view: ViewNum(999),
            height: 999,
            author: Some(Identity(999)),
            slot: SlotNum(999),
            hash: Some(BlockHash(999)),
        },
        hash: Some(BlockHash(999)),
    };

    let result = store.get_block(&fake_block_ref).unwrap();
    assert!(result.is_none());

    let fake_qc_ref = QCRef {
        vote_data: VoteData {
            z: 1,
            for_which: fake_block_ref.key.clone(),
        },
        hash: None,
    };

    let result = store.get_qc(&fake_qc_ref).unwrap();
    assert!(result.is_none());

    let fake_vote_ref = VoteRef {
        voter: Identity(999),
        vote_data: fake_qc_ref.vote_data.clone(),
        hash: None,
    };

    let result = store.get_vote(&fake_vote_ref).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_storage_idempotency() {
    let db = create_test_db();
    let mut store = RedbBulkStore::new(db.clone()).unwrap();

    // Test that appending the same block multiple times is idempotent
    let block = common::create_test_block(1, 1, 1);

    let ref1 = store.append_block(block.clone()).unwrap();
    let ref2 = store.append_block(block.clone()).unwrap();

    assert_eq!(ref1, ref2);

    // Verify view index doesn't have duplicates
    let blocks_in_view: Vec<BlockRef> = store.get_blocks_in_view(ViewNum(1)).unwrap();
    assert_eq!(blocks_in_view.len(), 1);

    // Test the same for QCs
    let qc = create_test_qc(block.data.key.clone(), 1);

    let qc_ref1 = store.append_qc(qc.clone()).unwrap();
    let qc_ref2 = store.append_qc(qc.clone()).unwrap();

    assert_eq!(qc_ref1, qc_ref2);

    let qcs_in_view = store.get_qcs_in_view(ViewNum(1)).unwrap();
    assert_eq!(qcs_in_view.len(), 1);
}

#[test]
fn test_snapshot_pruning() {
    let db = create_test_db();
    let mut store = RedbSnapshotStore::new(db.clone()).unwrap();

    // Create multiple snapshots
    for i in 0..10 {
        let state = ConsensusState {
            current_view: ViewNum(i),
            current_phase: Phase::High,
            view_entry_time: i as u128 * 100,
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

        store.save_snapshot(&state).unwrap();
    }

    // Prune old snapshots
    store.prune_snapshots(3).unwrap();

    // Verify that we have at most 3 snapshots - need to check based on actual implementation
}
