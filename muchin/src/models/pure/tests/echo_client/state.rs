use crate::automaton::{Timeout, Uid};

#[derive(Debug)]
pub struct EchoClientConfig {
    pub connect_to_address: String,
    pub connect_timeout: Timeout,
    pub poll_timeout: u64,
    pub max_connection_attempts: usize,
    pub retry_interval_ms: u64,
    pub max_send_size: u64,
    pub min_rnd_timeout: u64,
    pub max_rnd_timeout: u64,
}

#[derive(Debug)]
pub enum EchoClientStatus {
    Init,
    Connecting,
    Connected {
        connection: Uid,
    },
    Sending {
        connection: Uid,
        request: Uid,
        data: Vec<u8>,
    },
    Receiving {
        connection: Uid,
        request: Uid,
        sent_data: Vec<u8>,
    },
}

#[derive(Debug)]
pub struct EchoClientState {
    pub status: EchoClientStatus,
    pub connection_attempt: usize,
    pub config: EchoClientConfig,
}

impl EchoClientState {
    pub fn from_config(config: EchoClientConfig) -> Self {
        Self {
            status: EchoClientStatus::Init,
            connection_attempt: 0,
            config,
        }
    }
}
