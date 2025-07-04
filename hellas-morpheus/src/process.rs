use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::state_tracking::{PendingVotes, StateIndex};
use crate::*;
use redb::{ReadableTable, ReadableTableMetadata, TableDefinition};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct ArkSerialize<T>(pub T);

impl<T> redb::Value for ArkSerialize<T>
where
    T: Debug + CanonicalDeserialize + CanonicalSerialize,
{
    type SelfType<'a>
        = T
    where
        Self: 'a;

    type AsBytes<'a>
        = Vec<u8>
    where
        Self: 'a;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        T::deserialize_compressed(data).unwrap()
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'a,
        Self: 'b,
    {
        let mut writer = Vec::new();
        T::serialize_compressed(value, &mut writer).unwrap();
        writer
    }

    fn type_name() -> redb::TypeName {
        redb::TypeName::new(&format!("ArkSerialize<{}>", std::any::type_name::<T>()))
    }
}

impl<T> redb::Key for ArkSerialize<T>
where
    T: Debug + CanonicalDeserialize + CanonicalSerialize + Ord,
{
    fn compare(data1: &[u8], data2: &[u8]) -> std::cmp::Ordering {
        use redb::Value;
        Self::from_bytes(data1).cmp(&Self::from_bytes(data2))
    }
}

#[derive(Debug)]
pub struct Postcard<T>(pub T);

impl<T> redb::Value for Postcard<T>
where
    T: Debug + Serialize + for<'a> Deserialize<'a>,
{
    type SelfType<'a>
        = T
    where
        Self: 'a;

    type AsBytes<'a>
        = Vec<u8>
    where
        Self: 'a;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        postcard::from_bytes(data).unwrap()
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'a,
        Self: 'b,
    {
        postcard::to_stdvec(value).unwrap()
    }

    fn type_name() -> redb::TypeName {
        redb::TypeName::new(&format!("Postcard<{}>", std::any::type_name::<T>()))
    }
}

impl<T> redb::Key for Postcard<T>
where
    T: Debug + Serialize + for<'a> Deserialize<'a> + Ord,
{
    fn compare(data1: &[u8], data2: &[u8]) -> std::cmp::Ordering {
        use redb::Value;
        Self::from_bytes(data1).cmp(&Self::from_bytes(data2))
    }
}

/// MorpheusProcess represents a single process (p_i) in the Morpheus protocol
///
/// This struct implements the Algorithm 1 from the Morpheus pseudocode,
/// maintaining all state required for processing messages, voting, and
/// producing blocks according to the protocol specification.
#[derive(Clone, derive_more::Debug, Serialize, Deserialize)]
pub struct MorpheusProcess<Tr: Transaction> {
    pub kb: KeyBook,

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
    #[serde(with = "serde_json_any_key::any_key_map")]
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
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub start_views: BTreeMap<ViewNum, Vec<Arc<Signed<StartView>>>>,

    pub index: StateIndex<Tr>,

    /// Tracks whether we've produced a leader block in each view
    /// Used for leader logic to avoid producing multiple leader blocks in same view
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub produced_lead_in_view: BTreeMap<ViewNum, bool>,

    /// All messages received by this process
    pub received_messages: u64,
    pub received_messages_bloom: fastbloom::BloomFilter,
    #[debug(skip)]
    #[serde(default = "received_messages_table_default")]
    #[serde(skip)]
    pub received_messages_table: Option<TableDefinition<'static, u64, Postcard<Message<Tr>>>>,

    pub qcs: BTreeSet<FinishedQC>,

    pub genesis: Arc<Signed<Block<Tr>>>,
    pub genesis_qc: FinishedQC,
    pub ready_transactions: Vec<Tr>,

    pub pending_votes: BTreeMap<ViewNum, PendingVotes>,
}

pub(crate) fn received_messages_table_default<Tr: Transaction>()
-> Option<TableDefinition<'static, u64, Postcard<Message<Tr>>>> {
    Some(TableDefinition::new("received_messages"))
}

const RECEIVED_MESSAGES_BLOOM_TABLE: TableDefinition<
    'static,
    (),
    Postcard<fastbloom::BloomFilter>,
> = TableDefinition::new("received_messages_bloom");

impl<Tr: Transaction> MorpheusProcess<Tr> {
    pub fn new(db: &redb::Database, keybook: KeyBook, id: Identity, n: u32, f: u32) -> Self {
        crate::tracing_setup::register_process(&id, n, f);

        let received_messages_table = received_messages_table_default::<Tr>().unwrap();

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

        let (received_messages, received_messages_bloom) = {
            let tx = db.begin_write().unwrap();
            let val = {
                let mut recvd_tbl = tx.open_table(received_messages_table).unwrap();
                let mut bloom_tbl = tx.open_table(RECEIVED_MESSAGES_BLOOM_TABLE).unwrap();
                let received_messages = recvd_tbl.len().unwrap();
                if received_messages == 0 {
                    recvd_tbl
                        .insert(0, Message::Block(genesis_block.clone()))
                        .unwrap();
                    recvd_tbl
                        .insert(1, Message::QC(genesis_qc.clone()))
                        .unwrap();
                    // TODO: fixed size of 16KiB should be justified
                    // TODO: expected_items of 100_000 is tuff, should we do something fancier?
                    let mut filter = fastbloom::BloomFilter::with_num_bits(8 * 1024 * 16)
                        .expected_items(100_000);
                    filter.insert(&Message::Block::<Tr>(genesis_block.clone()));
                    filter.insert(&Message::QC::<Tr>(genesis_qc.clone()));
                    bloom_tbl.insert((), filter.clone()).unwrap();
                    (2, filter)
                } else {
                    let bloom = bloom_tbl
                        .get(())
                        .unwrap()
                        .expect("missing bloom filter from already-initialized database")
                        .value();
                    (received_messages, bloom)
                }
            };
            tx.commit().unwrap();

            val
        };

        MorpheusProcess {
            kb: keybook,
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
            received_messages,
            received_messages_bloom,
            received_messages_table: Some(TableDefinition::new("received_messages")),
            qcs: BTreeSet::from([genesis_qc.clone()]),
            genesis: genesis_block,
            genesis_qc: genesis_qc.clone(),
            ready_transactions: Vec::new(),
            pending_votes: BTreeMap::new(),
        }
    }
}
