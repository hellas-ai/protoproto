//! High-level node implementation that integrates consensus, protocol, and networking

use crate::{
    config::{ConsensusConfig, NetworkConfig, NodeConfig, ProtocolConfig},
    error::{NodeError, NodeResult},
    messages::{ConsensusMessage, HellasTicket, NetworkMessage, ProtocolMessage, StatusMessage},
    network::{Network, NetworkEvent, NetworkHandle},
};

use hellas_morpheus::{
    {InvariantCheckConfig, RedbBulkStore, RedbSnapshotStore},
    Action, Block, BlockData, Identity, KeyBook, Message as MorpheusMessage, MorpheusProcess,
    StartView, Transaction, ViewNum, VoteData,
};

use hellas_protocol::{
    HellasAccount, JobEscrow, JobStatus, Object, ObjectId, SignedTransaction,
    StateTransitionEngine, TransactionEffects,
};

use iroh::{PublicKey, SecretKey};

use std::{
    collections::{BTreeMap, HashMap},
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
}

impl Node {
    /// Create a new node
    pub async fn new(config: NodeConfig) -> Result<Self> {
        // Create database
        let db = Arc::new(
            redb::Builder::new().create_with_backend(redb::backends::InMemoryBackend::new())?,
        );

        // Initialize network
        let network = Network::new(config.network.clone()).await?;
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

        // Initialize protocol engine
        let protocol_engine = StateTransitionEngine::new();

        Ok(Self {
            config,
            network,
            morpheus: Arc::new(Mutex::new(morpheus)),
            protocol_engine: Arc::new(Mutex::new(protocol_engine)),
            db,
            node_id,
            consensus_id,
            pending_transactions: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Run the node
    pub async fn run(self) -> Result<(JoinHandle<()>, NodeHandle)> {
        let (command_tx, mut command_rx) = mpsc::channel(100);
        let handle = NodeHandle { command_tx };

        // Get network handle
        let network_handle = self.network.handle();
        let mut network_events = network_handle.subscribe_events();

        // Spawn main node task
        let task = tokio::spawn(async move {
            info!("Node {} starting", self.node_id);

            // Set up timers
            let mut consensus_timer = interval(Duration::from_millis(100));
            consensus_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

            let mut block_production_timer = interval(Duration::from_secs(1));
            block_production_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    // Handle network events
                    Some(event) = network_events.recv() => {
                        if let Err(e) = self.handle_network_event(event).await {
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
                }
            }

            info!("Node {} stopped", self.node_id);
        });

        Ok((task, handle))
    }

    /// Handle network events
    async fn handle_network_event(&self, event: NetworkEvent) -> Result<()> {
        match event {
            NetworkEvent::Message { from, message } => match message {
                NetworkMessage::Consensus(consensus_msg) => {
                    self.handle_consensus_message(from, consensus_msg).await?;
                }
                NetworkMessage::Protocol(protocol_msg) => {
                    self.handle_protocol_message(from, protocol_msg).await?;
                }
                NetworkMessage::Status(status_msg) => {
                    self.handle_status_message(from, status_msg).await?;
                }
            },
            NetworkEvent::NeighborUp(peer) => {
                info!("Peer joined: {}", peer);
            }
            NetworkEvent::NeighborDown(peer) => {
                info!("Peer left: {}", peer);
            }
            NetworkEvent::Lag { peer, lag_ms } => {
                debug!("Lag to peer {}: {}ms", peer, lag_ms);
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
            ProtocolMessage::SubmitTransaction(tx) => {
                // Add to pending transactions
                self.pending_transactions.lock().unwrap().push(tx);
            }
            ProtocolMessage::QueryObject { .. } => {
                // TODO: Implement object queries
            }
        }
        Ok(())
    }

    /// Handle status messages
    async fn handle_status_message(&self, from: PublicKey, msg: StatusMessage) -> Result<()> {
        match msg {
            StatusMessage::Heartbeat { .. } => {
                // TODO: Track peer status
            }
        }
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

                // For now, return a dummy effect
                let effects = TransactionEffects::default();
                let _ = response.send(Ok(effects));
            }
            NodeCommand::QueryObject {
                object_id,
                response,
            } => {
                // TODO: Query from protocol engine
                let _ = response.send(Ok(None));
            }
            NodeCommand::GetStatus { response } => {
                let morpheus = self.morpheus.lock().unwrap();
                let status = NodeStatus {
                    node_id: self.node_id,
                    consensus_view: morpheus.view_manager.current_view().0,
                    finalized_blocks: morpheus.finalized_blocks.len(),
                    pending_transactions: self.pending_transactions.lock().unwrap().len(),
                    connected_peers: self.network.handle().connected_peers(),
                    is_leader: false, // TODO: Check if current leader
                };
                let _ = response.send(status);
            }
            NodeCommand::Shutdown => unreachable!(),
        }
        Ok(())
    }

    /// Update consensus time
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
        for (msg, target) in messages {
            let consensus_msg = match msg {
                MorpheusMessage::Block(block) => {
                    ConsensusMessage::Block(postcard::to_stdvec(&block)?.into())
                }
                MorpheusMessage::NewVote(vote) => {
                    ConsensusMessage::Vote(postcard::to_stdvec(&vote)?.into())
                }
                MorpheusMessage::QC(qc) => ConsensusMessage::QC(postcard::to_stdvec(&qc)?.into()),
                MorpheusMessage::StartView(sv) => {
                    ConsensusMessage::StartView(postcard::to_stdvec(&sv)?.into())
                }
                MorpheusMessage::EndView(ev) => {
                    ConsensusMessage::EndView(postcard::to_stdvec(&ev)?.into())
                }
                MorpheusMessage::EndViewCert(evc) => {
                    ConsensusMessage::EndViewCert(postcard::to_stdvec(&evc)?.into())
                }
            };

            let network_msg = NetworkMessage::Consensus(consensus_msg);

            match target {
                Some(id) => {
                    // Map consensus ID to network ID (reverse of earlier mapping)
                    // This is a simplified mapping - in production you'd have a proper mapping
                    let target_bytes = id.0.to_le_bytes();
                    let mut key_bytes = [0u8; 32];
                    key_bytes[0..4].copy_from_slice(&target_bytes);
                    if let Ok(target_key) = PublicKey::try_from(&key_bytes) {
                        self.network
                            .handle()
                            .send_to(target_key, network_msg)
                            .await?;
                    }
                }
                None => {
                    // Broadcast to all
                    self.network.handle().broadcast(network_msg).await?;
                }
            }
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
