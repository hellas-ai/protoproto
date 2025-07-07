//! Common test utilities and helpers
//!
//! This module provides shared test infrastructure including:
//! - Mock transaction types
//! - Test database creation helpers
//! - Test block/QC generation utilities
//! - Common test setup functions

#![allow(dead_code)]

use ark_std::test_rng;
use hellas_morpheus::test_harness::TestTransaction;
use hellas_morpheus::*;
use redb::Database;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Create a test database with in-memory backend
pub fn create_test_db() -> Arc<Database> {
    Arc::new(
        redb::Builder::new()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap(),
    )
}

/// Create a test block with specified parameters
pub fn create_test_block(
    view: i64,
    height: usize,
    author: u32,
) -> Arc<Signed<Block<TestTransaction>>> {
    let block_key = BlockKey {
        type_: BlockType::Tr,
        view: ViewNum(view),
        height: height as u64,
        author: Some(Identity(author)),
        slot: SlotNum(height as u64),
        hash: Some(BlockHash(view as u64 * 1000 + height as u64)),
    };

    let genesis_qc = Arc::new(ThreshSigned {
        data: VoteData {
            z: 1,
            for_which: GEN_BLOCK_KEY,
        },
        signature: hints::Signature::default(),
    });

    let transactions: Vec<TestTransaction> = (0..5)
        .map(|i| TestTransaction {
            id: i,
            data: format!("tx_{}", i).into_bytes(),
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

/// Create a test QC for a block
pub fn create_test_qc(block_key: BlockKey, z: u8) -> FinishedQC {
    Arc::new(ThreshSigned {
        data: VoteData {
            z,
            for_which: block_key,
        },
        signature: hints::Signature::default(),
    })
}

/// Create a test vote
pub fn create_test_vote(voter: Identity, vote_data: VoteData) -> Arc<ThreshPartial<VoteData>> {
    Arc::new(ThreshPartial {
        data: vote_data,
        author: voter,
        signature: hints::PartialSignature::default(),
    })
}

/// Setup crypto for testing with n processes
pub fn setup_test_crypto(n: u32) -> (KeyBook, Vec<hints::SecretKey>, Vec<hints::PublicKey>) {
    let domain_max = (1 + n as usize).next_power_of_two();
    let gd = hints::GlobalData::new(domain_max, &mut test_rng()).unwrap();
    let privs = vec![hints::SecretKey::random(&mut test_rng()); domain_max - 1];
    let pubkeys: Vec<hints::PublicKey> = privs.iter().map(|sk| sk.public(&gd)).collect();
    let weights = vec![hints::F::from(1); domain_max - 1];

    let hints_data = (0..domain_max - 1)
        .map(|i| hints::generate_hint(&gd, &privs[i], domain_max, i).unwrap())
        .collect::<Vec<_>>();

    let setup = hints::setup_universe(&gd, pubkeys.clone(), &hints_data, weights).unwrap();

    let keys: BTreeMap<Identity, hints::PublicKey> = (0..n)
        .map(|i| (Identity(i as u32 + 1), pubkeys[i as usize].clone()))
        .collect();

    let identities: BTreeMap<hints::PublicKey, Identity> = (0..n)
        .map(|i| (pubkeys[i as usize].clone(), Identity(i as u32 + 1)))
        .collect();

    let kb = KeyBook {
        keys: keys.clone(),
        identities: identities.clone(),
        me_identity: Identity(1),
        me_pub_key: pubkeys[0].clone(),
        me_sec_key: privs[0].clone(),
        hints_setup: Some(setup.clone()),
    };

    (kb, privs, pubkeys)
}

/// Create a test process with given identity
pub fn create_test_process_with_id(
    id: Identity,
) -> MorpheusProcess<
    TestTransaction,
    storage::bulk::RedbBulkStore<TestTransaction>,
    storage::snapshot::RedbSnapshotStore,
> {
    let db = create_test_db();
    let (mut kb, privs, pubkeys) = setup_test_crypto(1); // Assuming n=1 for simplicity in this test

    // Update keybook for this specific identity
    kb.me_identity = id.clone();
    kb.me_pub_key = pubkeys[(id.0 as usize) - 1].clone();
    kb.me_sec_key = privs[(id.0 as usize) - 1].clone();

    let bulk_store = storage::bulk::RedbBulkStore::new(db.clone()).unwrap();
    let snapshot_store = storage::snapshot::RedbSnapshotStore::new(db.clone()).unwrap();

    MorpheusProcess::new(&db, kb, id, 1, 0, bulk_store, snapshot_store, None).unwrap()
}

/// Create a simple test message
pub fn create_test_end_view_message(view: ViewNum, kb: &KeyBook) -> Message<TestTransaction> {
    Message::EndView(Arc::new(ThreshPartial::from_data(view, kb)))
}
