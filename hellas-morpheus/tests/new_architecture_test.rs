//! Test demonstrating the new storage architecture

use ark_std::test_rng;
use hellas_morpheus::storage::{
    bulk::RedbBulkStore, snapshot::RedbSnapshotStore, InvariantCheckConfig,
};
use hellas_morpheus::test_harness::{MockHarness, TestTransaction};
use hellas_morpheus::*;
use std::collections::BTreeMap;
use std::sync::Arc;

#[test]
fn test_new_architecture_basic() {
    // Create test database
    let db = Arc::new(
        redb::Builder::new()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap(),
    );

    // Create storage factories for test harness
    let db_clone = db.clone();
    let create_bulk = move |_db: &redb::Database| RedbBulkStore::new(db_clone.clone()).unwrap();
    let db_clone = db.clone();
    let create_snapshot =
        move |_db: &redb::Database| RedbSnapshotStore::new(db_clone.clone()).unwrap();

    // Use paranoid mode for testing
    let invariant_config = Some(InvariantCheckConfig::paranoid());

    // Create test harness with 3 nodes
    let mut harness = MockHarness::<_, _>::create_test_setup(
        3,
        create_bulk,
        create_snapshot,
        invariant_config.clone(),
    );

    // Run the harness for 10 steps
    for i in 0..10 {
        println!("Step {}", i);
        harness.step();
    }

    // Verify all processes have the same view
    let process1 = harness.processes.get(&Identity(1)).unwrap();
    let process2 = harness.processes.get(&Identity(2)).unwrap();
    let process3 = harness.processes.get(&Identity(3)).unwrap();

    assert_eq!(
        process1.view_manager.current_view(),
        process2.view_manager.current_view()
    );
    assert_eq!(
        process2.view_manager.current_view(),
        process3.view_manager.current_view()
    );

    println!(
        "All processes in view: {:?}",
        process1.view_manager.current_view()
    );
}

#[test]
fn test_storage_isolation() {
    // Each process gets its own storage
    let db1 = Arc::new(
        redb::Builder::new()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap(),
    );
    let db2 = Arc::new(
        redb::Builder::new()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap(),
    );

    // Create storage for process 1
    let bulk_store1 = RedbBulkStore::new(db1.clone()).unwrap();
    let snapshot_store1 = RedbSnapshotStore::new(db1.clone()).unwrap();

    // Create storage for process 2
    let bulk_store2 = RedbBulkStore::new(db2.clone()).unwrap();
    let snapshot_store2 = RedbSnapshotStore::new(db2.clone()).unwrap();

    // Setup crypto similar to test harness
    let n = 2;
    let f = 0;
    let domain_max = (1 + n as usize).next_power_of_two();
    let gd = hints::GlobalData::new(domain_max, &mut test_rng()).unwrap();
    let privs = vec![hints::SecretKey::random(&mut test_rng()); domain_max - 1];
    let pubkeys: Vec<hints::PublicKey> = privs.iter().map(|sk| sk.public(&gd)).collect();
    let weights = vec![hints::F::from(1); domain_max - 1];

    let hints_data = (0..domain_max - 1)
        .map(|i| hints::generate_hint(&gd, &privs[i], domain_max, i).unwrap())
        .collect::<Vec<_>>();

    let setup = hints::setup_universe(&gd, pubkeys.clone(), &hints_data, weights).unwrap();

    let keys: BTreeMap<Identity, hints::PublicKey> = (0..n as usize)
        .map(|i| (Identity(i as u32 + 1), pubkeys[i].clone()))
        .collect();

    let identities: BTreeMap<hints::PublicKey, Identity> = (0..n as usize)
        .map(|i| (pubkeys[i].clone(), Identity(i as u32 + 1)))
        .collect();

    // Create processes
    let process1: MorpheusProcess<TestTransaction, _, _> = MorpheusProcess::new(
        &db1,
        KeyBook {
            keys: keys.clone(),
            identities: identities.clone(),
            me_identity: Identity(1),
            me_pub_key: pubkeys[0].clone(),
            me_sec_key: privs[0].clone(),
            hints_setup: Some(setup.clone()),
        },
        Identity(1),
        n,
        f,
        bulk_store1,
        snapshot_store1,
        Some(InvariantCheckConfig::debug()),
    )
    .unwrap();

    let process2: MorpheusProcess<TestTransaction, _, _> = MorpheusProcess::new(
        &db2,
        KeyBook {
            keys: keys.clone(),
            identities: identities.clone(),
            me_identity: Identity(2),
            me_pub_key: pubkeys[1].clone(),
            me_sec_key: privs[1].clone(),
            hints_setup: Some(setup.clone()),
        },
        Identity(2),
        n,
        f,
        bulk_store2,
        snapshot_store2,
        Some(InvariantCheckConfig::debug()),
    )
    .unwrap();

    // Verify they start in the same state
    assert_eq!(process1.view_manager.current_view(), ViewNum(0));
    assert_eq!(process2.view_manager.current_view(), ViewNum(0));

    // Each has their own storage
    assert_eq!(process1.finalized_blocks.len(), 0);
    assert_eq!(process2.finalized_blocks.len(), 0);
}

#[test]
fn test_snapshot_and_restore() {
    let db = Arc::new(
        redb::Builder::new()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap(),
    );

    // Create storage
    let bulk_store = RedbBulkStore::new(db.clone()).unwrap();
    let snapshot_store = RedbSnapshotStore::new(db.clone()).unwrap();

    // Setup crypto
    let n = 1;
    let f = 0;
    let domain_max = (1 + n as usize).next_power_of_two();
    let gd = hints::GlobalData::new(domain_max, &mut test_rng()).unwrap();
    let privs = vec![hints::SecretKey::random(&mut test_rng()); domain_max - 1];
    let pubkeys: Vec<hints::PublicKey> = privs.iter().map(|sk| sk.public(&gd)).collect();
    let weights = vec![hints::F::from(1); domain_max - 1];

    let hints_data = (0..domain_max - 1)
        .map(|i| hints::generate_hint(&gd, &privs[i], domain_max, i).unwrap())
        .collect::<Vec<_>>();

    let setup = hints::setup_universe(&gd, pubkeys.clone(), &hints_data, weights).unwrap();

    let keys: BTreeMap<Identity, hints::PublicKey> = (0..n as usize)
        .map(|i| (Identity(i as u32 + 1), pubkeys[i].clone()))
        .collect();

    let identities: BTreeMap<hints::PublicKey, Identity> = (0..n as usize)
        .map(|i| (pubkeys[i].clone(), Identity(i as u32 + 1)))
        .collect();

    // Create process
    let mut process: MorpheusProcess<TestTransaction, _, _> = MorpheusProcess::new(
        &db,
        KeyBook {
            keys: keys.clone(),
            identities: identities.clone(),
            me_identity: Identity(1),
            me_pub_key: pubkeys[0].clone(),
            me_sec_key: privs[0].clone(),
            hints_setup: Some(setup.clone()),
        },
        Identity(1),
        n,
        f,
        bulk_store,
        snapshot_store,
        None,
    )
    .unwrap();

    // Set time and produce some blocks
    process.set_now(&db, 100).unwrap();
    process
        .set_ready_transactions(
            &db,
            vec![TestTransaction {
                id: 0,
                data: vec![1, 2, 3],
            }],
        )
        .unwrap();
    process.try_produce_blocks(&db).unwrap();

    // Save snapshot
    process.save_snapshot().unwrap();

    // Create a snapshot of the process state
    let snapshot = process.to_snapshot();

    // Create new storage for restored process
    let bulk_store_new = RedbBulkStore::new(db.clone()).unwrap();
    let snapshot_store_new = RedbSnapshotStore::new(db.clone()).unwrap();

    // Create a new process from the snapshot
    let restored_process =
        MorpheusProcess::from_snapshot(snapshot, bulk_store_new, snapshot_store_new, None);

    // Verify state was restored
    assert_eq!(
        process.view_manager.current_view(),
        restored_process.view_manager.current_view()
    );
    assert_eq!(
        process.finalized_blocks.len(),
        restored_process.finalized_blocks.len()
    );
    assert_eq!(process.tips_refs.len(), restored_process.tips_refs.len());
}

#[test]
fn test_invariant_checking() {
    let db = Arc::new(
        redb::Builder::new()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap(),
    );

    // Create storage
    let bulk_store = RedbBulkStore::new(db.clone()).unwrap();
    let snapshot_store = RedbSnapshotStore::new(db.clone()).unwrap();

    // Setup crypto
    let n = 3;
    let f = 1;
    let domain_max = (1 + n as usize).next_power_of_two();
    let gd = hints::GlobalData::new(domain_max, &mut test_rng()).unwrap();
    let privs = vec![hints::SecretKey::random(&mut test_rng()); domain_max - 1];
    let pubkeys: Vec<hints::PublicKey> = privs.iter().map(|sk| sk.public(&gd)).collect();
    let weights = vec![hints::F::from(1); domain_max - 1];

    let hints_data = (0..domain_max - 1)
        .map(|i| hints::generate_hint(&gd, &privs[i], domain_max, i).unwrap())
        .collect::<Vec<_>>();

    let setup = hints::setup_universe(&gd, pubkeys.clone(), &hints_data, weights).unwrap();

    let keys: BTreeMap<Identity, hints::PublicKey> = (0..n as usize)
        .map(|i| (Identity(i as u32 + 1), pubkeys[i].clone()))
        .collect();

    let identities: BTreeMap<hints::PublicKey, Identity> = (0..n as usize)
        .map(|i| (pubkeys[i].clone(), Identity(i as u32 + 1)))
        .collect();

    // Create process with paranoid invariant checking
    let mut process: MorpheusProcess<TestTransaction, _, _> = MorpheusProcess::new(
        &db,
        KeyBook {
            keys: keys.clone(),
            identities: identities.clone(),
            me_identity: Identity(1),
            me_pub_key: pubkeys[0].clone(),
            me_sec_key: privs[0].clone(),
            hints_setup: Some(setup.clone()),
        },
        Identity(1),
        n,
        f,
        bulk_store,
        snapshot_store,
        Some(InvariantCheckConfig::paranoid()),
    )
    .unwrap();

    // Run some operations - invariants will be checked after each
    process.set_now(&db, 100).unwrap();
    process.check_timeouts(&db).unwrap();
    process.try_produce_blocks(&db).unwrap();

    // If we get here, all invariants passed
    println!("All invariants checked successfully!");
}
