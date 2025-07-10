use serde::{Deserialize, Serialize};
use hellas_morpheus::{Message as MorpheusMessage, Identity, TestTransaction};
use hellas_protocol::{SignedTransaction, TransactionEffects};
use iroh_base::{ticket::Ticket, Signature};
use iroh::PublicKey;

/// Topic IDs for different gossip channels
pub const CONSENSUS_TOPIC_PREFIX: &str = "hellas-consensus/0:";
pub const PROTOCOL_TOPIC_PREFIX: &str = "hellas-protocol/0:";

/// A ticket for joining the Hellas network
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HellasTicket {
    /// The consensus topic ID
    pub consensus_topic: iroh_gossip::proto::TopicId,
    
    /// The protocol topic ID
    pub protocol_topic: iroh_gossip::proto::TopicId,
    
    /// Bootstrap nodes
    pub bootstrap: std::collections::BTreeSet<iroh::NodeId>,
}

impl HellasTicket {
    pub fn new(chain_id: [u8; 32]) -> Self {
        let consensus_topic = iroh_gossip::proto::TopicId::from_bytes(
            blake3::hash(&[CONSENSUS_TOPIC_PREFIX.as_bytes(), &chain_id].concat()).as_bytes()[..32]
                .try_into()
                .unwrap(),
        );
        
        let protocol_topic = iroh_gossip::proto::TopicId::from_bytes(
            blake3::hash(&[PROTOCOL_TOPIC_PREFIX.as_bytes(), &chain_id].concat()).as_bytes()[..32]
                .try_into()
                .unwrap(),
        );
        
        Self {
            consensus_topic,
            protocol_topic,
            bootstrap: Default::default(),
        }
    }
    
    pub fn serialize(&self) -> String {
        <Self as Ticket>::serialize(self)
    }
    
    pub fn deserialize(input: &str) -> anyhow::Result<Self> {
        <Self as Ticket>::deserialize(input).map_err(Into::into)
    }
}

impl Ticket for HellasTicket {
    const KIND: &'static str = "hellas";
    
    fn to_bytes(&self) -> Vec<u8> {
        postcard::to_stdvec(&self).unwrap()
    }
    
    fn from_bytes(bytes: &[u8]) -> Result<Self, iroh_base::ticket::ParseError> {
        let ticket = postcard::from_bytes(bytes)?;
        Ok(ticket)
    }
}

/// Wire message format for signed messages
#[derive(Debug, Serialize, Deserialize)]
pub struct SignedMessage {
    pub from: PublicKey,
    pub data: Vec<u8>,
    pub signature: Signature,
    pub timestamp: u64,
}

impl SignedMessage {
    pub fn sign_and_encode(
        secret_key: &iroh::SecretKey,
        message: NetworkMessage,
    ) -> anyhow::Result<Vec<u8>> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64;
            
        let wire_message = WireMessage::V0 { timestamp, message };
        let data = postcard::to_stdvec(&wire_message)?;
        let signature = secret_key.sign(&data);
        let from = secret_key.public();
        
        let signed_message = Self {
            from,
            data,
            signature,
            timestamp,
        };
        
        let encoded = postcard::to_stdvec(&signed_message)?;
        Ok(encoded)
    }
    
    pub fn verify_and_decode(bytes: &[u8]) -> anyhow::Result<ReceivedMessage> {
        let signed_message: Self = postcard::from_bytes(bytes)?;
        let key: PublicKey = signed_message.from;
        key.verify(&signed_message.data, &signed_message.signature)?;
        
        let wire_message: WireMessage = postcard::from_bytes(&signed_message.data)?;
        let WireMessage::V0 { timestamp, message } = wire_message;
        
        Ok(ReceivedMessage {
            from: signed_message.from,
            timestamp,
            message,
        })
    }
}

/// Versioned wire message format
#[derive(Debug, Serialize, Deserialize)]
pub enum WireMessage {
    V0 { timestamp: u64, message: NetworkMessage },
}

/// Received message with metadata
#[derive(Debug)]
pub struct ReceivedMessage {
    pub from: iroh::NodeId,
    pub timestamp: u64,
    pub message: NetworkMessage,
}

/// All possible network messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    /// Consensus-related messages
    Consensus(ConsensusMessage),
    
    /// Protocol-related messages
    Protocol(ProtocolMessage),
    
    /// Node status/heartbeat
    Status(StatusMessage),
}

/// Consensus-specific messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusMessage {
    /// A Morpheus block
    Block(Vec<u8>), // Serialized Arc<Signed<Block<HellasTransaction>>>
    
    /// A vote for a block
    Vote(Vec<u8>), // Serialized Arc<ThreshPartial<VoteData>>
    
    /// A quorum certificate
    QC(Vec<u8>), // Serialized Arc<ThreshSigned<VoteData>>
    
    /// Start view message
    StartView(Vec<u8>), // Serialized Arc<Signed<StartView>>
    
    /// End view message
    EndView(Vec<u8>), // Serialized Arc<ThreshPartial<ViewNum>>
    
    /// End view certificate
    EndViewCert(Vec<u8>), // Serialized Arc<ThreshSigned<ViewNum>>
    
    /// Request for missing blocks/QCs
    SyncRequest {
        from_height: u64,
        to_height: u64,
    },
    
    /// Response to sync request
    SyncResponse {
        blocks: Vec<Vec<u8>>, // Serialized blocks
        qcs: Vec<Vec<u8>>,    // Serialized QCs
    },
}

/// Protocol-specific messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProtocolMessage {
    /// Submit a transaction
    Transaction(Vec<u8>), // Serialized SignedTransaction
    
    /// Transaction execution result
    TransactionResult {
        tx_hash: [u8; 32],
        effects: Vec<u8>, // Serialized TransactionEffects
    },
    
    /// Request object state
    ObjectRequest {
        object_ids: Vec<[u8; 32]>,
    },
    
    /// Object state response
    ObjectResponse {
        objects: Vec<Vec<u8>>, // Serialized objects
    },
}

/// Node status messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusMessage {
    /// Node's identity in the consensus
    pub consensus_identity: u32,
    
    /// Current view number
    pub current_view: i64,
    
    /// Current block height
    pub block_height: u64,
    
    /// Is the node synced
    pub is_synced: bool,
} 