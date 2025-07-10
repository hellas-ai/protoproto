use crate::{
    error::{NetworkError, NetworkResult},
    messages::{ConsensusMessage, HellasTicket, NetworkMessage, ProtocolMessage, ReceivedMessage, SignedMessage, StatusMessage},
    NetworkConfig,
};
use anyhow::{Context, Result};
use futures::StreamExt;
use iroh::{endpoint::RemoteInfo, protocol::Router, Endpoint, NodeId, PublicKey, SecretKey};
use iroh_gossip::{
    api::{Event as GossipEvent, GossipSender},
    net::{Gossip, GOSSIP_ALPN},
    proto::TopicId,
};
use std::{
    collections::BTreeSet,
    sync::Arc,
};
use tokio::sync::{mpsc, oneshot, Mutex as TokioMutex};
use tracing::{debug, info, warn, error};

/// Network manager for Hellas node
pub struct Network {
    /// The secret key for this node
    secret_key: SecretKey,
    
    /// The Iroh router
    router: Router,
    
    /// The gossip protocol handler
    gossip: Gossip,
    
    /// Channel for sending outgoing messages
    tx: mpsc::Sender<OutgoingMessage>,
}

/// An outgoing message to be broadcast
struct OutgoingMessage {
    topic: TopicId,
    message: NetworkMessage,
}

impl Network {
    /// Spawn a new network instance
    pub async fn spawn(config: NetworkConfig) -> NetworkResult<Self> {
        // Parse or generate secret key
        let secret_key = if let Some(key_str) = config.secret_key {
            let bytes = hex::decode(&key_str)
                .map_err(|e| NetworkError::Config(format!("Invalid secret key: {}", e)))?;
            SecretKey::try_from_bytes(&bytes)
                .map_err(|e| NetworkError::Config(format!("Invalid secret key: {}", e)))?
        } else {
            SecretKey::generate(rand::rngs::OsRng)
        };
        
        // Build endpoint
        let mut endpoint_builder = Endpoint::builder()
            .secret_key(secret_key.clone())
            .alpns(vec![GOSSIP_ALPN.to_vec()]);
            
        if config.enable_mdns {
            endpoint_builder = endpoint_builder.discovery_n0();
        }
        
        if config.port != 0 {
            endpoint_builder = endpoint_builder.bind_port(config.port);
        }
        
        if config.enable_relay {
            endpoint_builder = endpoint_builder.relay_mode(iroh::RelayMode::Default);
        } else {
            endpoint_builder = endpoint_builder.relay_mode(iroh::RelayMode::Disabled);
        }
        
        let endpoint = endpoint_builder
            .bind()
            .await
            .map_err(|e| NetworkError::Endpoint(e.to_string()))?;
            
        let node_id = endpoint.node_id();
        info!("Network endpoint bound - Node ID: {}", node_id);
        
        // Spawn gossip
        let gossip = Gossip::builder().spawn(endpoint.clone());
        info!("Gossip protocol spawned");
        
        // Build router
        let router = Router::builder(endpoint)
            .accept(GOSSIP_ALPN, gossip.clone())
            .spawn();
        info!("Router spawned");
        
        // Create message sending channel
        let (tx, mut rx) = mpsc::channel::<OutgoingMessage>(100);
        
        // Spawn message sending task
        let gossip_clone = gossip.clone();
        let secret_key_clone = secret_key.clone();
        tokio::spawn(async move {
            let mut senders: std::collections::HashMap<TopicId, Arc<TokioMutex<GossipSender>>> = 
                std::collections::HashMap::new();
                
            while let Some(outgoing) = rx.recv().await {
                let sender = match senders.get(&outgoing.topic) {
                    Some(sender) => sender.clone(),
                    None => {
                        error!("No sender for topic {:?}", outgoing.topic);
                        continue;
                    }
                };
                
                match SignedMessage::sign_and_encode(&secret_key_clone, outgoing.message) {
                    Ok(encoded) => {
                        if let Err(e) = sender.lock().await.broadcast(encoded.into()).await {
                            error!("Failed to broadcast message: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Failed to encode message: {}", e);
                    }
                }
            }
        });
        
        Ok(Self {
            secret_key,
            router,
            gossip,
            tx,
        })
    }
    
    /// Get the node ID
    pub fn node_id(&self) -> NodeId {
        self.router.endpoint().node_id()
    }
    
    /// Get information about remote nodes
    pub fn remote_info(&self) -> Vec<RemoteInfo> {
        self.router.endpoint().remote_info_iter().collect()
    }
    
    /// Join a network from a ticket
    pub async fn join(
        &self,
        ticket: &HellasTicket,
    ) -> NetworkResult<NetworkHandle> {
        // Subscribe to consensus topic
        let consensus_topic = self.gossip
            .subscribe(ticket.consensus_topic, ticket.bootstrap.clone())
            .await
            .map_err(|e| NetworkError::Gossip(e.to_string()))?;
            
        let (consensus_sender, consensus_receiver) = consensus_topic.split();
        
        // Subscribe to protocol topic  
        let protocol_topic = self.gossip
            .subscribe(ticket.protocol_topic, ticket.bootstrap.clone())
            .await
            .map_err(|e| NetworkError::Gossip(e.to_string()))?;
            
        let (protocol_sender, protocol_receiver) = protocol_topic.split();
        
        // Create event channel
        let (event_tx, event_rx) = mpsc::channel(1000);
        
        // Spawn consensus receiver task
        let secret_key = self.secret_key.clone();
        tokio::spawn(async move {
            handle_gossip_events(
                consensus_receiver, 
                event_tx.clone(),
                MessageFilter::Consensus,
            ).await;
        });
        
        // Spawn protocol receiver task
        tokio::spawn(async move {
            handle_gossip_events(
                protocol_receiver,
                event_tx,
                MessageFilter::Protocol,
            ).await;
        });
        
        Ok(NetworkHandle {
            consensus_sender: Arc::new(TokioMutex::new(consensus_sender)),
            protocol_sender: Arc::new(TokioMutex::new(protocol_sender)),
            event_rx,
            secret_key: self.secret_key.clone(),
            node_id: self.node_id(),
        })
    }
    
    /// Create a new network with a new chain ID
    pub async fn create(&self, chain_id: [u8; 32]) -> NetworkResult<(HellasTicket, NetworkHandle)> {
        let mut ticket = HellasTicket::new(chain_id);
        ticket.bootstrap.insert(self.node_id());
        
        let handle = self.join(&ticket).await?;
        Ok((ticket, handle))
    }
    
    /// Shutdown the network
    pub async fn shutdown(&self) {
        if let Err(err) = self.router.shutdown().await {
            warn!("Failed to shutdown router cleanly: {}", err);
        }
        self.router.endpoint().close().await;
    }
}

/// Handle to interact with the network
pub struct NetworkHandle {
    consensus_sender: Arc<TokioMutex<GossipSender>>,
    protocol_sender: Arc<TokioMutex<GossipSender>>,
    event_rx: mpsc::Receiver<NetworkEvent>,
    secret_key: SecretKey,
    node_id: NodeId,
}

impl NetworkHandle {
    /// Send a consensus message
    pub async fn send_consensus(&self, message: ConsensusMessage) -> NetworkResult<()> {
        let network_msg = NetworkMessage::Consensus(message);
        let encoded = SignedMessage::sign_and_encode(&self.secret_key, network_msg)
            .map_err(|e| NetworkError::Serialization(e.to_string()))?;
            
        self.consensus_sender
            .lock()
            .await
            .broadcast(encoded.into())
            .await
            .map_err(|e| NetworkError::Gossip(e.to_string()))?;
            
        Ok(())
    }
    
    /// Send a protocol message
    pub async fn send_protocol(&self, message: ProtocolMessage) -> NetworkResult<()> {
        let network_msg = NetworkMessage::Protocol(message);
        let encoded = SignedMessage::sign_and_encode(&self.secret_key, network_msg)
            .map_err(|e| NetworkError::Serialization(e.to_string()))?;
            
        self.protocol_sender
            .lock()
            .await
            .broadcast(encoded.into())
            .await
            .map_err(|e| NetworkError::Gossip(e.to_string()))?;
            
        Ok(())
    }
    
    /// Send a status message to both topics
    pub async fn send_status(&self, status: StatusMessage) -> NetworkResult<()> {
        let network_msg = NetworkMessage::Status(status);
        let encoded = SignedMessage::sign_and_encode(&self.secret_key, network_msg)
            .map_err(|e| NetworkError::Serialization(e.to_string()))?;
            
        // Send to both topics
        let consensus_fut = self.consensus_sender
            .lock()
            .await
            .broadcast(encoded.clone().into());
            
        let protocol_fut = self.protocol_sender
            .lock()
            .await
            .broadcast(encoded.into());
            
        // Wait for both
        let (r1, r2) = tokio::join!(consensus_fut, protocol_fut);
        
        r1.map_err(|e| NetworkError::Gossip(e.to_string()))?;
        r2.map_err(|e| NetworkError::Gossip(e.to_string()))?;
        
        Ok(())
    }
    
    /// Receive the next network event
    pub async fn recv(&mut self) -> Option<NetworkEvent> {
        self.event_rx.recv().await
    }
    
    /// Get our node ID
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }
}

/// Network events
#[derive(Debug)]
pub enum NetworkEvent {
    /// A message was received
    Message(ReceivedMessage),
    
    /// A neighbor came online
    NeighborUp(NodeId),
    
    /// A neighbor went offline
    NeighborDown(NodeId),
    
    /// The event stream lagged
    Lagged,
}

/// Message filter for gossip receivers
enum MessageFilter {
    Consensus,
    Protocol,
}

/// Handle gossip events and convert to network events
async fn handle_gossip_events(
    mut receiver: impl StreamExt<Item = Result<GossipEvent>> + Unpin,
    event_tx: mpsc::Sender<NetworkEvent>,
    filter: MessageFilter,
) {
    while let Some(result) = receiver.next().await {
        let event = match result {
            Ok(event) => event,
            Err(e) => {
                error!("Gossip receiver error: {}", e);
                continue;
            }
        };
        
        let network_event = match event {
            GossipEvent::NeighborUp(node_id) => NetworkEvent::NeighborUp(node_id),
            GossipEvent::NeighborDown(node_id) => NetworkEvent::NeighborDown(node_id),
            GossipEvent::Lagged => NetworkEvent::Lagged,
            GossipEvent::Received(message) => {
                match SignedMessage::verify_and_decode(&message.content) {
                    Ok(received) => {
                        // Filter messages based on type
                        let should_forward = match (&received.message, &filter) {
                            (NetworkMessage::Consensus(_), MessageFilter::Consensus) => true,
                            (NetworkMessage::Protocol(_), MessageFilter::Protocol) => true,
                            (NetworkMessage::Status(_), _) => true, // Status messages go to both
                            _ => false,
                        };
                        
                        if should_forward {
                            NetworkEvent::Message(received)
                        } else {
                            continue;
                        }
                    }
                    Err(e) => {
                        warn!("Failed to verify/decode message: {}", e);
                        continue;
                    }
                }
            }
        };
        
        if event_tx.send(network_event).await.is_err() {
            debug!("Event receiver dropped, stopping gossip handler");
            break;
        }
    }
} 