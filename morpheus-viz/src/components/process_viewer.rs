use leptos::prelude::*;

/*

#[derive(Serialize, Deserialize)]
pub struct MorpheusProcess {
    /// Identity of this process (equivalent to p_i in the pseudocode)
    pub id: Identity,

    // === Core protocol variables from pseudocode ===
    /// Current view number (view_i in pseudocode)
    /// "Initially 0, represents the present view"
    pub view_i: ViewNum,

    /// Current slot for leader blocks (slot_i(lead) in pseudocode)
    /// "Initially 0, represents present slot" for leader blocks
    pub slot_i_lead: SlotNum,

    /// Current slot for transaction blocks (slot_i(Tr) in pseudocode)
    /// "Initially 0, represents present slot" for transaction blocks
    pub slot_i_tr: SlotNum,

    /// Tracks which blocks this process has already voted for (voted_i(z,x,s,p_j) in pseudocode)
    /// "Initially 0" for all combinations of z, x, s, p_j
    /// Used to ensure process votes only once for each (z,x,s,p_j) combination
    pub voted_i: BTreeSet<(u8, BlockType, SlotNum, Identity)>,

    /// Tracks the phase within each view (phase_i(v) in pseudocode)
    /// "Initially 0" for each view, represents high throughput (0) or low throughput (1) phase
    pub phase_i: BTreeMap<ViewNum, Phase>,

    /// Total number of processes in the system
    pub n: usize,

    /// Maximum number of faulty processes tolerated (n-f is the quorum size)
    pub f: usize,

    /// Network delay parameter (Δ in pseudocode)
    /// Used for timeouts in the protocol (6Δ and 12Δ)
    pub delta: u128,

    // === Implementation-specific auxiliary variables ===
    /// Tracks end-view messages for view changes
    /// Used to form (v+1)-certificates when f+1 end-view v messages are collected
    pub end_views: VoteTrack<ViewNum>,

    /// Tracks which 0-QCs have been sent to avoid duplicates
    /// Implements "p_i has not previously sent a 0-QC for b to other processors"
    pub zero_qcs_sent: BTreeSet<BlockKey>,

    /// Tracks which QCs we've already complained about to the leader
    /// Implements "Send q to lead(view_i) if not previously sent"
    pub complained_qcs: BTreeSet<VoteData>,

    /// Time when this process entered the current view
    /// Used for timeout calculations (6Δ and 12Δ since entering view)
    pub view_entry_time: u128,

    /// Current logical time
    pub current_time: u128,

    // === State tracking fields (corresponding to M_i and Q_i in pseudocode) ===
    /// Tracks votes for each VoteData to form quorums
    /// Part of M_i in pseudocode - "the set of all received messages"
    pub vote_tracker: VoteTrack<VoteData>,

    /// Tracks view change messages
    /// Used to collect view v messages with 1-QCs sent to the leader
    pub start_views: BTreeMap<ViewNum, Vec<Arc<Signed<StartView>>>>,

    /// Stores QCs indexed by their VoteData
    /// Part of Q_i in pseudocode - "stores at most one z-QC for each block"
    pub qcs: BTreeMap<VoteData, Arc<ThreshSigned<VoteData>>>,

    /// Tracks the maximum view seen and its associated VoteData
    pub max_view: (ViewNum, VoteData),

    /// Tracks the maximum height block seen and its key
    /// Used for identifying the tallest block in the DAG
    pub max_height: (usize, BlockKey),

    /// Stores the maximum 1-QC seen by this process
    /// Used when entering a new view: "Send (v, q') signed by p_i to lead(v),
    /// where q' is a maximal amongst 1-QCs seen by p_i"
    pub max_1qc: Arc<ThreshSigned<VoteData>>,

    /// Stores all 1-QCs seen by this process
    pub all_1qc: BTreeSet<Arc<ThreshSigned<VoteData>>>,

    /// Stores the current tips of the block DAG
    /// "The tips of Q_i are those q ∈ Q_i such that there does not exist q' ∈ Q_i with q' ≻ q"
    pub tips: Vec<VoteData>,

    /// Maps block keys to blocks (part of M_i in pseudocode)
    /// Implements part of "the set of all received messages"
    pub blocks: BTreeMap<BlockKey, Arc<Signed<Block>>>,

    /// Tracks which blocks point to which other blocks
    /// Used to efficiently determine the DAG structure
    pub block_pointed_by: BTreeMap<BlockKey, BTreeSet<BlockKey>>,

    /// Tracks unfinalized blocks with 2-QC
    /// Used to identify blocks that have 2-QC but are not yet finalized
    pub unfinalized_2qc: BTreeSet<VoteData>,

    /// Maps block keys to their finalization status
    /// Used to track which blocks have been finalized
    pub finalized: BTreeMap<BlockKey, bool>,

    /// Maps block keys to their unfinalized QCs
    /// Used to track which QCs are not yet finalized
    pub unfinalized: BTreeMap<BlockKey, BTreeSet<VoteData>>,

    /// Tracks whether we've seen a leader block for each view
    /// Used to implement logic that depends on leader blocks within a view
    pub contains_lead_by_view: BTreeMap<ViewNum, bool>,

    /// Maps views to sets of unfinalized leader blocks
    /// Tracks which leader blocks are not yet finalized by view
    pub unfinalized_lead_by_view: BTreeMap<ViewNum, BTreeSet<BlockKey>>,

    // === Performance optimization indexes ===
    /// Quick index to QCs by block type, author, and slot
    /// Enables O(1) lookup of QCs
    pub qc_by_slot: BTreeMap<(BlockType, Identity, SlotNum), Arc<ThreshSigned<VoteData>>>,

    /// Maps (type, author, view) to QCs for efficient retrieval
    /// Used to find QCs for a specific block type, author, and view
    pub qc_by_view: BTreeMap<(BlockType, Identity, ViewNum), Vec<Arc<ThreshSigned<VoteData>>>>,

    /// Maps (type, view, author) to blocks for efficient retrieval
    /// Used to find blocks of a specific type, view, and author
    pub block_index: BTreeMap<(BlockType, ViewNum, Identity), Vec<Arc<Signed<Block>>>>,

    /// Tracks whether we've produced a leader block in each view
    /// Used for leader logic to avoid producing multiple leader blocks in same view
    pub produced_lead_in_view: BTreeMap<ViewNum, bool>,

    /// All messages received by this process
    pub received_messages: BTreeSet<Message>,

    pub genesis: Arc<Block>,
    pub genesis_qc: Arc<ThreshSigned<VoteData>>,
    pub ready_transactions: Vec<Transaction>,
}

 */
/// A parameterized incrementing button
#[component]
pub fn Button(#[prop(default = 1)] increment: i32) -> impl IntoView {
    let (count, set_count) = signal(0);
    view! {
        <button on:click=move |_| {
            set_count(count() + increment)
        }>

            "Click me: " {count}
        </button>
    }
}
