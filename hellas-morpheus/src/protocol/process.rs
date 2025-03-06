use std::collections::{HashMap, HashSet};
use std::cmp::{Ordering};
use std::time::Instant;

use crate::types::{
    Block, BlockId, BlockType, EndViewMessage, Message, MorpheusProcess, Phase, ProcessId, QcId, QuorumCertificate, SlotNum, ViewMessage, ViewNum, Vote, VoteKind
};

impl MorpheusProcess {
    /// Create a new Morpheus process instance.
    ///
    /// This initializes a process with the given parameters and creates
    /// a genesis block and certificate to bootstrap the consensus process.
    ///
    /// # Arguments
    ///
    /// * `id` - The unique identifier for this process
    /// * `n` - Total number of processes in the system
    /// * `f` - Maximum number of faulty processes the system can tolerate
    ///
    /// # Returns
    ///
    /// A new `MorpheusProcess` instance configured with initial state
    pub fn new(id: ProcessId, n: usize, f: usize) -> Self {
        // Create a genesis block
        let genesis_auth = ProcessId(0);
        let genesis_block_id = BlockId {
            block_type: BlockType::Tr,
            auth: genesis_auth,
            view: ViewNum(0),
            slot: SlotNum(0),
        };
        
        let genesis_block = Block {
            id: genesis_block_id,
            height: 0,
            prev_qcs: Vec::new(),
            one_qc: None,
            justification: Vec::new(),
        };
        
        // Create QC ID for genesis block
        let genesis_qc_id = QcId {
            block_id: genesis_block_id,
        };
        
        // Create a 1-QC certificate for the genesis block
        let genesis_qc = QuorumCertificate {
            id: genesis_qc_id,
            height: 0,
        };
        
        let mut m_i = HashSet::new();
        m_i.insert(Message::Block(genesis_block.clone()));
        
        let mut q_i = HashSet::new();
        q_i.insert(genesis_qc.clone());
        
        let mut blocks = HashMap::new();
        blocks.insert(genesis_block_id, genesis_block);
        
        let mut qcs = HashMap::new();
        qcs.insert(genesis_qc_id, genesis_qc);
        
        MorpheusProcess {
            id,
            m_i,
            q_i,
            view_i: ViewNum(0),
            slot_i: {
                let mut map = HashMap::new();
                map.insert(BlockType::Lead, SlotNum(0));
                map.insert(BlockType::Tr, SlotNum(0));
                map
            },
            voted_i: HashMap::new(),
            phase_i: {
                let mut map = HashMap::new();
                map.insert(ViewNum(0), Phase::High);
                map
            },
            n,
            f,
            view_entry_time: Instant::now(),
            sent_zero_qc: HashSet::new(),
            blocks,
            qcs,
        }
    }

    /// Determine the leader of a view.
    ///
    /// The leader is determined by taking the view number modulo the total number of processes.
    ///
    /// # Arguments
    ///
    /// * `v` - The view number
    ///
    /// # Returns
    ///
    /// The process ID of the leader for the specified view
    pub fn lead(&self, v: ViewNum) -> ProcessId {
        ProcessId(v.0 % self.n as u64)
    }

    /// Process a received message.
    ///
    /// This function updates the process's state based on the content of the received message.
    ///
    /// # Arguments
    ///
    /// * `message` - The message to process
    pub fn process_message(&mut self, message: Message) {
        match message.clone() {
            Message::Block(block) => {
                // Store the block
                self.blocks.insert(block.id, block);
                self.m_i.insert(message);
            },
            Message::Vote(vote) => {
                self.m_i.insert(message);
            },
            Message::QC(qc) => {
                self.qcs.insert(qc.id, qc.clone());
                self.q_i.insert(qc);
                self.m_i.insert(message);
            },
            Message::ViewMsg(vm) => {
                self.m_i.insert(message);
            },
            Message::EndViewMsg(evm) => {
                self.m_i.insert(message);
            },
        }
    }

    /// Compare two QCs according to the ordering relation.
    ///
    /// This comparison follows a specific ordering relation defined by the Morpheus protocol:
    /// 1. First compare by view (lower view < higher view)
    /// 2. For the same view, compare by type (Lead < Tr)
    /// 3. For the same view and type, compare by height
    ///
    /// # Arguments
    ///
    /// * `q` - The first QC to compare
    /// * `q_prime` - The second QC to compare
    ///
    /// # Returns
    ///
    /// The ordering relation between the two QCs
    pub fn compare_qcs(&self, q: &QuorumCertificate, q_prime: &QuorumCertificate) -> Ordering {
        // Compare by view
        match q.id.block_id.view.cmp(&q_prime.id.block_id.view) {
            Ordering::Less => return Ordering::Less,
            Ordering::Greater => return Ordering::Greater,
            Ordering::Equal => {}
        }
        
        // Same view, compare by type (lead < Tr)
        match (q.id.block_id.block_type, q_prime.id.block_id.block_type) {
            (BlockType::Lead, BlockType::Tr) => return Ordering::Less,
            (BlockType::Tr, BlockType::Lead) => return Ordering::Greater,
            _ => {}
        }
        
        // Same view and type, compare by height
        q.height.cmp(&q_prime.height)
    }

    /// Execute one step of the Morpheus algorithm.
    ///
    /// This function implements a single step of the protocol, which may include:
    /// - Updating the view
    /// - Sending votes
    /// - Creating QCs
    /// - Producing new blocks
    /// - Managing view changes
    ///
    /// # Returns
    ///
    /// A vector of messages to be sent to other processes
    pub fn step(&mut self) -> Vec<Message> {
        let mut messages_to_send = Vec::new();
        
        // According to the pseudocode, we should execute only one transition
        // at a time, in order of priority

        // 1. First try to handle view updates
        if self.handle_view_updates(&mut messages_to_send) {
            return messages_to_send;
        }
        
        // 2. Then try to handle voting
        if self.handle_voting(&mut messages_to_send) {
            return messages_to_send;
        }
        
        // 3. Then try to handle block creation
        if self.handle_block_creation(&mut messages_to_send) {
            return messages_to_send;
        }
        
        // 4. Finally try to handle complaints
        self.handle_complaints(&mut messages_to_send);
        
        messages_to_send
    }
} 