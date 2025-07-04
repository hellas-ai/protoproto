use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::state_tracking::{PendingVotes, StateIndex};
use crate::*;
use fastbloom::BloomFilter;
use redb::{ReadableTableMetadata, TableDefinition};
use serde::{Deserialize, Serialize};

/// MorpheusProcess represents a single process (p_i) in the Morpheus protocol
///
/// This struct implements the Algorithm 1 from the Morpheus pseudocode,
/// maintaining all state required for processing messages, voting, and
/// producing blocks according to the protocol specification.
#[derive(Clone, derive_more::Debug, Serialize, Deserialize, derivative::Derivative)]
#[derivative(PartialEq)]
pub struct MorpheusProcess<Tr: Transaction> {
    #[debug(skip)]
    pub kb: KeyBook,

    #[derivative(PartialEq = "ignore")]
    pub replaying: bool,

    /// Identity of this process (equivalent to p_i in the pseudocode)
    pub id: Identity,

    /// Current view number
    ///
    /// "Initially 0, represents the present view"
    pub view_i: ViewNum,

    /// Current slot for leader blocks
    ///
    /// "Initially 0, represents present slot" for leader blocks
    pub slot_i_lead: SlotNum,

    /// Current slot for transaction blocks
    ///
    /// "Initially 0, represents present slot" for transaction blocks
    pub slot_i_tr: SlotNum,

    /// Tracks which blocks this process has already voted for (voted_i(z,x,s,p_j) in pseudocode)
    /// "Initially 0" for all combinations of z, x, s, p_j
    /// Used to ensure process votes only once for each (z,x,s,p_j) combination
    pub voted_i: BTreeSet<(u8, BlockType, SlotNum, Identity)>,

    /// Tracks the phase within each view (phase_i(v) in pseudocode)
    /// "Initially 0" for each view, represents high throughput (0) or low throughput (1) phase
    //#[serde(with = "serde_json_any_key::any_key_map")]
    pub phase_i: BTreeMap<ViewNum, Phase>,

    /// Total number of processes in the system
    pub n: u32,

    /// Maximum number of faulty processes tolerated (n-f is the quorum size)
    pub f: u32,

    /// Network delay parameter (Δ in pseudocode)
    /// Used for timeouts in the protocol (6Δ and 12Δ)
    pub delta: u128,

    /// Tracks end-view messages for view changes
    /// Used to form (v+1)-certificates when f+1 end-view v messages are collected
    pub end_views: QuorumTrack<ViewNum>,

    /// Tracks which 0-QCs have been sent to avoid duplicates
    /// Implements "p_i has not previously sent a 0-QC for b to other processors"
    pub zero_qcs_sent: BTreeSet<BlockKey>,

    /// Tracks which QCs we've already complained about to the leader
    /// Implements "Send q to lead(view_i) if not previously sent"
    pub complained_qcs: BTreeSet<FinishedQC>,

    /// Time when this process entered the current view
    /// Used for timeout calculations (6Δ and 12Δ since entering view)
    pub view_entry_time: u128,

    /// Current logical time
    pub current_time: u128,

    // === State tracking fields (corresponding to M_i and Q_i in pseudocode) ===
    /// Tracks votes for each VoteData to form quorums
    /// Part of M_i in pseudocode - "the set of all received messages"
    pub vote_tracker: QuorumTrack<VoteData>,

    /// Tracks view change messages
    /// Used to collect view v messages with 1-QCs sent to the leader
    //#[serde(with = "serde_json_any_key::any_key_map")]
    pub start_views: BTreeMap<ViewNum, Vec<Arc<Signed<StartView>>>>,

    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub index: StateIndex<Tr>,

    /// Tracks whether we've produced a leader block in each view
    /// Used for leader logic to avoid producing multiple leader blocks in same view
    //#[serde(with = "serde_json_any_key::any_key_map")]
    pub produced_lead_in_view: BTreeMap<ViewNum, bool>,

    /// All messages received by this process
    pub qcs: BTreeSet<FinishedQC>,
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub genesis: Arc<Signed<Block<Tr>>>,
    pub genesis_qc: FinishedQC,

    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    #[serde(with = "ark_serialize::vec_compressed_checked")]
    pub ready_transactions: Vec<Tr>,

    pub pending_votes: BTreeMap<ViewNum, PendingVotes>,
    #[debug(skip)]
    pub seen_messages: BloomFilter,

    pub recorded_events: u64,

    #[debug(skip)]
    #[serde(default = "recorded_events_table_default")]
    #[serde(skip)]
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    #[derivative(PartialEq = "ignore")]
    pub recorded_events_table: Option<TableDefinition<'static, u64, Postcard<Event<Tr>>>>,

    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    #[debug(skip)]
    #[serde(default = "snapshots_table_default")]
    #[serde(skip)]
    #[derivative(PartialEq = "ignore")]
    pub snapshots_table: Option<TableDefinition<'static, u64, Postcard<MorpheusProcess<Tr>>>>,
}

pub fn recorded_events_table_default<Tr: Transaction>()
-> Option<TableDefinition<'static, u64, Postcard<Event<Tr>>>> {
    Some(TableDefinition::new("recorded_events"))
}

pub fn snapshots_table_default<Tr: Transaction>()
-> Option<TableDefinition<'static, u64, Postcard<MorpheusProcess<Tr>>>> {
    Some(TableDefinition::new("snapshots"))
}

pub const SEEN_MESSAGE_HASHES_TABLE: TableDefinition<'static, [u8; 32], ()> =
    TableDefinition::new("seen_message_hashes");

impl<Tr: Transaction> MorpheusProcess<Tr> {
    pub fn new(_db: &redb::Database, keybook: KeyBook, id: Identity, n: u32, f: u32) -> Self {
        crate::tracing_setup::register_process(&id, n, f);

        let recorded_events_table = recorded_events_table_default::<Tr>().unwrap();
        let tx = _db.begin_write().unwrap();
        {
            // other code wants to assume this table exists
            tx.open_table(recorded_events_table).unwrap();
        }
        tx.commit().unwrap();

        let genesis_block = Arc::new(Signed {
            data: Block {
                key: GEN_BLOCK_KEY,
                prev: Vec::new(),
                one: Arc::new(ThreshSigned {
                    data: VoteData {
                        z: 1,
                        for_which: GEN_BLOCK_KEY,
                    },
                    signature: hints::Signature::default(),
                }),
                data: BlockData::Genesis,
            },
            author: Identity(u32::MAX),
            signature: hints::PartialSignature::default(),
        });

        let genesis_qc = Arc::new(ThreshSigned {
            data: VoteData {
                z: 1,
                for_which: GEN_BLOCK_KEY,
            },
            signature: hints::Signature::default(),
        });

        let mut p = MorpheusProcess {
            kb: keybook,
            replaying: false,
            id,
            view_i: ViewNum(0),
            slot_i_lead: SlotNum(0),
            slot_i_tr: SlotNum(0),
            voted_i: BTreeSet::new(),
            phase_i: {
                let mut map = BTreeMap::new();
                map.insert(ViewNum(0), Phase::High);
                map
            },
            n,
            f,
            delta: 10, // 10 ... "units"

            end_views: QuorumTrack {
                votes: BTreeMap::new(),
            },
            zero_qcs_sent: BTreeSet::new(),
            complained_qcs: BTreeSet::new(),
            view_entry_time: 0,
            current_time: 0,

            vote_tracker: QuorumTrack {
                votes: BTreeMap::new(),
            },
            start_views: BTreeMap::new(),
            index: StateIndex::new(genesis_qc.clone(), genesis_block.clone()),
            produced_lead_in_view: {
                let mut map = BTreeMap::new();
                map.insert(ViewNum(0), false);
                map
            },
            recorded_events: 0,
            recorded_events_table: Some(recorded_events_table),
            snapshots_table: snapshots_table_default::<Tr>(),
            qcs: BTreeSet::from([genesis_qc.clone()]),
            genesis: genesis_block.clone(),
            genesis_qc: genesis_qc.clone(),
            ready_transactions: Vec::new(),
            pending_votes: BTreeMap::new(),
            seen_messages: BloomFilter::with_num_bits(8 * 1024 * 16)
                .seed(&0x8F3A57D2C19E4B7F0123456789ABCDEF)
                .expected_items(100_000),
        };
        p.record_event(
            _db,
            Event::ProcessMessage {
                sender: Identity(u32::MAX),
                payload: Message::Block(genesis_block.clone()),
            },
        );
        p.record_event(
            _db,
            Event::ProcessMessage {
                sender: Identity(u32::MAX),
                payload: Message::QC(genesis_qc.clone()),
            },
        );
        p
    }

    /// Records an event to the event log if not replaying
    pub(crate) fn record_event(&mut self, db: &redb::Database, event: Event<Tr>) {
        // Always update the bloom filter for ProcessMessage events
        if let Event::ProcessMessage { ref payload, .. } = event {
            self.seen_messages.insert(payload);
        }

        if self.replaying {
            self.recorded_events += 1;
            return;
        }

        let tx = db.begin_write().unwrap();
        if let Event::ProcessMessage { ref payload, .. } = event {
            {
                let mut seen_tbl = tx.open_table(SEEN_MESSAGE_HASHES_TABLE).unwrap();
                let bytes = postcard::to_stdvec(payload).unwrap();
                let hash = blake3::hash(&bytes);
                seen_tbl.insert(hash.as_bytes(), ()).unwrap();
            }
        }

        {
            let mut tbl = tx.open_table(self.recorded_events_table.unwrap()).unwrap();
            let processed_messages = tbl.len().unwrap();
            tbl.insert(processed_messages, &event).unwrap();
            self.recorded_events = processed_messages + 1;
        }

        tx.commit().unwrap();
    }

    /// Sets ready transactions and records the event
    pub fn set_ready_transactions(&mut self, db: &redb::Database, transactions: Vec<Tr>) {
        self.record_event(db, Event::SetReadyTransactions(transactions.clone()));
        self.ready_transactions = transactions;
    }

    /// Wrapper that records check_timeouts event
    pub fn check_timeouts_recorded(
        &mut self,
        db: &redb::Database,
        to_send: &mut Vec<(Message<Tr>, Option<Identity>)>,
    ) {
        self.record_event(db, Event::CheckTimeouts);
        self.check_timeouts(db, to_send);
    }

    /// Wrapper that records try_produce_blocks event
    pub fn try_produce_blocks_recorded(
        &mut self,
        db: &redb::Database,
        to_send: &mut Vec<(Message<Tr>, Option<Identity>)>,
    ) {
        self.record_event(db, Event::CheckProduceBlocks);
        self.try_produce_blocks(db, to_send);
    }
}
