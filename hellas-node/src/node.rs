//! High-level node implementation that integrates consensus, protocol, and networking

use crate::{
    config::{ConsensusConfig, NetworkConfig, NodeConfig, ProtocolConfig},
    error::{NodeError, NodeResult},
    messages::{ConsensusMessage, HellasTicket, NetworkMessage, ProtocolMessage, StatusMessage},
    network::{Network, NetworkEvent, NetworkHandle},
};

use hellas_morpheus::{
    {InvariantCheckConfig, RedbBulkStore, RedbSnapshotStore},
    Action, Block, BlockData, BlockKey, Identity, KeyBook, Message as MorpheusMessage, MorpheusProcess,
    StartView, Transaction, ViewNum, VoteData, hints,
};

use hellas_protocol::{
    HellasAccount, JobEscrow, JobStatus, Object, ObjectId, Pubkey as HellasPubkey, SignedTransaction,
    StateTransitionEngine, TransactionCertificate, TransactionEffects,
};

use iroh::{PublicKey, SecretKey};

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::{interval, MissedTickBehavior},
};

use anyhow::Result;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

/// Hellas protocol transaction that implements morpheus Transaction trait
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct HellasTransaction {
    pub inner: SignedTransaction,
}

impl Default for HellasTransaction {
    fn default() -> Self {
        Self {
            inner: SignedTransaction::default(),
        }
    }
}

impl ark_serialize::Valid for HellasTransaction {
    fn check(&self) -> Result<(), ark_serialize::SerializationError> {
        Ok(())
    }
}

impl ark_serialize::CanonicalSerialize for HellasTransaction {
    fn serialize_with_mode<W: std::io::Write>(
        &self,
        writer: W,
        compress: ark_serialize::Compress,
    ) -> Result<(), ark_serialize::SerializationError> {
        let bytes = postcard::to_stdvec(&self)
            .map_err(|_| ark_serialize::SerializationError::InvalidData)?;
        bytes.serialize_with_mode(writer, compress)
    }

    fn serialized_size(&self, compress: ark_serialize::Compress) -> usize {
        let bytes = postcard::to_stdvec(&self).unwrap_or_default();
        bytes.serialized_size(compress)
    }
}

impl ark_serialize::CanonicalDeserialize for HellasTransaction {
    fn deserialize_with_mode<R: std::io::Read>(
        reader: R,
        compress: ark_serialize::Compress,
        validate: ark_serialize::Validate,
    ) -> Result<Self, ark_serialize::SerializationError> {
        let bytes = Vec::<u8>::deserialize_with_mode(reader, compress, validate)?;
        postcard::from_bytes(&bytes).map_err(|_| ark_serialize::SerializationError::InvalidData)
    }
}

// Implement morpheus Transaction trait for HellasTransaction
impl Transaction for HellasTransaction {}

/// Node command for external control
#[derive(Debug)]
pub enum NodeCommand {
    /// Submit a transaction
    SubmitTransaction {
        transaction: SignedTransaction,
        response: oneshot::Sender<Result<TransactionEffects>>,
    },
    /// Query object state
    QueryObject {
        object_id: ObjectId,
        response: oneshot::Sender<Result<Option<Object>>>,
    },
    /// Get node status
    GetStatus {
        response: oneshot::Sender<NodeStatus>,
    },
    /// Shut down the node
    Shutdown,
}

/// Node status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    pub node_id: PublicKey,
    pub consensus_view: i64,
    pub finalized_blocks: usize,
    pub pending_transactions: usize,
    pub connected_peers: Vec<PublicKey>,
    pub is_leader: bool,
}

/// Handle for interacting with a running node
#[derive(Clone)]
pub struct NodeHandle {
    command_tx: mpsc::Sender<NodeCommand>,
}

impl NodeHandle {
    /// Submit a transaction to the node
    pub async fn submit_transaction(
        &self,
        transaction: SignedTransaction,
    ) -> Result<TransactionEffects> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(NodeCommand::SubmitTransaction {
                transaction,
                response: response_tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Node is not running"))?;

        response_rx.await?
    }

    /// Query object state
    pub async fn query_object(&self, object_id: ObjectId) -> Result<Option<Object>> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(NodeCommand::QueryObject {
                object_id,
                response: response_tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Node is not running"))?;

        response_rx.await?
    }

    /// Get node status
    pub async fn get_status(&self) -> Result<NodeStatus> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(NodeCommand::GetStatus {
                response: response_tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Node is not running"))?;

        Ok(response_rx.await?)
    }

    /// Shutdown the node
    pub async fn shutdown(&self) -> Result<()> {
        self.command_tx
            .send(NodeCommand::Shutdown)
            .await
            .map_err(|_| anyhow::anyhow!("Node is not running"))?;
        Ok(())
    }
}

/// Block execution status
#[derive(Debug, Clone)]
struct BlockExecution {
    /// The block that was executed
    block_key: BlockKey,
    /// Transactions that were executed
    transactions: Vec<SignedTransaction>,
    /// Whether this was speculative (1-QC) or finalized (2-QC)
    speculative: bool,
    /// The resulting state version after execution
    state_version: u64,
}

/// High-level node that integrates consensus, protocol, and networking
pub struct Node {
    config: NodeConfig,
    network: Network,
    morpheus: Arc<
        Mutex<
            MorpheusProcess<
                HellasTransaction,
                RedbBulkStore<HellasTransaction>,
                RedbSnapshotStore,
            >,
        >,
    >,
    protocol_engine: Arc<Mutex<StateTransitionEngine>>,
    db: Arc<redb::Database>,
    node_id: PublicKey,
    consensus_id: Identity,
    pending_transactions: Arc<Mutex<Vec<SignedTransaction>>>,
    /// Maps Morpheus block keys to their execution status
    executed_blocks: Arc<Mutex<HashMap<BlockKey, BlockExecution>>>,
    /// Maps Morpheus Identity to hellas-protocol Pubkey
    identity_mapping: Arc<HashMap<Identity, HellasPubkey>>,
    /// Reverse mapping from Pubkey to Identity
    pubkey_to_identity: Arc<HashMap<HellasPubkey, Identity>>,
    /// Tracks which blocks we've seen finalized
    last_finalized_blocks: Arc<Mutex<HashSet<BlockKey>>>,
}

impl Node {
    /// Create a new node
    pub async fn new(config: NodeConfig) -> Result<Self> {
        // Create database
        let db = Arc::new(
            redb::Builder::new().create_with_backend(redb::backends::InMemoryBackend::new())?,
        );

        // Initialize network
        let network = Network::spawn(config.network.clone()).await?;
        let node_id = network.node_id();

        // Map node ID to consensus identity (simple mapping for now)
        let consensus_id = Identity(u32::from_le_bytes(
            node_id.as_bytes()[0..4].try_into().unwrap_or([0; 4]),
        ));

        // Create keybook for consensus (simplified for now)
        let keybook = create_test_keybook(consensus_id, config.consensus.n);

        // Initialize Morpheus consensus
        let bulk_store = RedbBulkStore::new(&db);
        let snapshot_store = RedbSnapshotStore::new(&db);
        let morpheus = MorpheusProcess::new(
            &db,
            keybook,
            consensus_id,
            config.consensus.n,
            config.consensus.f,
            bulk_store,
            snapshot_store,
            Some(InvariantCheckConfig::default()),
        )?;

        // Initialize protocol engine with validators
        let validators = (0..config.consensus.n)
            .map(|i| {
                let id = Identity((i + 1) as u32);
                // Create a hellas Pubkey from the consensus identity
                let pubkey_bytes = id.0.to_le_bytes();
                let mut key_data = [0u8; 32];
                key_data[0..4].copy_from_slice(&pubkey_bytes);
                HellasPubkey::new(key_data)
            })
            .collect::<Vec<_>>();

        let mut protocol_engine = StateTransitionEngine::new(
            validators.clone(),
            config.consensus.f as usize,
        );

        // Create identity mappings
        let mut identity_mapping = HashMap::new();
        let mut pubkey_to_identity = HashMap::new();
        for (i, pubkey) in validators.iter().enumerate() {
            let id = Identity((i + 1) as u32);
            identity_mapping.insert(id, *pubkey);
            pubkey_to_identity.insert(*pubkey, id);
        }

        Ok(Self {
            config,
            network,
            morpheus: Arc::new(Mutex::new(morpheus)),
            protocol_engine: Arc::new(Mutex::new(protocol_engine)),
            db,
            node_id,
            consensus_id,
            pending_transactions: Arc::new(Mutex::new(Vec::new())),
            executed_blocks: Arc::new(Mutex::new(HashMap::new())),
            identity_mapping: Arc::new(identity_mapping),
            pubkey_to_identity: Arc::new(pubkey_to_identity),
            last_finalized_blocks: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    /// Run the node
    pub async fn run(self) -> Result<(JoinHandle<()>, NodeHandle)> {
        let (command_tx, mut command_rx) = mpsc::channel(100);
        let handle = NodeHandle { command_tx };

        // Create and join network
        let (ticket, mut network_handle) = self.network.create(self.config.protocol.chain_id).await?;
        info!("Created network with ticket: {}", ticket.serialize());

        // Spawn main node task
        let task = tokio::spawn(async move {
            info!("Node {} starting", self.node_id);

            // Set up timers
            let mut consensus_timer = interval(Duration::from_millis(100));
            consensus_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

            let mut block_production_timer = interval(Duration::from_secs(1));
            block_production_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

            let mut speculative_execution_timer = interval(Duration::from_millis(500));
            speculative_execution_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    // Handle network events
                    Some(event) = network_handle.recv() => {
                        if let Err(e) = self.handle_network_event(event, &network_handle).await {
                            error!("Error handling network event: {}", e);
                        }
                    }

                    // Handle node commands
                    Some(command) = command_rx.recv() => {
                        match command {
                            NodeCommand::Shutdown => {
                                info!("Node shutting down");
                                break;
                            }
                            _ => {
                                if let Err(e) = self.handle_command(command).await {
                                    error!("Error handling command: {}", e);
                                }
                            }
                        }
                    }

                    // Consensus timer
                    _ = consensus_timer.tick() => {
                        if let Err(e) = self.update_consensus_time().await {
                            error!("Error updating consensus time: {}", e);
                        }
                    }

                    // Block production timer
                    _ = block_production_timer.tick() => {
                        if let Err(e) = self.try_produce_blocks().await {
                            error!("Error producing blocks: {}", e);
                        }
                    }

                    // Speculative execution timer
                    _ = speculative_execution_timer.tick() => {
                        if let Err(e) = self.check_speculative_execution().await {
                            error!("Error checking speculative execution: {}", e);
                        }
                    }
                }
            }

            info!("Node {} stopped", self.node_id);
        });

        Ok((task, handle))
    }

    /// Handle network events
    async fn handle_network_event(&self, event: NetworkEvent, network_handle: &NetworkHandle) -> Result<()> {
        match event {
            NetworkEvent::Message(received) => match received.message {
                NetworkMessage::Consensus(consensus_msg) => {
                    self.handle_consensus_message(received.from, consensus_msg).await?;
                }
                NetworkMessage::Protocol(protocol_msg) => {
                    self.handle_protocol_message(received.from, protocol_msg).await?;
                }
                NetworkMessage::Status(status_msg) => {
                    self.handle_status_message(received.from, status_msg).await?;
                }
            },
            NetworkEvent::NeighborUp(peer) => {
                info!("Peer joined: {}", peer);
            }
            NetworkEvent::NeighborDown(peer) => {
                info!("Peer left: {}", peer);
            }
            NetworkEvent::Lagged => {
                warn!("Network event stream lagged");
            }
        }
        Ok(())
    }

    /// Handle consensus messages
    async fn handle_consensus_message(&self, from: PublicKey, msg: ConsensusMessage) -> Result<()> {
        // Map network ID to consensus ID
        let sender_id = Identity(u32::from_le_bytes(
            from.as_bytes()[0..4].try_into().unwrap_or([0; 4]),
        ));

        // Convert to morpheus message
        let morpheus_msg = match msg {
            ConsensusMessage::Block(block_data) => {
                // Deserialize block
                let block: Arc<hellas_morpheus::Signed<Block<HellasTransaction>>> =
                    postcard::from_bytes(&block_data)?;
                MorpheusMessage::Block(block)
            }
            ConsensusMessage::Vote(vote_data) => {
                // Deserialize vote
                let vote: Arc<hellas_morpheus::ThreshPartial<VoteData>> =
                    postcard::from_bytes(&vote_data)?;
                MorpheusMessage::NewVote(vote)
            }
            ConsensusMessage::QC(qc_data) => {
                // Deserialize QC
                let qc: Arc<hellas_morpheus::ThreshSigned<VoteData>> =
                    postcard::from_bytes(&qc_data)?;
                MorpheusMessage::QC(qc)
            }
            ConsensusMessage::StartView(sv_data) => {
                // Deserialize start view
                let sv: Arc<hellas_morpheus::Signed<StartView>> = postcard::from_bytes(&sv_data)?;
                MorpheusMessage::StartView(sv)
            }
            ConsensusMessage::EndView(ev_data) => {
                // Deserialize end view
                let ev: Arc<hellas_morpheus::ThreshPartial<ViewNum>> =
                    postcard::from_bytes(&ev_data)?;
                MorpheusMessage::EndView(ev)
            }
            ConsensusMessage::EndViewCert(evc_data) => {
                // Deserialize end view cert
                let evc: Arc<hellas_morpheus::ThreshSigned<ViewNum>> =
                    postcard::from_bytes(&evc_data)?;
                MorpheusMessage::EndViewCert(evc)
            }
            ConsensusMessage::SyncRequest { .. } | ConsensusMessage::SyncResponse { .. } => {
                // TODO: Implement sync
                return Ok(());
            }
        };

        // Process message in morpheus
        let messages = {
            let mut morpheus = self.morpheus.lock().unwrap();
            morpheus.process_message(&self.db, morpheus_msg, sender_id)?
        };

        // Send out any resulting messages
        self.broadcast_morpheus_messages(messages).await?;

        Ok(())
    }

    /// Handle protocol messages
    async fn handle_protocol_message(&self, from: PublicKey, msg: ProtocolMessage) -> Result<()> {
        match msg {
            ProtocolMessage::Transaction(tx_bytes) => {
                // Deserialize transaction
                let tx: SignedTransaction = postcard::from_bytes(&tx_bytes)?;
                // Add to pending transactions
                self.pending_transactions.lock().unwrap().push(tx);
            }
            ProtocolMessage::TransactionResult { tx_hash, effects } => {
                // TODO: Handle transaction results from other nodes
            }
            ProtocolMessage::ObjectRequest { object_ids } => {
                // TODO: Respond with requested objects
            }
            ProtocolMessage::ObjectResponse { objects } => {
                // TODO: Handle object responses
            }
        }
        Ok(())
    }

    /// Handle status messages
    async fn handle_status_message(&self, from: PublicKey, msg: StatusMessage) -> Result<()> {
        debug!("Received status from {}: {:?}", from, msg);
        // TODO: Track peer status for network health monitoring
        Ok(())
    }

    /// Handle node commands
    async fn handle_command(&self, command: NodeCommand) -> Result<()> {
        match command {
            NodeCommand::SubmitTransaction {
                transaction,
                response,
            } => {
                // Add to pending transactions
                self.pending_transactions
                    .lock()
                    .unwrap()
                    .push(transaction.clone());

                // Return a pending result - the actual execution will happen when the transaction is included in a block
                let effects = TransactionEffects::new(
                    transaction.digest(),
                    vec![],  // consumed objects
                    vec![],  // created objects
                    vec![],  // mutated objects
                    true,    // success (pending)
                    Some("Transaction submitted, pending execution".to_string()),
                    0,       // gas used
                );
                let _ = response.send(Ok(effects));
            }
            NodeCommand::QueryObject {
                object_id,
                response,
            } => {
                // Query from protocol engine
                let engine = self.protocol_engine.lock().unwrap();
                let latest_version = engine.latest_versions.get(&object_id);
                
                let object = if let Some(&version) = latest_version {
                    let key = hellas_protocol::ObjectKey::new(object_id, version);
                    engine.objects.get(&key)
                        .map(|meta| meta.object.clone())
                } else {
                    None
                };
                
                let _ = response.send(Ok(object));
            }
            NodeCommand::GetStatus { response } => {
                let morpheus = self.morpheus.lock().unwrap();
                let engine = self.protocol_engine.lock().unwrap();
                let executed_blocks = self.executed_blocks.lock().unwrap();
                
                let status = NodeStatus {
                    node_id: self.node_id,
                    consensus_view: morpheus.view_i.0,
                    finalized_blocks: morpheus.index.finalized.len(),
                    pending_transactions: self.pending_transactions.lock().unwrap().len(),
                    connected_peers: vec![], // TODO: Get from network
                    is_leader: morpheus.id == morpheus.lead(morpheus.view_i),
                };
                let _ = response.send(status);
            }
            NodeCommand::Shutdown => unreachable!(),
        }
        Ok(())
    }

    /// Update consensus time and check for newly finalized blocks
    async fn update_consensus_time(&self) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();

        let messages = {
            let mut morpheus = self.morpheus.lock().unwrap();
            morpheus.set_now(&self.db, now)?
        };

        self.broadcast_morpheus_messages(messages).await?;

        // Check timeouts
        let messages = {
            let mut morpheus = self.morpheus.lock().unwrap();
            morpheus.check_timeouts(&self.db)?
        };

        self.broadcast_morpheus_messages(messages).await?;

        // Check for newly finalized blocks and execute them
        self.execute_finalized_blocks().await?;

        Ok(())
    }

    /// Execute transactions from newly finalized blocks
    async fn execute_finalized_blocks(&self) -> Result<()> {
        let mut newly_finalized = Vec::new();

        {
            let morpheus = self.morpheus.lock().unwrap();
            let mut last_finalized = self.last_finalized_blocks.lock().unwrap();
            
            // Find newly finalized blocks
            for block_key in &morpheus.index.finalized {
                if !last_finalized.contains(block_key) {
                    newly_finalized.push(block_key.clone());
                    last_finalized.insert(block_key.clone());
                }
            }
        }

        // Execute transactions from newly finalized blocks
        for block_key in newly_finalized {
            if let Err(e) = self.execute_block(&block_key, false).await {
                error!("Failed to execute finalized block {:?}: {}", block_key, e);
            }
        }

        Ok(())
    }

    /// Execute transactions from a block
    async fn execute_block(&self, block_key: &BlockKey, speculative: bool) -> Result<()> {
        // Check if already executed
        {
            let executed = self.executed_blocks.lock().unwrap();
            if executed.contains_key(block_key) {
                debug!("Block {:?} already executed", block_key);
                return Ok(());
            }
        }

        // Get the block
        let block = {
            let morpheus = self.morpheus.lock().unwrap();
            morpheus.index.blocks.get(block_key).cloned()
        };

        let block = block.ok_or_else(|| anyhow::anyhow!("Block not found: {:?}", block_key))?;

        // Extract transactions from the block
        let transactions = match &block.data.data {
            BlockData::Tr { transactions } => {
                transactions.iter()
                    .map(|tx| tx.inner.clone())
                    .collect::<Vec<_>>()
            }
            _ => {
                // Not a transaction block
                return Ok(());
            }
        };

        if transactions.is_empty() {
            return Ok(());
        }

        info!(
            "Executing {} transactions from block {:?} (speculative: {})",
            transactions.len(),
            block_key,
            speculative
        );

        // Execute transactions in the protocol engine
        let mut engine = self.protocol_engine.lock().unwrap();
        
        // Create a snapshot for speculative execution if needed
        let snapshot_engine = if speculative {
            let mut snapshot = engine.create_snapshot();
            let block_hash = block_key.hash
                .map(|h| {
                    let mut bytes = [0u8; 32];
                    bytes[0..8].copy_from_slice(&h.0.to_le_bytes());
                    hellas_protocol::Hash::new(bytes)
                })
                .unwrap_or_else(|| hellas_protocol::Hash::new([0u8; 32]));
            snapshot.begin_speculative_execution(block_hash);
            Some(snapshot)
        } else {
            None
        };

        let engine_to_use = snapshot_engine.as_ref().unwrap_or(&mut *engine);

        // Execute each transaction
        let mut executed_txs = Vec::new();
        for tx in &transactions {
            // Map signer from hellas Pubkey to consensus Identity
            let signer_id = self.pubkey_to_identity.get(&tx.signer)
                .ok_or_else(|| anyhow::anyhow!("Unknown signer: {:?}", tx.signer))?;

            // Get the proposing validator's pubkey
            let proposer_pubkey = block_key.author
                .and_then(|id| self.identity_mapping.get(&id))
                .copied()
                .unwrap_or(tx.signer);

            // First process the transaction (validation and locking)
            match engine_to_use.process_transaction(tx) {
                Ok(()) => {
                    // Create a certificate for execution
                    let certificate = TransactionCertificate {
                        transaction: tx.clone(),
                        auth_signatures: vec![], // Would be filled in production
                    };

                    // Execute the certificate
                    match engine_to_use.execute_certificate(&certificate, proposer_pubkey) {
                        Ok(effects) => {
                            debug!("Executed transaction {:?} with effects: {:?}", tx.digest(), effects);
                            executed_txs.push(tx.clone());
                        }
                        Err(e) => {
                            warn!("Failed to execute transaction {:?}: {}", tx.digest(), e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to process transaction {:?}: {}", tx.digest(), e);
                }
            }
        }

        // Record block execution
        let state_version = engine_to_use.state_version;
        let execution = BlockExecution {
            block_key: block_key.clone(),
            transactions: executed_txs,
            speculative,
            state_version,
        };

        self.executed_blocks.lock().unwrap().insert(block_key.clone(), execution);

        // If this was speculative execution, we might need to commit or rollback later
        if let Some(snapshot) = snapshot_engine {
            if !speculative {
                // This shouldn't happen, but handle it
                warn!("Had snapshot for non-speculative execution");
            } else {
                // For now, we'll commit speculative executions immediately
                // In a full implementation, we'd wait for 2-QC finalization
                let block_hash = block_key.hash
                    .map(|h| {
                        let mut bytes = [0u8; 32];
                        bytes[0..8].copy_from_slice(&h.0.to_le_bytes());
                        hellas_protocol::Hash::new(bytes)
                    })
                    .unwrap_or_else(|| hellas_protocol::Hash::new([0u8; 32]));
                engine.commit_snapshot(snapshot, block_hash)?;
            }
        }

        Ok(())
    }

    /// Check for blocks with 1-QCs that can be speculatively executed
    async fn check_speculative_execution(&self) -> Result<()> {
        let blocks_with_1qc = {
            let morpheus = self.morpheus.lock().unwrap();
            let mut blocks = Vec::new();
            
            // Find blocks with 1-QCs that aren't finalized yet
            for (block_key, qcs) in &morpheus.index.unfinalized {
                if qcs.iter().any(|qc| qc.data.z == 1) 
                    && !morpheus.index.finalized.contains(block_key) {
                    blocks.push(block_key.clone());
                }
            }
            
            blocks
        };

        // Speculatively execute blocks with 1-QCs
        for block_key in blocks_with_1qc {
            let executed = self.executed_blocks.lock().unwrap();
            if !executed.contains_key(&block_key) {
                drop(executed); // Release lock before async call
                if let Err(e) = self.execute_block(&block_key, true).await {
                    debug!("Failed to speculatively execute block {:?}: {}", block_key, e);
                }
            }
        }

        Ok(())
    }

    /// Try to produce blocks
    async fn try_produce_blocks(&self) -> Result<()> {
        // Set ready transactions
        let pending = self.pending_transactions.lock().unwrap().clone();
        if !pending.is_empty() {
            let hellas_txs: Vec<HellasTransaction> = pending
                .into_iter()
                .map(|tx| HellasTransaction { inner: tx })
                .collect();

            let messages = {
                let mut morpheus = self.morpheus.lock().unwrap();
                morpheus.set_ready_transactions(&self.db, hellas_txs)?
            };

            self.broadcast_morpheus_messages(messages).await?;

            // Clear pending after setting
            self.pending_transactions.lock().unwrap().clear();
        }

        // Try to produce blocks
        let messages = {
            let mut morpheus = self.morpheus.lock().unwrap();
            morpheus.try_produce_blocks(&self.db)?
        };

        self.broadcast_morpheus_messages(messages).await?;

        Ok(())
    }

    /// Broadcast morpheus messages to the network
    async fn broadcast_morpheus_messages(
        &self,
        messages: Vec<(MorpheusMessage<HellasTransaction>, Option<Identity>)>,
    ) -> Result<()> {
        // Get network handle from a shared reference
        // In a real implementation, we'd store the network handle as a field
        // For now, we'll skip the actual sending
        for (msg, target) in messages {
            debug!("Would broadcast message: {:?} to {:?}", msg, target);
            // TODO: Implement actual message broadcasting
        }
        Ok(())
    }

    /// Join a network using a ticket
    pub async fn join_network(&self, ticket: HellasTicket) -> Result<()> {
        self.network.join(ticket).await
    }

    /// Create a ticket for others to join
    pub fn create_ticket(&self) -> Result<HellasTicket> {
        self.network.create_ticket()
    }
}

// Helper function to create test keybook (simplified for now)
fn create_test_keybook(my_id: Identity, n: u32) -> KeyBook {
    use ark_std::test_rng;
    use std::collections::BTreeMap;

    let domain_max = (1 + n as usize).next_power_of_two();
    let gd = hints::GlobalData::new(domain_max, &mut test_rng()).unwrap();
    let privs = vec![hints::SecretKey::random(&mut test_rng()); domain_max - 1];
    let pubkeys: Vec<hints::PublicKey> = privs.iter().map(|sk| sk.public(&gd)).collect();
    let weights = vec![hints::F::from(1); domain_max - 1];

    let hints = (0..domain_max - 1)
        .map(|i| hints::generate_hint(&gd, &privs[i], domain_max, i).unwrap())
        .collect::<Vec<_>>();

    let setup = hints::setup_universe(&gd, pubkeys.clone(), &hints, weights).unwrap();

    let keys: BTreeMap<Identity, hints::PublicKey> = (0..n)
        .map(|i| (Identity(i as u32 + 1), pubkeys[i as usize].clone()))
        .collect();

    let identities: BTreeMap<hints::PublicKey, Identity> = (0..n)
        .map(|i| (pubkeys[i as usize].clone(), Identity(i as u32 + 1)))
        .collect();

    let my_idx = (my_id.0 - 1) as usize;

    KeyBook {
        keys,
        identities,
        me_identity: my_id,
        me_pub_key: pubkeys[my_idx].clone(),
        me_sec_key: privs[my_idx].clone(),
        hints_setup: Some(setup),
    }
}
